# Stack Safety in the F1r3node Rholang Interpreter

### A design and results report on the elimination of depth-proportional native-stack consumption over the `Par` term family

**Repository** `f1r3node-rust-mettail`, branch `feature/mettail`
**Companion repository** `mettail-rust`, branch `feature/rho-native-set-automata` (§5.6)
**Report date** 2026-07-29 · **Measurement tree** `8853f839`
**Audit ledgers superseded by nothing; this report *cites* them** —
`docs/design/audits/theta-depth-traversals-2026-07-26.md`,
`docs/design/audits/four-quadrant-s0-baseline-2026-07-28.md`,
`docs/design/audits/four-quadrant-s2-prost-encoder-2026-07-28.md`

---

## Evidentiary convention

Every factual claim in this report carries one of two tags.

| tag | meaning |
|---|---|
| **DERIVED** | read from source, from a build artefact, or from a commit body. The provenance is named inline (`file:line`, commit hash, or `crate-version/path`). |
| **MEASURED** | observed by executing something. Either **(q)** *quoted* from a recorded measurement, with the commit or document that recorded it named, or **(f)** *freshly measured for this report* on 2026-07-29, with the teed log named. |

A claim with no tag is a definition or an argument, not a fact about the system.
Where a number could **not** be obtained it is written **NOT MEASURED**, with the reason. There are seven such entries; they are collected in §5.9 and §7.

---

## 0. THE FIX REGISTER — the scannable index

★ **This is a living document.** It is maintained on the same standing obligation as the consensus register: when a stack-safety fix lands, it is added here. The register below is the index a reader scans without opening a single body; the identifiers are **stable and never reused**, so a fix may be cited as *"SS-C1"* from a commit message, an issue, or another document and the citation will still resolve after the document is reorganised.

**Adding a fix is filling in a form, not inventing a shape.** The blank form is [Appendix F](#appendix-f--the-per-fix-template-fill-this-in-do-not-invent-a-shape); the mechanism that is supposed to notice when this register goes stale is [Appendix G](#appendix-g--keeping-this-document-current).

**Conventions.** `$`B_0 \rightarrow B_1`$` is bytes of native stack per nesting level before and after, release profile unless the row says otherwise. **0** means *measured flat at both ends of a 4 $`\rightarrow`$ 4,096 ladder in both profiles*. "—" means the axis does not apply; **⌀** means **no measurement exists** (every ⌀ is itemised in [§5.9](#59-measurements-that-could-not-be-obtained)).

★★ **`SS-Y…` is a family added by this revision, and it exists because the register had no way to spell the thing it most needed to say.** The prior families — `SS-A…` core traversals, `SS-B…` evaluator/async, `SS-C…` codecs, `SS-D…` deploy path, `SS-E…` instrument, `SS-F…`/`SS-G…` `mettail-rust`, `SS-X…` rejected — could record a *fix*, a *partial* fix, or a *rejected candidate*, but **not a live unrepaired defect introduced by a fix in this very register**. A register that can only hold good news is a register that reports coverage it does not have. **`SS-Y…` rows are defects that are OPEN at the pinned HEAD**, they are never "class change: yes", and a `SS-Y` row is discharged only by a commit that repairs it — never by the row being deleted. The allocation rule is added to [Appendix F](#appendix-f--the-per-fix-template-fill-this-in-do-not-invent-a-shape) with the others.

| ID | commit | repo | traversal | $`B_0 \rightarrow B_1`$ | class change? | § |
|---|---|---|---|---|:---:|---|
| **SS-A1** | `f0894109` | f1r3node | substitution SCC (strongly connected component), leg-1 de-clone | 36,416 $`\rightarrow`$ 27,179 | **no** ($`-`$ 25.4 % only) | [5.1](#51-family-a--the-substitution-sorting-normalisation-and-evaluation-cores) |
| **SS-A2** | `f11ffb54` | f1r3node | substitution SCC $`\rightarrow`$ explicit worklist | 195,728 $`\rightarrow`$ **0** *(debug)* | **yes** | [5.1](#51-family-a--the-substitution-sorting-normalisation-and-evaluation-cores) |
| **SS-A3** | `6ce7c5b9` | f1r3node | score tree: comparator, sibling walk, `Clone`, `Drop`, `PartialEq` | 1,329 / 201 / 1,578 / 370 / 719 $`\rightarrow`$ **0** | **yes** | [5.1](#51-family-a--the-substitution-sorting-normalisation-and-evaluation-cores) |
| **SS-A4** | `6ce7c5b9` | f1r3node | `ParSortMatcher` — `sort`, `sort_wide` | 78,592 $`\rightarrow`$ **0** *(debug)* | **yes** | [5.1](#51-family-a--the-substitution-sorting-normalisation-and-evaluation-cores) |
| **SS-A5** | `a3fd6fe4` | f1r3node | `rho-pure-eval`'s `eval_with` SCC | 3,359 $`\rightarrow`$ **0** | **yes** | [5.1](#51-family-a--the-substitution-sorting-normalisation-and-evaluation-cores) |
| **SS-A6** | staged | f1r3node | `PrettyPrinter` pushdown driver | ⌀ $`\rightarrow`$ **0** | **yes** | [5.1](#51-family-a--the-substitution-sorting-normalisation-and-evaluation-cores) |
| **SS-A7** | Stage G | f1r3node | `normalize_ann_proc`'s 26-fn SCC $`\rightarrow`$ `norm_drive` | 7,261 $`\rightarrow`$ **0** | **yes** | [5.1](#51-family-a--the-substitution-sorting-normalisation-and-evaluation-cores) |
| **SS-B1** | `a929a2d6` | f1r3node | expression-evaluator SCC $`\rightarrow`$ `eval_drive` | overflow $`\approx`$ 1.5k $`\rightarrow`$ OK at 50,000 | **yes** | [5.2.1](#521-the-expression-evaluator-trampoline-a929a2d6) |
| **SS-B2** | `29856679`, `55b97f84`, `a0a50473` | f1r3node | five async join sites detached | 300 s $`\rightarrow`$ **93.7 s CPU** | **yes** (heap chain) | [5.2.2](#522--the-tokio-fire-and-forget-driver--establishing-the-mechanism-not-assuming-it) |
| **SS-B3** | `9843e4b6` | f1r3node | `StackGrowingFuture` + `stacker` **deleted** | — | dependency removed | [5.2.2](#522--the-tokio-fire-and-forget-driver--establishing-the-mechanism-not-assuming-it) |
| **SS-C1** | `9a5521a2` | f1r3node | cold-store **decoder** (`par_codec`) | 12,894 $`\rightarrow`$ **0** | **yes** | [5.3.3](#533-the-cold-store-decoder--an-obligation-stack-with-eighteen-value-stacks-9a5521a2) |
| **SS-C2** | `c28f4cf6`, `a169cc61` | f1r3node | cold-store **encoder** (`wire_encode`) | ~224 $`\rightarrow`$ **0** | **yes** | [5.3.2](#532-the-cold-store-encoder--a-single-walk-trampolined-serializer-c28f4cf6-a169cc61) |
| **SS-C3** | `7c74260d` | f1r3node | wire-schema generator (one walk, four outputs) | — | enabling | [5.3.2](#532-the-cold-store-encoder--a-single-walk-trampolined-serializer-c28f4cf6-a169cc61) |
| **SS-C4** | `56fb1fd0` | f1r3node | prost encoder: $`\Theta(d^2) \rightarrow \Theta(n)`$ **work** | 302 $`\rightarrow`$ 302 | ⚠ **no** — and **dormant** | [5.3.5](#535-the-prost-network-encoder-56fb1fd0--converted-in-work-not-in-stack-and-dormant) |
| **SS-D1** | `d2591fa1` | f1r3node | task-spawn boundary per-branch deep clone | 2,867 $`\rightarrow`$ **0** *(this site)* | **yes** | [5.5.3](#553-the-three-repairs) |
| **SS-D2** | `94dc983f` | f1r3node | ownership to the substitution; **15** deep copies | incl. $`O(n^2)`$ $`\rightarrow`$ $`O(n)`$ | **yes** | [5.5.3](#553-the-three-repairs) |
| **SS-D3** | `9082d12c` | f1r3node | `inj_attempt` read-back clone $`\rightarrow`$ by-move | 2,852 $`\rightarrow`$ **0** | **yes** | [5.5.3](#553-the-three-repairs) |
| **SS-D4** | `a09f1de2`, `3b265eb7` | f1r3node | gRPC ingress teardown | 96.0 $`\rightarrow`$ **0** | **yes** | [5.5.3](#553-the-three-repairs) |
| **SS-D5** | `64a5d2bc` | f1r3node | metered wrappers take term by value | 2,852 $`\rightarrow`$ **146** | **no** — `encoded_len` remains | [5.5.4](#554-results-and-a-control-that-behaved-exactly-as-predicted) |
| **SS-F1** | `3c0c3585` | mettail | 87-member lowering component $`\rightarrow`$ one worklist | 2,157 $`\rightarrow`$ **1** | **yes** | [5.6.1](#561-the-lowering-component-3c0c3585) |
| **SS-G1** | `9c55d81d`, `651499e2` | mettail | ★★ AST (abstract syntax tree) children `Box` $`\rightarrow`$ `Arc`; `Clone` becomes a refcount bump | 30 GB $`\rightarrow`$ **112 MB**; **0 B/level**; **0 bytes allocated** | **yes**, by *representation* | [5.11](#511--ss-g1--the-arc-fix-eliminating-a-traversal-instead-of-converting-it) |
| **SS-G3** | `ecbe352c`, `f8f71f4c` | mettail | the eight UNMEASURED generated drivers get subjects | ⌀ $`\rightarrow`$ **measured: 8 of 9 SLOPED** | ⚠ **defect found** | [5.10.10](#51010--the-gap-is-now-closed-by-measurement--and-eight-of-the-nine-drivers-are-sloped) |
| **SS-G2** | generator | mettail | nine generated `*_iterative` drivers (`Hash`, `Ord`, `Drop`, `Debug`, `Display`, …) | **flat on a pure chain; 1,215–10,592 debug on an alternating one** | ⚠ **only within one category** — ★ **now HISTORICAL, see SS-G4** | [5.10.10](#51010--the-gap-is-now-closed-by-measurement--and-eight-of-the-nine-drivers-are-sloped) |
| **SS-G4** | `fab6de24`, `6e4abbd8`, `a21b0bf9`, `dc104aa3`, `4ee48db9` (#162); `844364d2` (#189) | mettail | ★★ **ALL ELEVEN generated `ast_*` drivers** — the `CollectionLiteral` arm divergence repaired at the classifier | `ast_cmp` 10,590 · `ast_debug` 10,542 · `ast_eq` 6,144 · `ast_match_pattern` 6,136 · `ast_term_depth` 3,408 · `ast_is_ground` 2,225 $`\rightarrow`$ **0, every one, both profiles** | **yes** — ⚠ **but see SS-Y1** | [8.6.1](#861--162189--the-eleven-generated-drivers-converted-and-the-root-cause-that-unifies-154-with-162) |
| **SS-Y1** | `fab6de24` | mettail | ⚠★★ **A live, unrepaired defect INSIDE `SS-G4`'s OWN COMMIT** — the optional-collection shape: `Option::len` on `Option<Vec<Proc>>` (**E0624**) and `&Vec<Proc>` cast as `*const Proc` (**E0606**) | — | ⛔ **blocks `--all-targets` repo-wide** | [8.6.1a](#861a--197--the-defect-inside-162s-own-commit-live-at-head) |
| **SS-G5** | `ed44c429` | mettail | ★ **the TWELFTH generated driver, `try_eval`** — `CrossKind::OptionalSameCat` replaces a same-category optional child's host recursion with a presence flag; a `compile_error!` refuses the capture-rule shape that would reintroduce it | `ast_try_eval` / `ast_try_eval_cast` **0**, both profiles | **yes** | [5.6.5](#565--the-twelfth-generated-driver-and-the-seven-numerals-beside-it-ed44c429) |
| **SS-G6** | `3276c1ee` | mettail | **#174's hash-keyed collection cost, ATTRIBUTED** — `par_hash` / `par_hashmap` isolate `models`' `impl Hash for Par`; a subtraction control pins the attribution | 625 / 113 recorded with ceilings; ⚠ **both filed figures withdrawn** — `map_pair_lower` 10,491 $`\rightarrow`$ **227**, `list_pair_lower` 950 $`\rightarrow`$ **0** | **no** — a residue is *named*, not converted | [5.6.6](#566--174-attributed-to-models-impl-hash-for-par-3276c1ee) |
| **SS-Y2** | `3276c1ee` | f1r3node | ⚠★★ **A live, unrepaired defect NAMED by `SS-G6`** — `impl Hash for Par` (`models/src/lib.rs:284`) and `impl PartialEq for Par` (`:265`) are **hand-written host-recursive** traversals on a **consensus-adjacent** path (`SortedParMap` feeds the canonical sort `cost_accounting/sig.rs` signs) | 625 debug / 113 release B/level | ⛔ **open**; invisible to **both** existing censuses | [5.6.6](#566--174-attributed-to-models-impl-hash-for-par-3276c1ee) |
| **SS-E1** | `5a744c66`, `ad468163`, `08e876fd`, `6a264e05` | f1r3node | ★ **Phase 3b's PREREQUISITE instrument** — the identical-total-order argument, the sorter golden's first depth-$`\geq 2`$ rows, and the re-entry ladder probe. ⚠ **No traversal was converted**, so this is deliberately not a class change | ⌀ — an instrument, not a traversal | **no** — by construction | [5.6.7](#567--ss-e1-3bs-prerequisite-instrument-and-the-two-checks-that-were-blind) |
| **SS-Y3** | *(pre-existing; MEASURED by `SS-E1`'s `6a264e05`)* | f1r3node | ⛔★★★ **A live, unrepaired defect measured by `SS-E1`** — the three collection arms (`combine_eset` / `combine_emap` / `combine_epathmap`) re-score every element **three times per nesting level**, giving $`\Theta(3^d)`$ on the path that decides **canonical form** | $`3.016\times`$ per level (Ir, baseline-subtracted); $`d{=}14`$ costs **13.63 s**, $`d{=}16`$ **exceeds 120 s** | ⛔ **open** | [5.6.8](#568--ss-y3-the-collection-arms-re-score-every-element-three-times-per-level) |

| **SS-Y4** | *(pre-existing; MEASURED and PINNED by `6bdd6ad7`)* | f1r3node | ⛔★★★ **A live consensus SAFETY FORK** — sibling order is not a total function of the term. `combine_emap` chains only the **key's** score, so distinct canonical terms share a score tree; `sort_vec` is **stable**, so tied siblings keep their input order | seeded: **20/20** split over 40 processes · deterministic: `{3:30} \| {3:90}` ≠ `{3:90} \| {3:30}` | ⛔ **open** — repair route ruled but **not yet landed** | [5.6.9](#569--ss-y4-sibling-order-is-not-a-total-function-of-the-term) |

⚠ **`SS-E1` and `SS-Y3` are a second instance of [Appendix F](#appendix-f--the-per-fix-template-fill-this-in-do-not-invent-a-shape)'s rule 4, in the same *revealed-by* form as `SS-G6`/`SS-Y2`**: `SS-E1`'s commits do not create the defect, they **measure** one that was already live and unquantified. Each cross-references the other, and `SS-Y3` is discharged only by a commit that repairs it — never by deletion.

⚠ **`SS-G6` and `SS-Y2` are the mandatory pair required by [Appendix F](#appendix-f--the-per-fix-template-fill-this-in-do-not-invent-a-shape)'s rule 4**, in its *revealed-by* rather than *introduced-by* form: `SS-G6`'s commit does not create the defect, it **names** one that was already live and unattributed. Each cross-references the other; `SS-Y2` is discharged only by a commit that repairs it.

★ **`SS-Y2` is the strongest available argument for the derived call-graph census**, and it arrived before the census was built. `DERIVE_DISPOSITIONS` closes over derive **tokens** and these impls are hand-written; `GENERATED_FILE_CENSUS` closes over generated **files** and these are not generated. Neither could ever have seen them — which is exactly the three-way split [§5.7.8](#578-the-read-ceiling-registry) argues the third leg of. `models/build.rs`'s own fail-on-`None` message already carries the lesson in one line: *"a hand-picked list of four missed `Hash` entirely."*

**Rejected candidates** (kept in the register so they are not re-proposed): **SS-X1** `cf35ab53` — exhaustive `PartialEq`/`Hash`; not stack safety, see [§5.8](#58-the-rejected-candidate).

⚠★ **Read `SS-D3` and `SS-D5` correctly.** Both eliminate a **call to** `<Par as Clone>::clone`; **neither converts the impl**. The campaign's strategy at those two sites is **call-site elimination**, argued in [§5.10.5a](#5105a--the-strategy-is-call-site-elimination-not-impl-conversion--and-it-should-be-argued-not-inferred). Two one-line task summaries read otherwise and are corrected in [§5.10.5](#5105-the-verdict-and-the-correction-to-the-tracker).

> ★★★ **SUPERSEDED, and the superseded wording is kept here verbatim so it cannot be "restored" as a bug fix.** This paragraph previously continued: *"and no commit in the history does. `<Par as Clone>::clone` remains in `TRIPWIRE_DEPTH` at **3,254 B/level** — the largest unconverted traversal in the system after `prost_de`."*
>
> **That is no longer true at HEAD.** `<Par as Clone>::clone` **was converted** by **stage F-4**, commit **`0eac9c3a`** (2026-07-29): `models/build.rs` strips the derive and `models/build/wire_schema.rs` generates the impl over `drive_with`. `clone` **moved from `TRIPWIRE_DEPTH` to `CONVERTED_DEPTH`**, and its `assert_slope_below("clone", ceiling(25_000, 5_000), 16, 128)` was **deleted rather than relaxed** — it left by being **CONVERTED**, never by having its ceiling raised (**DERIVED**, `f1r3node-rust-mettail@0eac9c3a`; **read from source** at `8bf298ba`, `rholang/tests/stack_depth_gate.rs`, the `"clone"` entry inside `CONVERTED_DEPTH` and the `` ⚠★ `clone` IS GONE FROM THIS LIST `` comment inside `TRIPWIRE_DEPTH`).
>
> **MEASURED (q)**, `0eac9c3a`, both profiles, ladder $`16 \rightarrow 128`$, subject and derived-oracle control in the *same binary*:
>
> | | `clone_oracle` (the DERIVE, re-emitted) | `clone` (the DRIVER) |
> |---|---:|---:|
> | debug | 276 $`\rightarrow`$ 2,040 KiB = **16,128 B/level** | 48 $`\rightarrow`$ 48 KiB = **0** |
> | release | 124 $`\rightarrow`$ 892 KiB = **7,021 B/level** | 12 $`\rightarrow`$ 12 KiB = **0** |
>
> ⚠ **And the 7,021-versus-3,254 gap is an ORACLE artefact, not a second measurement of the derive.** The oracle is a *semantic* reproduction of the derive's body — proven byte-identical on eight axes over 67 shapes — and **not a frame-layout one**: under `-O` a family of free functions inlines differently from one monolithic `<Par as Clone>::clone`. It reads **2.16$`\times`$ high** against the on-record 3,254. $`\Rightarrow`$ **Neither 3,254 nor 7,021 is now a live figure for this traversal**; the live figure is **0**. Anything that divided by 3,254 — a stack budget, a $`D_{\max}`$ extrapolation, a $`k \leq 12288/3254`$ constraint — was dividing an instrument floor ([§8.6.8](#868--the-instrument-floor-and-the-wrong-shape--two-corrections-that-change-the-method-not-just-a-number)) by a figure that has since gone to zero, and is void on both factors.
>
> **The residual that remains is `substitute_deep_binding` at 7,460 B/level**, not `clone`; the eight surviving `TRIPWIRE_DEPTH` members are enumerated in [§8.6.2](#862--43--f1r3nodes-hand-written-par-traversals-8-tripwire-members-remain).

---

## Abstract

A `Par` — the term representation of the Rholang interpreter — is a mutually recursive family of 37 protobuf message types whose every cycle passes through `Par` itself (**MEASURED (q)**, `7c74260d`: 58 nodes, 95 edges, 22 strongly connected components, exactly one cyclic). Until 2026-07-26, essentially every traversal of that family was written as recursive descent, so each consumed native stack in proportion to the *nesting depth of an attacker-chosen term*. Because a native-stack overflow in Rust is a `SIGSEGV` on the guard page and not a catchable panic, program-controlled nesting depth controlled node liveness. The worst reachable instance measured **8.8 kB of source text aborting a node** through the term *destructor* alone (**MEASURED (q)**, `291bc217`), and a second, on unauthenticated pre-consensus gRPC ingress, at **43,565 bytes** (**MEASURED (q)**, `3b265eb7`).

This report documents **twenty code fixes across five families**, the **eighteen instrument commits** that make their results admissible, and **one candidate rejected** from the list as not belonging to this problem at all. **Twenty-one traversals — fifteen on the depth axis and six on the width axis — are now measured flat**: identical minimum surviving stack at depth 4 and depth 4,096, and at width 4 and width 65,536, in both build profiles (**MEASURED (f)**, `/tmp/sd_gate_release.log`; **read from source** at `f1r3node-rust-mettail@8bf298ba`, `CONVERTED_DEPTH` $`=`$ 15 entries, `CONVERTED_WIDTH` $`=`$ 6).

> ★ **Superseded count, kept so it is not "restored".** This sentence previously read *"Nineteen traversals — thirteen on the depth axis and six on the width axis"*. Two further depth subjects were converted after it was written — **`clone`** and **`clone_send_chain`**, both by stage F-4 (`0eac9c3a`) — moving the depth axis from 13 to **15**. See the correction in [§0](#0-the-fix-register--the-scannable-index).

⚠★ **And this abstract does not describe a finished problem.** **Eight `TRIPWIRE_DEPTH` members remain**; three event-hash legs are **measured and not converted**; `try_eval` is converted in **one category of sixteen**; the prost read ceiling is **designed, not built**; and **#197 — a defect inside one of the very commits reported here as a success — is live and unrepaired, blocking `--all-targets` repo-wide**. The complete register is [§8.6](#86--the-open-residual-register--what-this-report-does-not-establish) and its verdict is [§8.6.10](#8610-the-verdict-of-this-section). **Cite this report for a mechanically-checked class change over the traversals named in [§0](#0-the-fix-register--the-scannable-index); do not cite it for the stack-safety of the `Par` family.**

Headline results, all **MEASURED**:

| what | before | after | factor |
|---|---:|---:|---:|
| substitution driver, `substitute_no_sort` (debug) | 195,728 B/level | **0** | class change |
| the normalizer, `normalize` (debug / release) | 43,542 / 7,261 B/level | **0 / 0** | class change |
| cold-store **decoder** (debug / release) | 28,362 / 12,894 B/level | **0 / 0** | class change |
| cold-store **encoder** (release, in-binary control) | ~224 B/level | **0** | class change |
| `inj_attempt` metering handshake (release) | 2,852 B/level, $`D_{\max}=729`$ | **0**, $`D_{\max} \geq 1{,}048{,}576`$ | $`\geq 1438\times`$ |
| gRPC ingress teardown (release) | 96.0 B/level, $`D_{\max}=21{,}781`$ | **0**, no ceiling $`< 262{,}144`$ | $`\geq 12\times`$ |
| metered wrapper `subst_and_charge` (release) | 2,852 B/level | **146** B/level | $`19.5\times`$ |
| end-to-end `plain_deploy` on a 2 MiB worker | 286 levels | **6,831** levels | $`23.9\times`$ |
| end-to-end `env_get_deploy` — **the control** | 283 levels | **274** levels (2026-07-29) | **$`\approx 1\times`$, as predicted** |
| cold-store encode, production-weighted wall clock | — | **faster**; magnitude **NOW UNKNOWN**, bracketed $`1.07\times`$–$`1.19\times`$ | ⚠ see §5.4.1 — the $`\pm 0.005`$ is **RETRACTED** |
| cold-store encode, allocations (reused-buffer form) | 20,022 blocks / 20k calls | **26** blocks / 20k calls | $`770\times`$ fewer |

The costs are reported with the same candour. The single-walk encoder performs **$`3.25\times`$ more heap writes** than the derive it replaces and **doubles peak heap** on the production shape, because it retains two thread-local arenas (**MEASURED (f)**, DHAT (dynamic heap analysis tool)). The prost network encoder's memoised rewrite trades $`O(1)`$ space for $`\Theta(n)`$ space to buy $`\Theta(d^2) \rightarrow \Theta(n)`$ work, and is **dormant** — not wired into any `src/` tree (**DERIVED**, `56fb1fd0`).

Three predictions were **falsified by measurement and are reported as results**: that `Env::get` was the deploy path's bound (§5.5.1), that the ingress slope was 84.3 B/level (§5.5.2), and — in the companion repository — that the Rholang parser was depth-independent (§5.6.3). Two recorded constants have **drifted at HEAD** and are corrected here (§5.5.4, §5.5.5).

The residual is named, not implied. **The protobuf network format is still recursive on both sides** — 302 B/level writing, 4,096 B/level reading — and its reader is capped at term depth **33 / 32 / 31** by a private `prost` constant while its writer has no cap at all. That is the **write/read asymmetry** (§6.4): a term that can be built, reduced and serialised, and cannot be read back. It has now surfaced four independent times.

---

## Table of contents

- [0. THE FIX REGISTER — the scannable index](#0-the-fix-register--the-scannable-index)
- [1. Introduction](#1-introduction)
- [2. Background](#2-background)
  - [2.1 The term family](#21-the-term-family)
  - [2.2 The two wire formats](#22-the-two-wire-formats)
  - [2.3 The observable: `B/level`](#23-the-observable-blevel)
  - [2.4 Glossary](#24-glossary--every-term-defined-before-first-use)
- [3. Related work](#3-related-work)
- [4. Methods](#4-methods)
- [5. Results](#5-results)
  - [5.1 Family A — the substitution, sorting, normalisation and evaluation cores](#51-family-a--the-substitution-sorting-normalisation-and-evaluation-cores)
  - [5.2 Family B — the expression evaluator and the `tokio` fire-and-forget driver](#52-family-b--the-expression-evaluator-and-the-tokio-fire-and-forget-driver)
  - [5.3 Family C — the codecs, and the malloc profile](#53-family-c--the-codecs-and-the-malloc-profile)
  - [5.4 Throughput and CPU profile of the codec conversion](#54-throughput-and-cpu-profile-of-the-codec-conversion)
  - [5.5 Family D — the deploy path](#55-family-d--the-deploy-path)
  - [5.6 Family F — fixes originating in `mettail-rust`](#56-family-f--fixes-originating-in-mettail-rust)
  - [5.7 Family E — the instrument](#57-family-e--the-instrument-and-what-it-caught-in-itself)
  - [5.8 The rejected candidate](#58-the-rejected-candidate)
  - [5.9 Measurements that could not be obtained](#59-measurements-that-could-not-be-obtained)
  - [5.10 ★★ The generated trait implementations, and the `Clone` question](#510--the-generated-trait-implementations-and-the-clone-question)
  - [5.11 ★★ SS-G1 — the Arc fix](#511--ss-g1--the-arc-fix-eliminating-a-traversal-instead-of-converting-it)
- [6. Discussion](#6-discussion)
- [7. Threats to validity](#7-threats-to-validity)
- [8. Residuals and future work](#8-residuals-and-future-work)
  - [8.6 ⚠★★★ THE OPEN RESIDUAL REGISTER — what this report does NOT establish](#86--the-open-residual-register--what-this-report-does-not-establish)
    - [8.6.1 #162/#189 — the eleven generated drivers converted, and the root cause](#861--162189--the-eleven-generated-drivers-converted-and-the-root-cause-that-unifies-154-with-162)
    - [8.6.1a ⛔ #197 — the defect inside #162's own commit, LIVE at HEAD](#861a--197--the-defect-inside-162s-own-commit-live-at-head)
    - [8.6.2 #43 — 8 tripwire members remain, and it is NOT superseded](#862--43--f1r3nodes-hand-written-par-traversals-8-tripwire-members-remain)
    - [8.6.3 #124 — the event-hash legs, and the $`\Theta(d^2)`$ in TIME](#863--124--the-event-hash-legs-measured-not-converted-and-quadratic-in-time)
    - [8.6.4 #189 residual — `try_eval` partially converted](#864--189-residual--try_eval-is-partially-converted-and-the-ratchet-that-says-so)
    - [8.6.5 #119/#120 — the prost read ceiling and the trap in removing it](#865--119120--the-prost-read-ceiling-and-the-trap-in-removing-it)
    - [8.6.6 #174 — 10,491 B/level that belongs to no measured driver](#866--174--10491-blevel-that-belongs-to-no-measured-driver)
    - [8.6.7 #157 — a transcribed ceiling a tripwire cannot see drift](#867--157--a-transcribed-ceiling-and-a-tripwire-that-cannot-see-it-drift)
    - [8.6.8 The instrument floor, and the wrong shape](#868--the-instrument-floor-and-the-wrong-shape--two-corrections-that-change-the-method-not-just-a-number)
    - [8.6.9 #121 — the gate that overflowed, and the frame that was not the encoder](#869--121--the-gate-built-to-demonstrate-a-fix-overflowed-and-the-frame-was-not-the-encoder)
    - [8.6.10 The verdict of this section](#8610-the-verdict-of-this-section)
- [9. Conclusions](#9-conclusions)
- [References](#references)
- [Appendix A — reproduction commands](#appendix-a--reproduction-commands)
- [Appendix B — raw data locations](#appendix-b--raw-data-locations)
- [Appendix C — the complete fix inventory](#appendix-c--the-complete-fix-inventory)
- [Appendix D — corrections to the commissioning brief](#appendix-d--corrections-to-the-commissioning-brief)
- [Appendix E — documentation-guideline conformance](#appendix-e--documentation-guideline-conformance)
- [Appendix F — the per-fix template](#appendix-f--the-per-fix-template-fill-this-in-do-not-invent-a-shape)
- [Appendix G — keeping this document current](#appendix-g--keeping-this-document-current)

---

## 1. Introduction

### 1.1 The problem is liveness, not performance

Rholang is a concurrent language in the reflective higher-order calculus tradition [[Meredith & Radestock 2005](#ref-meredith2005)], itself descended from the $`\pi`$-calculus [[Milner, Parrow & Walker 1992](#ref-milner1992)]. Its terms nest without bound: `[[[[0]]]]` is a legal list, and so is the same expression with a million brackets. A node accepts such terms from the network, normalises them, evaluates them, serialises them, and stores them.

Every one of those verbs was, until this campaign, implemented as **recursive descent** — a function that calls itself (or a mutually recursive partner) once per nesting level. In Rust, that consumes the **native call stack**: a fixed-size region, allocated at thread creation, terminated by a **guard page**. Running off the end raises `SIGSEGV`, which the Rust runtime converts to

```text
thread '<name>' has overflowed its stack
fatal runtime error: stack overflow, aborting
```

followed by `abort()`. Shell status **134** = `128 + SIGABRT`.

★ **This is not a panic.** `catch_unwind` cannot intercept it. An enclosing `Err(e) => …` arm cannot observe it. The whole **process** dies, not one worker task. So the question *"how deep may an incoming term be?"* is not a performance question; it is the question *"can an unauthenticated party stop this node?"*, and in a consensus system that is a liveness and availability property.

**MEASURED (q)** — `f0894109`: a **30-character** Rholang program with no guest language and no $`\lambda`$-calculus, `@"OUT"!([[[[[[[[[[0]]]]]]]]]])` at nesting depth 10, aborted the reducer on the 2 MiB stack a `tokio` worker gets.

### 1.2 What makes a fix a fix

A constant-factor improvement is not a fix. Reducing a traversal from 195,728 B/level to 27,179 B/level multiplies the maximum admissible depth by seven and leaves the class — and therefore the vulnerability — exactly where it was. The campaign therefore adopted a **binary criterion**, enforced mechanically:

> A traversal is **converted** when its minimum surviving native stack is *identical* at nesting depth 4 and at nesting depth 4,096, in **both** build profiles.

and a matching admission rule, stated at the definition of the register itself (**DERIVED**, `rholang/tests/stack_depth_gate.rs:725-728`):

> *"Membership is the deliverable; it only ever grows. A traversal enters only by being converted, never by having a ceiling raised."*

Everything not yet converted sits in a **tripwire** list under a per-profile ceiling that certifies only *"not worse"*. §5 reports both lists in full, because a document that lists conversions without listing the residual reads as far more finished than the code is.

### 1.3 Contributions

1. A **complete inventory** of the stack-safety work in this worktree — 20 code fixes, 18 instrument commits, 1 rejected candidate — each with its defect, its repair architecture, and the alternatives that architecture was chosen over (§5, Appendix C).
2. **Fresh measurements** of the whole gate, the S0 baseline, and the end-to-end deploy ceiling on the report tree (§5, Appendix A), which reproduce the recorded figures **exactly** in ten of twelve cases and disagree in two, both reported (§5.5.4, §5.5.5).
3. The **first heap profile** of the codec conversion — massif and DHAT — answering the question *where did the allocations move?* quantitatively (§5.3.4).
4. A first-class treatment of the **`tokio` fire-and-forget** family, including the establishment — rather than the assumption — of its relationship to stack safety (§5.2.2).
5. An account of the **write/read asymmetry** that the campaign kept re-discovering, and of why only one of the two wire formats was repaired (§6.4).

---

## 2. Background

### 2.1 The term family

`Par` is the Rholang process term. It is defined in `models/src/main/protobuf/RhoTypes.proto` and compiled by `prost` into `models`.

**DERIVED** (`7c74260d`, cross-checked at build time by `models/build.rs`, and re-verified for this report against `target/release/build/models-949e215e3c72bc99/out/rhoapi.rs`): the schema has **57** messages carrying `::prost::Message` and **5** carrying `::prost::Oneof`. The child relation over them has **58 nodes and 95 edges**, decomposing into **22 strongly connected components** by Tarjan's algorithm [[Tarjan 1972](#ref-tarjan1972)], of which **exactly one is cyclic**, with **37 members**.

★ **$`\{\mathtt{Par}\}`$ is a feedback vertex set of size one.** Removing `Par` and recomputing leaves nothing cyclic (**MEASURED (q)**, `7c74260d`, test `par_is_a_feedback_vertex_set_of_size_one`). This was *computed*, not assumed: the incoming analysis asserted "the cycle is $`\mathtt{Par} \rightarrow \mathtt{Expr} \rightarrow \mathtt{ExprInstance} \rightarrow \mathtt{Par}`$, so about three frames", and a 37-member component may contain sub-cycles avoiding any given vertex. It does not — but that is a theorem about this schema, not a general fact.

Two consequences follow and are used throughout:

* the longest chain of derived traversal that can run **before** reaching a `Par` is **3 edges**, attained uniquely by $`\mathtt{Expr} \rightarrow \mathtt{EMap} \rightarrow \mathtt{KeyValuePair} \rightarrow \mathtt{Par}`$ (**MEASURED (q)**, `7c74260d`) — so the per-level cost of any derived traversal is bounded by the schema, not by the input; and
* one iterative teardown of `Par` would bound the whole schema — which is why `impl Drop for Par` keeps being proposed, and why §8.2 records precisely what it would break.

### 2.2 The two wire formats

A `Par` is serialised **two different ways**, by two different codecs, for two different purposes. This is the single most load-bearing structural fact in the report.

![two wire formats](figures/two-wire-formats.svg)

**Figure 1** — *`figures/two-wire-formats.puml`*. One term, two wire formats. Only the cold-store pair was trampolined on both sides.

| | **cold store** | **network / consensus** |
|---|---|---|
| codec | `bincode` 1.3.3 over `serde` | protobuf via `prost` 0.13.5 |
| direction of travel | node-local, into LMDB (lightning memory-mapped database) | between nodes, and into blocks |
| writer | `wire_encode::encode` — **CONVERTED** | `<Par as Message>::encode_raw` — recursive |
| reader | `par_codec::cold_decode` — **CONVERTED** | `<Par as Message>::merge_field` — recursive |
| writer depth limit | none | none |
| reader depth limit | none | **`RECURSION_LIMIT = 100`** |

**DERIVED**: `prost`'s `RECURSION_LIMIT` is `const RECURSION_LIMIT: u32 = 100;` at `prost-0.13.5/src/lib.rs:30`. It is **private and unconfigurable**. Because the `Par` cycle costs three message levels per Rholang nesting level, and an envelope of wrapper depth $`W`$ consumes $`W`$ of the budget, the admissible term depth is

```math
D_{\max}(W) \;=\; \left\lfloor \frac{100 - 1 - W}{3} \right\rfloor
```

which yields **three distinct ceilings — 33, 32 and 31** across the nine real envelopes the gate drives (**MEASURED (f)**, `/tmp/sd_gate_release.log`: `read ceiling: 9 envelopes, 3 distinct ceilings {31, 32, 33} (bare 33)`). The gate's test *fails if it ever yields fewer than three*, because it exists to refute the sentence "the ceiling is one number".

**DERIVED** (`9a5521a2`): `bincode` has no recursion limit — **1.3.3, 2.0.1 and `bincode-next` 3.1.1 alike** offer only a *byte* limit defaulting to `NoLimit`. That is why the cold-store reader was a $`\Theta(d)`$ traversal with **no ceiling other than the stack**, and why its failure mode was uniquely bad: `rspace_importer` writes cold-store bytes to LMDB *without deep-decoding them*, so a too-deep datum enters storage through a path that structurally cannot observe its depth, and then aborts the node **on every read-back, on every restart, on every peer that synced the same state**. Every other member of the family is a transient worker fault; this one is **permanent and replicated**.

### 2.3 The observable: `B/level`

Let $`S(d)`$ be the smallest thread stack, in bytes, on which a given traversal survives an input of nesting depth $`d`$. Empirically every traversal in this family is affine in $`d`$:

```math
S(d) \;=\; c \;+\; B \cdot d
```

where $`c`$ is a fixed **intercept** (the traversal's own set-up: parser tables, driver frame, the deepest non-recursive prelude) and $`B`$ is the **slope**, in *bytes of native stack per nesting level* — written **B/level** throughout. The estimator is a two-point difference over a ladder $`[d_{\mathrm{lo}}, d_{\mathrm{hi}}]`$:

```math
\hat{B} \;=\; \frac{S(d_{\mathrm{hi}}) - S(d_{\mathrm{lo}})}{d_{\mathrm{hi}} - d_{\mathrm{lo}}}
```

with each $`S`$ obtained by **bisecting the thread's `stack_size`** to a 4,096 B resolution, in a **forked child process** (an overflow aborts and cannot be caught, so an in-process probe would take every already-passing assertion down with it).

**Why this observable and not wall-clock or peak RSS (resident set size).** The quantity that decides liveness is $`D_{\max} = \lfloor (S_{\text{avail}} - c)/B \rfloor`$, and only $`B`$ distinguishes the complexity classes. A conversion must drive **$`B`$ to zero**; it is explicitly *not* required to reduce $`c`$, and in several cases it raises $`c`$ (§5.1.2). Asserting a *shape* — "the two ends are equal" — rather than a byte count also makes the criterion **profile-independent by construction**, which matters because the debug-to-release ratio in this family ranges from $`2.2\times`$ to $`12.1\times`$ and is *per traversal*, not uniform (**MEASURED (q)**, `f0894109`).

⚠ **Two symmetric hazards of this estimator**, both encountered and both recorded:

1. **A large intercept masks a small slope.** If both ladder ends sit inside $`c`$, the measured growth is zero and the traversal reads as converted when it is not. This produced a *retracted claim* about the Rholang parser (§5.6.3).
2. **A short ladder quantises to noise.** Release `par_drop` at ~144 B/level bisects to 12,288 B at *both* ends of a 16 $`\rightarrow`$ 128 ladder — inside the bisector's initial 16 KiB probe window — and reports 36 B/level, which is quantisation, not measurement. The S0 harness therefore **doubles its deep end until the growth exceeds $`8 \times`$ the resolution**, and fails loudly at the cap with a message that distinguishes *"the traversal is depth-independent"* from *"the ladder is too short"*, because those need opposite responses (**MEASURED (q)**, `44535d75`).

![recursive vs trampolined](figures/recursive-vs-trampolined.svg)

**Figure 2** — *`figures/recursive-vs-trampolined.puml`*. The central picture: why recursive descent grows the native stack and an explicit-worklist driver does not.

### 2.4 Glossary — every term defined before first use

| term | definition |
|---|---|
| **stack-safe** | a traversal whose native-stack consumption is $`O(1)`$ in the nesting depth of its input, i.e. $`B = 0`$. It may still be $`\Theta(d)`$ in *heap*; that is the point of the transformation, not a failure of it. |
| **B/level** | bytes of native stack consumed per additional nesting level; the slope $`B`$ of §2.3. |
| **guard page** | an unmapped page placed immediately past the end of a thread's stack by the operating system. Touching it raises `SIGSEGV`; Rust's handler prints `fatal runtime error: stack overflow` and calls `abort()`. Not a panic, not unwindable. |
| **trampoline** | a control structure in which a function, instead of calling its continuation, *returns* a description of the remaining work to a driver loop that performs it. The loop's single frame replaces the recursion's $`d`$ frames [[Ganz, Friedman & Wand 1999](#ref-ganz1999)]. |
| **explicit continuation** | the "rest of the computation", represented as a **first-class data value** on the heap rather than implicitly as the return address and live locals of a stack frame. |
| **defunctionalisation** | Reynolds's transformation replacing higher-order continuation *functions* with a first-order *data type* plus an `apply` function [[Reynolds 1972](#ref-reynolds1972); [Danvy & Nielsen 2001](#ref-danvy2001)]. The work-item enums in §5 are defunctionalised continuations. |
| **CEK** | *machine* — an abstract machine for the $`\lambda`$-calculus whose state is a triple of $`C`$ ontrol, $`E`$ nvironment and $`K`$ ontinuation, with the continuation an explicit stack of frames [[Felleisen & Friedman 1987](#ref-felleisen1987)]; the ancestor, via [[Ager et al. 2003](#ref-ager2003)], of every driver in §5. |
| **worklist / work stack** | the concrete `Vec` holding pending work items. LIFO gives depth-first order, matching what recursive descent did. |
| **value stack** | a companion `Vec` holding *completed* children, parked until their parent's combining step is reached. Only *bottom-up* (post-order) drivers need one. |
| **fire-and-forget** | of a spawned asynchronous task: the spawner does **not** hold or await its `JoinHandle`. Completion and errors are conveyed by some other channel — here an atomic counter and an error sink (§5.2.2). |
| **SCC** | strongly connected component of the call graph or of the type-child graph; the unit at which a conversion must be scoped, because converting a proper subset leaves the class intact. |
| **converted / tripwire** | the two registers in `rholang/tests/stack_depth_gate.rs`. *Converted* = measured $`B = 0`$ at both ladder ends in both profiles. *Tripwire* = measured $`B > 0`$, held under a ceiling. |
| **anti-vacuity** | a check that the *checker* can fail: a control the assertion must reject, run in-suite, so that a green result cannot be produced by a probe that measures nothing. |
| **$`D_{\max}`$** | the greatest nesting depth a traversal survives on a given stack: $`\lfloor (S_{\text{avail}} - c)/B \rfloor`$. |
| **slope** | the *estimated* $`B`$ of a traversal, obtained by differencing two bisected endpoint measurements rather than by reading a frame size: $`\widehat{B} = \dfrac{S(d_{hi}) - S(d_{lo})}{d_{hi} - d_{lo}}`$, where $`S(d)`$ is the minimum surviving stack at depth $`d`$. **A slope is a difference, so any constant common to both endpoints cancels** — which is exactly why a slope survives the *instrument floor* below while the endpoint values do not. Quantised to `RESOLUTION` (4,096 B). |
| **flat ladder** | a *ladder* is the ordered set of depths a subject is measured at (this report uses $`4 \rightarrow 4{,}096`$ on the depth axis and $`4 \rightarrow 65{,}536`$ on the width axis). The ladder is **flat** when $`S(d_{lo}) = S(d_{hi})`$ to bisection resolution, i.e. slope $`= 0`$ — the operational definition of *converted*. ⚠ Flatness is a claim about **two** measured rungs, not about the shape of the code between them. |
| **instrument floor** | the smallest value a measuring procedure can *emit*, independent of the subject. Here it is **12,288 B**, forced by `min_stack_for`'s `PROBE_START = 16 KiB` and `RESOLUTION = 4096` (derived in [§8.6.8](#868--the-instrument-floor-and-the-wrong-shape--two-corrections-that-change-the-method-not-just-a-number)). A reading *at* the floor carries **no subject information** and must never be divided by. Distinct from a *measurement* of 12,288 B, which is why `MinStack::BelowResolution` exists: the floor is now **unspellable as a number**. |
| **ratchet** | a pinned integer constant asserting the *count* of some known-bad population, so the population cannot grow silently. `UNMEASURED_TRAVERSALS = 7` is a ratchet: adding an unmeasured traversal fails the gate, and *removing* one requires editing the constant down, which makes the improvement explicit. A ratchet bounds a set whose **members** are not all enumerable; it is the honest instrument when enumeration is the failure mode ([§7.4](#74-enumeration-completeness)). |
| **`SIGSEGV` vs `SIGABRT`** | the two ways a stack-exhausted Rust process dies, and the distinction is the whole reason this report exists. A native-stack overflow touches the **guard page** and raises **`SIGSEGV`** (signal 11) — *not* a Rust panic, *not* catchable by `catch_unwind`, no unwinding, no destructors. Rust's runtime handler recognises the fault address, prints `fatal runtime error: stack overflow`, and calls `abort()`, which raises **`SIGABRT`** (signal 6, shell status **134**). $`\Rightarrow`$ The *observable* is usually 134, the *cause* is always 11, and **neither is a failed deploy** — both take the whole node process. A heap exhaustion, by contrast, is an `Err` a caller can handle. |

---

## 3. Related work

The transformation applied throughout §5 is not novel and was not treated as such; its value here is that it was applied at the **SCC** granularity, with **differential oracles**, to a **consensus-critical** codebase.

**Explicit continuations and abstract machines.** Landin's SECD (stack, environment, control, dump) machine [[Landin 1964](#ref-landin1964)] introduced the idea of making the control state of an evaluator an explicit data structure. Felleisen and Friedman's CEK (control, environment, kontinuation) machine [[Felleisen & Friedman 1987](#ref-felleisen1987)] gave the modern three-component form in which the continuation is a *stack of frames*. Ager, Biernacki, Danvy and Midtgaard [[Ager et al. 2003](#ref-ager2003)] established the *functional correspondence*: a compositional evaluator, CPS-transformed and then defunctionalised, **is** an abstract machine. That correspondence is exactly the recipe used here — every driver in §5 is a defunctionalised continuation over a term walk — and it is also why each conversion could be paired with a *retained recursive oracle* and checked differentially: the two are meant to be equal by construction, so any divergence is a bug in the mechanisation rather than a design question.

**Defunctionalisation.** Reynolds [[Reynolds 1972](#ref-reynolds1972)] introduced the transformation; Danvy and Nielsen [[Danvy & Nielsen 2001](#ref-danvy2001)] gave the systematic account. The `Op`, `Work`, `Kont` and `EvWork` enumerations of §5 are first-order representations of the continuations that recursive descent left implicit in return addresses.

**Trampolining.** Ganz, Friedman and Wand [[Ganz et al. 1999](#ref-ganz1999)] define *trampolined style*, in which a computation returns a thunk to a driver loop rather than calling its continuation. The interpreter's expression evaluator (§5.2.1) is trampolined in exactly this sense; the codecs (§5.3) go further and are *fully defunctionalised*, carrying no closures at all.

**Iterative traversal of recursive structures.** The problem is old in garbage collection, where a collector may not itself allocate stack. Schorr and Waite [[Schorr & Waite 1967](#ref-schorr1967)] traverse an arbitrary list structure in constant auxiliary space by **pointer reversal**, temporarily overwriting the very pointers being followed. Cheney [[Cheney 1970](#ref-cheney1970)] achieves constant auxiliary space differently, by using the *to-space itself* as the queue.

★ **Both were considered and neither was adopted**, and the reasons are worth recording because they explain the shape actually chosen:

* **Pointer reversal** requires mutating the structure during traversal. Several traversals here run on `&`-borrowed terms (the encoder walks `&dyn WireNode`), several run on terms shared behind `Arc`, and the sorter's output is *signed* — a traversal that transiently mutates a term another thread may observe is not admissible in this setting. Constant auxiliary space was also never the requirement: $`\Theta(d)`$ **heap** is entirely acceptable, because the heap can refuse.
* **Cheney's trick** presumes the output region is being built contiguously and can double as the queue. The encoder's output *is* contiguous — and, notably, the encoder needs no value stack at all for that reason (§5.3.2) — but the decoder must reassemble a pointer-rich `Par` whose children are not adjacent, so there is no to-space to borrow.

**Statistics.** ⚠★★ **REVISED 2026-07-30 — this sentence described the instrument, and the instrument was wrong.** It read: *"Throughput comparisons use Welch's unequal-variances $`t`$-test [[Welch 1947](#ref-welch1947)], as implemented in the repository's own bench harness."* They did, and that was the defect: the harness measured its arms in **disjoint time windows** (a **blocked** design) while its header claimed per-repetition interleaving, and Welch's $`t`$ divides by the *within-arm* standard error, which on blocked arms omits the dominant error term. Throughput comparisons now use the **paired** $`t`$ on the per-repetition difference [[Student 1908](#ref-student1908)] plus the median-of-repetition ratio as the load-robust point estimate, in the shared `models/benches/paired.rs`. ★ Every wall-clock magnitude taken under the old instrument is retracted in §5.4.1; every **stack-depth** figure in this report is unaffected, because none of them is a timing.

**Instruments.** Heap measurements use Valgrind's massif and DHAT tools [[Nethercote & Seward 2007](#ref-nethercote2007)].

**Presentation.** Algorithms are given in Knuth's literate style [[Knuth 1984](#ref-knuth1984)]: a captioned block, then prose that walks its steps and says why each exists.

---

## 4. Methods

### 4.1 Hardware

**DERIVED** — from `/home/dylon/.claude/hardware-specifications.md`, cross-checked against `lscpu` on the measurement host.

| | |
|---|---|
| CPU | AMD Ryzen Threadripper PRO 5975WX, Zen 3 (Chagall), family 25 model 8 stepping 2 |
| cores / threads | 32 physical / 64 logical, 4 CCDs $`\times`$ 8 cores |
| clocks | base $`\approx`$ 3.6 GHz, max boost **4,561.833 MHz**, min 412.214 MHz |
| caches | L1d 32 KiB/core, L1i 32 KiB/core, L2 512 KiB/core, L3 32 MiB/CCD (128 MiB total) |
| memory | 128 GiB, 8 $`\times`$ 16 GiB DDR4-2933 ECC (error-correcting code) RDIMM (registered dual in-line memory module), 8 channels, 1 NUMA (non-uniform memory access) node |
| storage | NVMe, `/home` on `/dev/nvme0n1p4` |
| frequency driver | `amd-pstate-epp` (active mode) |

### 4.2 Machine state at measurement time

**MEASURED (f)**, recorded at each measurement cell.

* **Governor**: `performance` on every core (`/sys/devices/system/cpu/cpu*/cpufreq/scaling_governor`).
* **Swappiness**: `vm.swappiness = 0`.
* ⚠ **Frequency**: the measurement host was **shared with concurrent agent workloads throughout**. At the timing cell, core 8 (the pinned core) read `scaling_cur_freq = 3,520,772 kHz` against `scaling_max_freq = 4,561,833 kHz` — i.e. **77.2 % of maximum boost**. The bench harness printed cpu0 at `3,433,018 kHz` in its own environment block. **The cores were therefore *not* at maximum frequency**, and this is recorded as a fact rather than asserted away. It is a threat to the *absolute* timings (§7.2) and not to the *relative* ones, because the harness interleaves the two arms within each repetition.
* ⚠ **Load**: 1-minute load average was **35.70** when the session began, **13.22 / 13.37 / 14.14** at the starts of timing runs 1 / 2 / 3. This is a 32-core machine, so a load of ~13 is roughly 40 % subscription — not an idle machine.
* **Settle**: a settle interval was started at 14:22:36 and the first timing cell began at 14:27:38 — **302 s**, satisfying the $`\geq 300`$ s discipline, though the machine was never *quiescent* in the interval, only quieter.

### 4.3 Toolchain and build configuration

**DERIVED**.

| | |
|---|---|
| rustc | `1.95.0-nightly (6efa357bf 2026-02-08)` |
| cargo | `1.95.0-nightly (fe2f314ae 2026-01-30)` |
| profile for all measurements in §5 | **release** (`[profile.release]`, `opt-level = 3`), unless a row says *debug* |
| `[profile.dev]` | `debug = true` only — ⚠ **no `codegen-backend = "cranelift"` is configured in this workspace**; verified by grep over `Cargo.toml`, every member `Cargo.toml`, and `.cargo/config.toml`. Every debug figure quoted here is therefore an ordinary `-O0` figure from LLVM (the compiler back end rustc emits through). |
| `rustflags` | `-C target-cpu=native` (from `.cargo/config.toml`) |
| `RUST_MIN_STACK` | `8388608` from `.cargo/config.toml`. ⚠ It affects **spawned threads only**, never a main thread, and every stack probe overrides it by passing an explicit `stack_size` to `std::thread::Builder`, so neither it nor `ulimit -s` can mask a regression. |
| feature flags | workspace defaults; no `--features` passed |

⚠ **A pre-existing `-D warnings` break, reported and not repaired.** CI runs `cargo test --release -p rholang`; at HEAD that fails to compile because `NormKont::arity` / `filled` are dead under `-D warnings`. All builds for this report were therefore made **without** `-D warnings`. The break is in a file owned by concurrent in-flight work and was deliberately not touched. A parallel instance of the same hazard is recorded in `56fb1fd0` for `models`.

### 4.4 Instruments, and which produced which number

| instrument | what it produced | invocation |
|---|---|---|
| `rholang/tests/stack_depth_gate.rs` | every **B/level** figure and every *converted* / *tripwire* verdict, by forked-child `stack_size` bisection at 4,096 B resolution | §A.1 |
| `four_quadrant_s0_baseline` (an `#[ignore]`d test in the same file) | the eight-traversal baseline table of §5.3.1 | §A.2 |
| `rholang/tests/deploy_depth_ceiling.rs` | the **end-to-end** deploy ceilings, from source text through the real runtime on an explicit 2 MiB `tokio` worker | §A.3 |
| `valgrind --tool=massif --time-unit=B` | **peak heap** and the heap-over-time series of §5.3.4 | §A.4 |
| `valgrind --tool=dhat` | **allocation counts**, total bytes, and heap read/write traffic | §A.5 |
| `models/benches/wire_encode_bench.rs` | wall-clock throughput, 60 reps after 10 warm-up. ⚠ **This cell read *"with Welch's $`t`$-test, interleaved A/B"* and BOTH halves were wrong**: the arms were **blocked**, not interleaved, and an unpaired $`t`$ is invalid on blocked arms. Now the shared paired harness (`models/benches/paired.rs`), paired $`t`$ + median-of-repetition ratio, order rotated by repetition parity | §A.6, §5.4.1 |
| `perf record -e cpu-clock --call-graph dwarf` | the CPU profile of §5.4.2 | §A.7 |

⚠ **`perf record --call-graph lbr` could not be used.** The observed failure was:

```text
Failure to open event 'cpu/cycles/Pu' on PMU 'cpu'
The sys_perf_event_open() syscall failed for event (cpu/cycles/Pu): Invalid argument
```

with `kernel.perf_event_paranoid = 2`. The profile was taken with the **software** `cpu-clock` event and DWARF unwinding instead. This is a deviation from the standing measurement discipline and is recorded as such; the substitution costs sampling fidelity (software timer rather than cycle counter) but not symbol attribution, which is what §5.4.2 uses.

#### ⚠⚠ CORRECTED 2026-07-29 — the diagnosis above was wrong, twice

This section said *"the hardware PMU refused every cycles event on this host."* PMU (Performance Monitoring Unit) — the CPU's hardware counter block. It
does not, and the correction matters because it decides which instruments the next
measurement campaign may rely on. Two errors:

**1. Plain `cycles` always worked.** What the trace above shows failing is
`cpu/cycles/Pu` — the `u` is the user-space modifier and **the `P` is the *precise*
modifier**. The event that failed is `cycles:P`, not `cycles`. Measured on this
host, same binary, same session:

| event | result |
|---|---|
| `perf stat -e cycles` | ✓ counts (78,056,897 on the probe) |
| `perf stat -e cycles:P` | ✓ counts **now** (89,395,859) — see (2) |
| `perf stat -e instructions` | ✓ counts |
| `perf stat -e ls_dispatch.store_dispatch` | ✓ counts — the AMD store-µop counter |
| `perf stat -e ls_dispatch.ld_dispatch` | ✓ counts |
| `perf record --call-graph dwarf -e cycles:P` | ✓ records (690 samples) |
| `perf stat -e mem-stores` | ✗ *"Unable to find event on a PMU"* |
| `perf record ... -e ibs_op/swfilt=1/` per-thread | ✗ *"Invalid event … enable system wide with `-a`"* |
| `perf record --call-graph lbr` | ✗ still fails on this part |

**2. The precise modifier is an ISA (Instruction Set Architecture) fact, not a permissions fact.** `:P` is
implemented on Intel by PEBS (Precise Event-Based Sampling). This host is an
**AMD Ryzen Threadripper PRO 5975WX — Zen 3, family 0x19 model 0x8 — and PEBS does
not exist on that ISA.** That is also why `mem-stores` cannot be found at all: it is
an Intel PEBS event, and no amount of privilege conjures it. AMD's counterpart is
IBS (Instruction-Based Sampling), which `perf` exposes as the `ibs_op` PMU —
⚠ and which **refuses per-thread mode**, wanting `-a` (system-wide, i.e. root). A
measurement plan that names *"a PEBS-precise `mem-stores` profile"* as its decisive
experiment is therefore unrunnable here **by construction**, and one such plan was
written before this was checked.

`kernel.perf_event_paranoid` was subsequently set to **0** on this host, after
which `cycles:P` and `ls_dispatch.*` count and `--call-graph dwarf` records. So the
paranoid setting was *a* blocker for some events and never the blocker for `:P`
sampling fidelity on AMD.

★ **What to use instead, and it is better than what was asked for.**
`valgrind --tool=cachegrind --cache-sim=yes` is **deterministic** — no sampling, no
skid, no run-to-run variation — and its `Dw` column counts exactly the write
references a byte-movement hypothesis is denominated in. On 2026-07-29 it resolved
the clone-conversion question that wall-clock could not: `Ir` +17.40%, `Dr` +26.04%,
`Dw` +25.71% per node with **cache misses at parity**, and `cg_annotate` attributed
+93.6 write references per node to `drive_with` itself against a predicted
`3 × 248 / 8 = 93`. Hardware then corroborated it — `instructions` gave **1.1742×**
where cachegrind gave **1.1740×**. Two instruments, four significant figures, on a
host whose *wall-clock* could not distinguish 1.07× from 0.95× on the same binary
minutes apart.

$`\Rightarrow`$ ★ For any future question of this shape, **make the deterministic instrument
primary and wall-clock corroboration only** — and characterise what actually opens
before designing a measurement around an event name. See
`models/build/wire_schema.rs`'s clone-throughput section for the full accounting
and `models/benches/term_ops_bench.rs`'s `TERM_OPS_ARM` mode for the harness.

### 4.5 Procedure

* **Pinning.** Every measurement cell ran under `taskset`: the gate on cores 16–23, the deploy bisection on 24–27, massif and DHAT on cores 4–8 and 10–14 (one arm per core, run in parallel — heap metrics are deterministic under Valgrind's serialised execution and unaffected by co-residency), the timing bench on **core 8 alone**, `perf` on core 12.
* **Resource limits.** Every build and every heavy test ran under `systemd-run --user --scope -p MemoryMax=28G`.
* **`n` and warm-up.** The timing bench performs `REPS = 60` measured repetitions after `WARMUP = 10` discarded ones, per arm, per cell; the whole bench was then run **3 times** end-to-end, so the between-run figures in §5.4.1 are $`n = 3`$ over means each of which is itself $`n = 60`$.
* **Teeing.** Every command's output was written to a file and the file analysed; no benchmark was re-run to see a different part of its output. Locations in Appendix B.
* **Overlap rule.** A difference is reported as a result only if the arms' $`[\text{mean} - \text{sd},\ \text{mean} + \text{sd}]`$ intervals do **not** overlap. ⚠★ **The clause *"in addition to the harness's own Welch test at $`\alpha = 0.01`$"* is RETRACTED as a sufficiency criterion**, and so is the overlap rule itself for *blocked* arms: both are computed from **within-arm** scatter, so on a blocked design both certify a quiet window rather than a real difference. Non-overlap plus $`\alpha = 0.01`$ is exactly what §5.4.1's three retracted runs reported. $`\Rightarrow`$ For wall-clock rows the criterion is now the **paired** $`t`$ [[Student 1908](#ref-student1908)] with the median-of-repetition ratio, and where the effect is smaller than the instrument's measured run-to-run spread the row says **NOW UNKNOWN with a bracket** rather than *no measured difference* — an absence and an unknown are different dispositions. Non-timing rows (B/level, $`D_{\max}`$, DHAT block counts) are deterministic and keep the original rule.

### 4.6 Anti-vacuity discipline

Every measurement in §5 comes from a harness that has been **shown to fail**. This is not decoration; four false zeros in this campaign came from probes that measured nothing (a decode probe with a wrong proto field number, so `prost` skipped the payload as unknown; a generator whose collection sizes were `0..1`, meaning *exactly zero elements, always*; a scan rule that discarded 97 % of the largest interpreter file; a `zsh` `set --` that does not word-split). The gate accordingly carries **synthetic controls that its own checkers must reject** — `synthetic_recurse` at **111 B/level** release and `synthetic_drop` at **31 B/level** release — alongside their iterative twins at **0**, and the tripwire refuses to finish unless the set of subjects it actually drove is *exactly* the declared `TRIPWIRE_DEPTH` (**MEASURED (f)**, `/tmp/sd_gate_release.log`).

---

## 5. Results

**Twenty-one traversals are converted** (15 depth $`+`$ 6 width, **read from source** at `f1r3node-rust-mettail@8bf298ba`; the figure was *nineteen* before stage F-4 added `clone` and `clone_send_chain`). §5.1 – §5.6 give each family its defect, architecture, transformation and numbers; §5.7 covers the instrument; §5.8 the rejected candidate; §5.9 what could not be measured; **[§8.6](#86--the-open-residual-register--what-this-report-does-not-establish) states what remains unconverted, and it is not a short list.**

![depth vs stack ladder](figures/depth-vs-stack-ladder.svg)

**Figure 3** — *`figures/depth-vs-stack-ladder.puml`*. Every gate subject, converted and tripwired, with the freshly measured release figures of 2026-07-29.

**MEASURED (f)** — the complete register, `/tmp/sd_gate_release.log`, release, this tree, 2026-07-29:

**Converted, depth axis (13).** `substitute_no_sort` 28 KiB $`\rightarrow`$ 28 KiB · `substitute_binders` 28 $`\rightarrow`$ 28 · `substitute` 32 $`\rightarrow`$ 32 · `sort` 12 $`\rightarrow`$ 12 · `score_cmp` 12 $`\rightarrow`$ 12 · `tree_drop` 12 $`\rightarrow`$ 12 · `tree_clone` 12 $`\rightarrow`$ 12 · `eval_with_nots` 12 $`\rightarrow`$ 12 · `bincode_de` 12 $`\rightarrow`$ 12 · `bincode_ser` 12 $`\rightarrow`$ 12 · `pretty` 12 $`\rightarrow`$ 12 · `normalize` 32 $`\rightarrow`$ 32 · `inj_attempt_clone` 32 $`\rightarrow`$ 32 — every one identical at depth 4 and depth 4,096.

**Converted, width axis (6).** `substitute_wide` 32 $`\rightarrow`$ 32 · `sort_wide` 12 $`\rightarrow`$ 12 · `score_cmp_wide` 12 $`\rightarrow`$ 12 · `free_check` 12 $`\rightarrow`$ 12 · `pretty_wide` 12 $`\rightarrow`$ 12 · `normalize_wide` 32 $`\rightarrow`$ 32 — identical at width 4 and width **65,536**.

**Tripwire, depth axis (9), release B/level.** `sort_nested_map` **7,509** (ceiling 10,394) · `substitute_deep_binding` **7,460** (12,000) · `sort_nested_set` **4,778** (7,680) · `clone_nested_set` **4,096** (9,000) · `clone` **3,254** (5,000) · `encode` **302** (1,500) · `subst_and_charge` **146** (700) · `par_drop` **144** (800) · `normalize_drop` **144** (800).

**Tripwire, width axis: empty** — and that is an *executed* claim, not an absence.

---

### 5.1 Family A — the substitution, sorting, normalisation and evaluation cores

#### 5.1.1 The defect

**DERIVED** — `rholang/src/rust/interpreter/substitute.rs`. `Substitute::substitute` and its partners formed a mutually recursive strongly connected component that descended one call per term level. Enumeration was **derived, not recalled**: Tarjan over `RhoTypes.proto` gave the 37-member type family; a per-`(file, name)` call-graph search over **4,503 functions**, with an explicit cross-file trait-dispatch pass, yielded **153 candidates in 53 files**, each of which was read, dispositioned and — where genuine — measured in both profiles (**MEASURED (q)**, `f0894109`).

**MEASURED (q)** — `f0894109`, by bisection on threads with explicit `stack_size`, no parser, no reducer, no tuple space, no `tokio`:

```math
S(N) \;=\; 225{,}280 \;+\; 194{,}970 \cdot N \quad \text{bytes (debug)}
```

against a relayed 194,694 B/level — agreement to **0.14 %**. Under `gdb` the per-level delta is a dead-constant **194,992 B** across ten consecutive levels. The fit predicts that depth 9 fits in 2 MiB and depth 10 does not; Rust's default spawned-thread stack is *exactly* 2 MiB, so **the measurement recovers the reported threshold without having been shown it**.

★ **Why one level cost 195 kB.** `gdb` per-frame attribution over the 20-frame cycle put **169,728 B — 87 % — in `SubstituteTrait<Expr>::substitute_no_sort` alone**. It is a 40-arm `match` over `ExprInstance`, and rustc does not overlap the stack slots of mutually exclusive arms at `-O0` (**MEASURED (q)**, `f0894109`). This is the single most useful diagnostic in the campaign: it explains why debug-to-release ratios are large and per-traversal, and why a hand-written work item is *smaller* than the frame it replaces (§6.1).

#### 5.1.2 Architecture of the repair, and why this shape

The chosen shape is an **explicit LIFO work stack of defunctionalised continuations, over borrowed sub-terms, with the environment carried as a delta into a borrowed root**.

Three design decisions, each with its rejected alternative.

**(a) Work items borrow; they do not own.** The recursion's frames held `&`-references into the input; a worklist of *owned* sub-terms would have had to clone them, and `<Par as Clone>::clone` is itself $`\Theta(d)`$ at 3,254 B/level release. Borrowing keeps the conversion honest: the heap grows by the *size of a work item* per level, not by the size of a subtree.

**(b) ★ The environment rides as a delta, and this was measured before it was designed.** `Env::shift(j)` is `Env { shift: self.shift + j, ..(*self).clone() }` — a full `HashMap<i32, Par>` clone, hence a $`\Theta(d)`$ `Par` clone of *every bound value*, *at every binder level*. **A worklist storing an owned `Env` per item would therefore have stayed $`\Theta(d)`$ however the traversal itself was written.**

**MEASURED (q)** — `f11ffb54`, *before* the conversion: **48,878 B/level** under a deep binding against **33,242 B/level** under a ground one. The difference, **15,636 B/level**, is `Par::clone`'s 15,875 to within **1.5 %**. Diagnosis confirmed.

The repair reads the SCC's *entire* use of `Env` — `get`, the `shift` field, and `shift(j)`; **there is no `put`** — and replaces it with a work item carrying `(depth, shift_delta)` plus **one borrowed root environment**, resolving

```rust
// ELIDED — the resolution rule, not compilable in isolation.
// Production form: rholang/src/rust/interpreter/substitute_drive.rs
root.env_map[(root.level + root.shift + shift_delta) - k - 1]
```

Equality with a chain of owned `Env::shift`s is *by construction*, and two guards keep it so: a source scan asserting `.put(` never appears in the SCC, and a test comparing the view against an actual chain of `Env::shift`s. **MEASURED (q)** — after the conversion the deep-environment and ground-environment readings are **identical (0 B/level both)**, which is the mechanism confirmed rather than assumed.

**(c) A hand-written driver, not one mode-flagged driver over a shared table.** The conversion removed a *dead* `SubstituteTrait<Expr>::substitute` that had silently diverged from its live twin — its `EMinusBody` arm rebuilt the term as an `EPlusBody`. A single mode-flagged driver would have made the dead method's behaviour a *configuration* of the live one and quietly promoted it (**DERIVED**, `f11ffb54`).

**Algorithm 1 (SUBSTITUTE-DRIVE).** *The shape shared by every driver in Family A.*

```pseudocode
 1  procedure DRIVE(root, env_root)
 2      work  ← [ Descend(root, depth 0, shift 0) ]      ▷ LIFO; the only frame is this one
 3      vals  ← [ ]                                       ▷ completed children, in order
 4      while work is not empty do
 5          item ← POP(work)
 6          case item of
 7            Descend(t, d, s) →
 8                (children, k) ← SPLIT(t)                ▷ the canonical child-slot table
 9                PUSH(work, Combine(SHELL(t), k))        ▷ the resume point, pushed FIRST
10                for c in REVERSE(children) do           ▷ reversed, so POP yields left-to-right
11                    PUSH(work, Descend(c, d', s'))      ▷ d', s' per the binder rule of t
12            Combine(shell, k) →
13                kids ← SPLIT_OFF(vals, |vals| − k)      ▷ takes exactly what this Descend pushed
14                PUSH(vals, REBUILD(shell, kids))
15      return POP(vals)
```

Read line by line. Line 2 seeds the work stack with the root; line 3 the value stack, which exists because this is a *bottom-up* fold — a node cannot be rebuilt until its children are. Line 9 is the whole trick: **the resume point is pushed before the children**, so it is popped after them, which is what "return to the parent" meant in the recursive form. Line 10's reversal makes `POP` yield children left to right, reproducing the recursive traversal order **exactly** — and order matters, because a differential that only compared final values would pass on a driver that visited children in the wrong order for every commutative operator. Line 13's `split_off` is the invariant that makes the two stacks agree: *every `Descend` eventually pushes exactly one value, and every `Combine` removes exactly the values its own `Descend` pushed*. Line 8's `SPLIT` is not written per driver; it is the **canonical child-slot table** of §5.7.2, whose matches are exhaustive with no `_` arm, so a schema change is a compile error rather than a silently skipped child.

#### 5.1.3 Results

| subject | before (debug) | after | before (release) | after | provenance |
|---|---:|---:|---:|---:|---|
| `substitute_no_sort` | 195,728 | **0** | 27,179 | **0** | **(q)** `f11ffb54`; **(f)** flat 28 KiB↔28 KiB |
| `substitute` (sorted entry) | 195,754 | **0** | — | **0** | **(q)** `f11ffb54`; **(f)** flat 32 KiB↔32 KiB |
| `substitute_binders`, deep env | 48,878 | **0** | — | **0** | **(q)** `f11ffb54`; **(f)** flat 28 KiB↔28 KiB |
| `substitute_binders`, ground env | 33,242 | **0** | — | **0** | **(q)** `f11ffb54` |
| `sort` (ParSortMatcher) | 78,592 | **0** | 6,485 | **0** | **(q)** `9ab6b0eb`, `f11ffb54`; **(f)** flat 12 KiB |
| `score_cmp` | 1,329 | **0** | 128 | **0** | **(q)** `6ce7c5b9`; **(f)** flat 12 KiB |
| `score_cmp_wide` (**width** axis) | 201 | **0** | 0 | **0** | **(q)** `6ce7c5b9`; **(f)** flat to width 65,536 |
| `tree_clone` | 1,578 | **0** | 485 | **0** | **(q)** `6ce7c5b9`; **(f)** flat 12 KiB |
| `tree_drop` | 370 | **0** | 204 | **0** | **(q)** `6ce7c5b9`; **(f)** flat 12 KiB |
| `Tree`'s derived `PartialEq` | 719 | **0** | — | **0** | **(q)** `6ce7c5b9` |
| `normalize` | 43,542 | **0** | 7,261 | **0** | **(q)** `CONVERTED_DEPTH` note; **(f)** flat 32 KiB |
| `eval_with_nots` | 21,584 | **0** | 3,359 | **0** | **(q)** `a3fd6fe4`; **(f)** flat 12 KiB |
| `pretty`, `pretty_wide` | — | **0** | — | **0** | **(f)** flat 12 KiB both axes |

**The reported reproducer is fixed.** `@"OUT"!([[[[[[[[[[0]]]]]]]]]])` at depth 10 now survives the 2 MiB stack a `tokio` worker gets, and `reported_reproducer_depth_survives_a_default_worker_stack` is no longer `#[ignore]`d — **MEASURED (f)**: it passes in the fresh release run.

#### 5.1.4 What it cost, and what leg-1 did *not* buy

★ **Leg-1 was honest about being insufficient, and that is a result.** Before any conversion, a de-cloning pass removed deep copies from `prepend_expr`/`_connective`/`_new`/`_bundle`, from four `Par::prepend_*` methods, from `sub_exp`'s discriminant read, from both `SubstituteTrait<Expr>` entries, and from twelve `.iter().map(p.clone())` sites.

**MEASURED (q)** — `f0894109`: debug **194,970 $`\rightarrow`$ 195,728 B/level** (+0.4 %, within bisection resolution — i.e. **unchanged**); release **36,416 $`\rightarrow`$ 27,179 B/level** (**$`-`$ 25.4 %**). Its own verdict: *it removes $`O(D^2)`$ heap churn and a real slice of the constant, and it cannot change the class.* Reported here because a report that showed only the successful leg would misrepresent how much work a class change actually takes.

**A latent bug surfaced by the unification.** `reduce.rs`'s private `expr_locally_free_ref` hard-coded depth 0 in its `EVar` arm where the by-value trait threads `depth` through — sound at its only call site, **wrong** at `prepend_expr`, which `sub_exp` calls at pattern depth $`> 0`$ (**DERIVED**, `f0894109`).

**Intercepts rose.** The converted subjects sit at 12–32 KiB minimum stack where several pre-conversion subjects sat at 12 KiB. The criterion is slope, not intercept (§2.3), and this is the expected direction: a driver frame holding two `Vec`s and a match on a work item is bigger than a leaf call.

**A scope correction that changed the enumeration method.** The audit's original enumeration was a Tarjan SCC over `RhoTypes.proto`. That method **structurally cannot see a recursive Rust type that is not a proto message** — and `models/src/rust/rholang/sorter/score_tree.rs` defines one, `Tree<T>`, carrying four $`\Theta`$ traversals plus a fifth on the width axis (**MEASURED (q)**, `6ce7c5b9`). The lesson is methodological and is recorded in §7.4.

⚠ **A trap the gate's own probe would have fallen into.** A linear chain sorts as a **one-element** vector, and `Vec::sort_by` on one element performs **zero** comparisons — so `compare_score` is *never entered* by the `sort` subject. Converting `sort_match` alone would have left the comparator $`\Theta(d)`$ **and the gate would still have passed**. The comparator is therefore a subject in its own right, driven by a *pair* of terms that differ only at the leaf (**DERIVED**, `6ce7c5b9`).

⚠ **A width-axis defect that only `-O0` could see.** `compare_score_nodes` recursed on the list *tail* (`&left[1..]`). At `-O2` LLVM turns that into a loop and the measured slope is **0**; at `-O0` it is **201 B per sibling**. Relying on a codegen accident for a consensus-liveness property is not acceptable, so it was made an explicit loop, which holds by construction in both profiles (**MEASURED (q)**, `6ce7c5b9`).

#### 5.1.5 The named residual of Family A

`Env::get` returns its value **cloned**, because substituting a `BoundVar` splices the bound term into the result. That clone is `<Par as Clone>::clone`, and **this call site cannot be removed, because the copy *is* the meaning of substitution**. **MEASURED (q)** `f11ffb54`: 15,850 B/level debug — `Par::clone` to within 0.2 % — identical in the recursive form, and bounded by the depth of the **bound value** rather than of the term traversed. **MEASURED (f)**: 7,460 B/level release at HEAD. It sits in the tripwire as `substitute_deep_binding`, never in the converted list, *so that the residual is visible instead of folded into a claim of full depth-independence*.

Two sorter arms remain: `sort_nested_set` **4,778** and `sort_nested_map` **7,509** B/level release (**MEASURED (f)**), self-contained arms measured against the derived floor `clone_nested_set` at **4,096**.

---

### 5.2 Family B — the expression evaluator and the `tokio` fire-and-forget driver

#### 5.2.1 The expression-evaluator trampoline (`a929a2d6`)

**The defect. DERIVED** — six mutually recursive evaluators in `rholang/src/rust/interpreter/reduce.rs` — `eval_expr`, `eval_expr_to_par`, `eval_expr_to_expr`, `eval_single_expr`, `eval_to_bool`, `eval_to_i64` — form a post-order fold whose native call depth is $`\Theta`$(term nesting).

**MEASURED (q)** — `a929a2d6`, `rholang/examples/so_probe.rs` on a **default 8 MiB** thread stack, release: arithmetic nesting (`EPlus`) overflowed at depth **$`\approx`$ 1,500**; list nesting (`EList`) at **$`\approx`$ 750**.

**The architecture.** The six become **thin wrappers over a single explicit-worklist driver `eval_drive`**: an `EvWork` heap stack (LIFO DFS (depth-first search), children pushed reversed so they pop left to right) and an `EvVal` value stack, with each arm's post-order body factored into a shared `combine_*` helper **used verbatim by both the driver and a retained recursive twin**.

★ **Why a *shared* combining helper rather than two independent implementations.** The differential oracle is only worth running if the two sides can actually disagree about the thing under test. Sharing the arm bodies makes the differential test the **driving** — push order, pop order, fold resumption, error position — which is what a worklist conversion can actually get wrong, while arm *semantics* are guarded separately by a round-trip identity test. §5.7.5 shows this was not theoretical: a later structural check found that the twins share **18 `combine_*` helpers**, every one of which is also called from outside the twin family.

**Owned intermediates are re-entered, not linearised.** Values that are *not* sub-terms of the input — `eval_var` results, method-apply results, `%%`/`++`/`--` `Set`/`Map` results, sorted `Set`/`Map` elements — are re-evaluated by direct wrapper calls, **each a fresh bounded drive**. This bounds them by the depth of the *bound value*, exactly as §5.1.5 does, rather than pretending they vanish.

**★ The interleaving a naïve post-order would have silently broken.** The five binary-operator helpers do **not** evaluate both operands and then check them:

```rust
// ELIDED — the pre-conversion shape, quoted for its ORDER, from `a929a2d6`.
let v1 = single_expr_instance(&eval_with(p1)?)?;   // check p1 HERE
let v2 = single_expr_instance(&eval_with(p2)?)?;   // only then touch p2
```

so if `p1` evaluates but is not a single value, **`p2` is never evaluated**. A machine that evaluated both children and then extracted would report `p2`'s error where the recursive form reports `p1`'s — an observable divergence in `EvalError`, on the path that decides `where`-clause guards. The machine therefore carries an `extract` flag on the operand work item and applies `single_expr_instance` through an `Extract` continuation pushed **before** that operand's `ParK`, so it runs the instant that operand finishes and strictly before the next is popped (**DERIVED**, `a3fd6fe4`).

⚠ **And the differential proves the order matters.** A binary `Combine` that pops its two children in the wrong order yields the same **multiset** of operands — indistinguishable from correct for every commutative operator (`+`, `*`, `==`, `&&`). The corpus therefore runs all 13 binary arms at 12 operand pairs, **each pair in both orders**, and `swapping_operands_actually_changes_the_answer` asserts that at least seven operators genuinely disagree under swap. Without it, an ordered-equality differential could pass on a corpus where order never mattered — *rigorous-looking and worthless*.

**The charge freeze.** Every `reserve_primitive` / `substitute_and_charge` / `Cost::` call is **relocated whole** — same count, order, operands, and charge-before-error ordering. Pre-order charges run in the descend handler before any child push; post-order charges run in `Combine` after all child values are available. Production `reserve_` site count **unchanged at 108** (**DERIVED**, `a929a2d6`).

**Results. MEASURED (q)** — `a929a2d6`, `so_probe`, default 8 MiB stack, release:

| family | before | after |
|---|---:|---|
| `plus` (nested `EPlus`) | overflow at $`\approx`$ 1,500 | **OK at 20,000 and 50,000** |
| `list` (nested `EList`) | overflow at $`\approx`$ 750 | **OK at 20,000 and 50,000** |
| `methodchain` (deep `EMethod` target) | *(new subject)* | **OK at 20,000** |

**MEASURED (q)** — `a3fd6fe4`, the separate `rho-pure-eval` SCC: `eval_with_nots` **21,584 $`\rightarrow`$ 0** debug, **3,359 $`\rightarrow`$ 0** release; `the_machine_survives_a_depth_the_oracle_could_not` evaluates **20,000 nested negations** on an ordinary test thread, where at the old constant the recursive form would have needed **$`\approx`$ 412 MiB**. **MEASURED (f)**: flat at 12 KiB, both ends.

**Cost. DERIVED**, `a929a2d6`: the residual ceiling of ~75k–100k is `drop_in_place::<Par>` — the *inherent* recursive `Drop` of a 100k-deep data structure, `gdb`-confirmed, with the evaluator **absent** from the overflow trace. That is a property of the data depth, not of the evaluator, and it is Family D's problem (§8.2).

⚠ **Named, not fixed** (**MEASURED (q)**, `a3fd6fe4`): the `eval_with` subject on a nested `EList` chain still measures **15,872 B/level**, which is `<Par as Clone>::clone` exactly. That is *not* this SCC — the `EListBody` arm returns `par_with_expr(expr.clone())` **without descending** — so on that shape the probe measures the derived class and nothing else. The gate subject is `eval_with_nots` precisely because it is the shape that actually recurses.

#### 5.2.2 ★ The `tokio` fire-and-forget driver — establishing the mechanism, not assuming it

![async detached driver](figures/async-detached-driver.svg)

**Figure 4** — *`figures/async-detached-driver.puml`*. Two resources, two defects, one repair.

**The relationship to stack safety had to be established rather than assumed, and it turns out there were *two* distinct $`\Theta(N)`$ chains on this path, in two different resources.**

**Chain (i) — nested `poll`, in the native stack. DERIVED** from the deleted code, `git show 9843e4b6 -- rholang/src/rust/interpreter/reduce.rs`. The reduction path $`\mathtt{eval} \rightarrow \mathtt{produce}/\mathtt{consume} \rightarrow \mathtt{dispatch} \rightarrow \mathtt{eval}`$ was a chain of futures **awaited inline** through `Box::pin`. Awaiting a future inline means the outer future's `poll` calls the inner future's `poll` **in the same native frame**. A depth-$`N`$ chain of inline awaits is therefore a depth-$`N`$ chain of `poll` frames — recursion wearing an `async` costume. The mitigation that had been shipped is the evidence:

```rust
// VERBATIM, deleted by 9843e4b6 from rholang/src/rust/interpreter/reduce.rs.
const STACK_RED_ZONE: usize = 1024 * 1024;      // 1 MB
const STACK_GROW_SIZE: usize = 2 * 1024 * 1024; // 2 MB

struct StackGrowingFuture<F> { inner: F }

impl<F: Future> Future for StackGrowingFuture<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        stacker::maybe_grow(STACK_RED_ZONE, STACK_GROW_SIZE, || inner.poll(cx))
    }
}
```

Its own doc comment named the mechanism: *"Each poll of this recursive future chain adds stack frames. In debug builds, unoptimized async state machines consume ~1–2 kB per recursion level, causing stack overflow with the default 2 MB thread stack."* ★ **The red zone had already been raised from 128 kB to 1 MB** because *"a single recursion frame in the Rholang interpreter consumes more than 128 kB between stacker checks"*. That is a $`\Theta(d)`$ native-stack traversal being **fed** rather than removed.

**Chain (ii) — parked parents, in the heap. DERIVED**, `reduce.rs:96-99`: *"`tokio::spawn` already gives an $`O(1)`$ async stack, but each parent `await`ing its children pins an $`O(N)`$ parked-parent chain (`shortslow`: 32,768 parked parents)."* This is a **different resource**. A parent that awaits a *spawned* child does not nest `poll` frames — the child is rooted in its own task — but the parent's future object stays **live and parked on the heap** until the whole subtree finishes. 32,768 nested continuations meant 32,768 simultaneously live future objects.

$`\Rightarrow`$ **The two are genuinely distinct and both were real.** Conflating them would have produced either a wrong fix or a wrong claim; the report keeps them apart.

**The architecture of the repair.** A **per-deploy atomic completion counter** with an error sink, and **detached** spawns at all five join sites.

```rust
// VERBATIM — rholang/src/rust/interpreter/reduce.rs:112-158, quoted in full
// because the ordering of the three fields' operations is the whole design.
pub(crate) struct DriveState {
    /// Outstanding tasks (root + all live detached children). Initialised to 1 (the root eval task).
    live: AtomicUsize,
    /// Flat, `Located`-wrapped errors pushed by detached tasks.
    sink: Mutex<Vec<InterpreterError>>,
    /// Fired exactly once, on the 1->0 transition, to wake `inj`.
    done: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// RAII decrement of `live`. Dropped LAST inside each detached task (after the error push), so the
/// sink is complete when `done` fires. Covers Ok / Err / `?` / panic — no missed decrement.
struct LiveGuard(Arc<DriveState>);

impl Drop for LiveGuard {
    fn drop(&mut self) {
        if self.0.live.fetch_sub(1, Ordering::AcqRel) == 1 {
            if let Some(tx) = self.0.done.lock()
                .expect("DriveState.done mutex poisoned").take()
            {
                let _ = tx.send(());
            }
        }
    }
}
```

**Algorithm 2 (SPAWN-DETACHED).** *The counted, unawaited spawn.* Production form at `reduce.rs:345-379`.

```pseudocode
 1  procedure SPAWN-DETACHED(drive, child_path, fut)
 2      FETCH-ADD(drive.live, 1, AcqRel)          ▷ ★ INCREMENT BEFORE SPAWN
 3      spawn detached task:
 4          guard ← LiveGuard(drive)              ▷ dropped LAST, after line 8
 5          outcome ← AWAIT(CATCH-UNWIND(fut))
 6          case outcome of
 7            Ok(Ok(_))  → ()                     ▷ the child's value is DISCARDED
 8            Ok(Err(e)) → PUSH(drive.sink, Located{path: child_path, source: e})
 9            Err(panic) → PUSH(drive.sink, Located{path: child_path, source: ReduceError})
10      return immediately                        ▷ the parent does NOT await
```

Line 2 is not an optimisation, it is a correctness requirement: incrementing **inside** the spawned task would allow `live` to reach 0 between the parent's return and the child's first poll, firing `done` while work remains. Line 4's guard is RAII (resource acquisition is initialisation) so that **`Ok`, `Err`, `?`-propagation and panic** all decrement — there is no path that misses one. Line 5's `catch_unwind` is **mandatory** rather than defensive: a panicking deploy must still record an error, or `is_failed` would flip to *success*. Line 7 discards the child's `Ok`: these five sites only ever conveyed `Skip`/error upward, and both the old aggregate and this sink discard `Ok(_)` and capture only `Err` — so the error plumbing is byte-identical (**DERIVED**, `a0a50473`).

The complementary half is in `inj`: seed a fresh `DriveState` with `live = 1` (the root eval), install it on `self.drive`, run the root eval, **drop the root guard**, await the `oneshot`, then sort the sink by `located_path` for a deterministic **display** order — explicitly *not* consensus (**DERIVED**, `reduce.rs:664-692`).

**Why this shape rather than the alternatives.**

* **`FuturesUnordered`** — the previously required architecture — still has the parent awaiting the join, so it keeps chain (ii) intact. The repro spec was updated to *forbid* it and require `spawn_detached`, and it records why: detaching is *"a strictly stronger form of the non-blocking, completion-order branch draining this repro spec guards"* (**DERIVED**, `53b8a85f`).
* **`stacker`-based stack growth** attacks chain (i) only, by *feeding* the recursion, and does nothing about chain (ii). It was deleted, not retuned.
* **A `JoinSet`** would reintroduce the awaiting parent.

**Consensus neutrality.** Concurrency is *unchanged* — the same spawns happen — so COMM order is unchanged. The differential harness checks this rather than arguing it: **MEASURED (q)**, `55b97f84` (5/5 fresh-genesis) and `a0a50473` (6/6): cost, `is_failed`, balance and the **COMM multiset** byte-identical old-vs-new, with Layer-1 replay $`=`$ play exactly.

**Results. MEASURED (q)** — `857c62fe`, the `deep_recursion_{long,short}slow` contracts (32,768 self-calls), debug:

| condition | load | wall | thread CPU | %CPU |
|---|---:|---:|---:|---:|
| idle | ~6 | 95.96 s | 93.66 s | 97 % |
| 32 spinners ($`2\times`$) | ~40 | 121.91 s | **93.73 s** | 77 % |
| 96 spinners ($`4\times`$) | ~108 | **609.6 s** | 93.4 s | — |

The pre-conversion parked-parent chain required a **300 s** budget for the same work; the heap-bound driver needs **93.7 s of CPU** (**DERIVED**, `857c62fe`, from the budget the file used to carry). That is the throughput result of the async conversion: **$`\approx 3.2\times`$ less work for the same contract**, measured in the one clock that is invariant to contention.

**What it cost.** The error sink's **push order is non-deterministic** (that is inherent to detachment), so `inj` sorts it; the ordering is documented as *display* only. And `catch_unwind` on every child is not free — **NOT MEASURED**: no isolated benchmark of `spawn_detached`'s per-spawn overhead exists, and none was constructed for this report (§5.9).

⚠ **A methodology defect found in this family and fixed as an instrument.** The two contract tests carried a single 180 s `tokio::time::timeout` doing two jobs — liveness *and* work. Wall time is computation divided by the share of a CPU the scheduler granted, and that denominator belongs to every other process on the machine. It caused **one false attribution**: a full-suite run at load ~125 recorded both RED, they were investigated as a regression, and they passed in isolation on the same tree. ★ **No larger wall budget fixes it**: a budget $`B`$ is safe iff $`B \geq T_{\text{idle}} \cdot C`$ where $`C`$ is contention imposed by *other* processes, and $`C`$ is unbounded. The instrument was therefore changed, not the number: `LIVENESS_TIMEOUT = 900 s` wall, `CPU_WORK_BUDGET = 180 s` **thread CPU**, read from `/proc/thread-self/stat` fields 14/15. **MEASURED (q)**, `857c62fe`: the two in-process readings sum to 189.4 s against `/usr/bin/time`'s 189.43 s — **0.02 %** agreement, which validates the field indices, the ticks-per-second constant and the single-thread attribution simultaneously.

---

### 5.3 Family C — the codecs, and the malloc profile

#### 5.3.1 The baseline: what had never been measured

★★ **Four of the eight traversals over the `Par` family had never been measured at all.** `eq`, `hash`, `ord` and `debug` appeared **nowhere** in the gate at that point — not in `CONVERTED_DEPTH`, not in `TRIPWIRE_DEPTH`, and in no `assert_slope_below` call. **Their absence meant *unmeasured*, not *flat*, and the two are indistinguishable from outside.**

**MEASURED (f)** — re-run in full for this report: `/tmp/sd_s0_release.log`, release, 2026-07-29. It reproduces the recorded S0 table **to the byte in every row**:

| subject | four-quadrant label | ladder | $`S_{\mathrm{lo}}`$ (B) | $`S_{\mathrm{hi}}`$ (B) | growth (B) | **B/level** |
|---|---|---|---:|---:|---:|---:|
| `clone` | `clone` | 16 $`\rightarrow`$ 128 | 65,536 | 430,080 | 364,544 | **3,254** |
| `par_drop` | `par_drop` | 16 $`\rightarrow`$ 256 | 12,288 | 45,056 | 32,768 | **136** |
| `eq` | `eq` | 16 $`\rightarrow`$ 256 | 12,288 | 65,536 | 53,248 | **221** |
| `hash` | `hash` | 16 $`\rightarrow`$ 256 | 12,288 | 45,056 | 32,768 | **136** |
| `ord` | `ord` | 16 $`\rightarrow`$ 128 | 12,288 | 61,440 | 49,152 | **438** |
| `debug` | `debug` | 16 $`\rightarrow`$ 128 | 28,672 | 167,936 | 139,264 | **1,243** |
| `encode` | `prost_ser` | 16 $`\rightarrow`$ 128 | 12,288 | 45,056 | 32,768 | **292** |
| `prost_de` | `prost_de` | 4 $`\rightarrow`$ 32 | 12,288 | 126,976 | 114,688 | **4,096** |
| `par_drop` | `par_drop@gate` | 256 $`\rightarrow`$ 4096 | 45,056 | 598,016 | 552,960 | **144** |
| `encode` | `prost_ser@gate` | 64 $`\rightarrow`$ 1024 | 28,672 | 319,488 | 290,816 | **302** |

Three facts the table makes visible:

1. **`prost_de` is the most expensive per level of the eight**, by $`\approx 1.26\times`$ over `clone` — and it is the **only** one of the eight that `prost` itself caps.
2. **`hash` is the cheapest**, at parity with `par_drop`. It had never been measured, and reading *unmeasured* as *unimportant* would have been supported by this number — while the *reason* it is cheap (the hand-written impl walks the same field set as `PartialEq` and allocates nothing) is exactly why it is also the easiest to convert.
3. **`ord` costs $`\approx 2\times`$ `eq`** despite comparing the same structure, because rustc's derived `cmp` materialises an `Ordering` per field and cannot reuse `eq`'s early-exit shape.

★ **The three columns are themselves the finding.** A driver list read off a `#[derive]` scan sees only the derived column: `PartialEq` and `Hash` are **stripped** from `prost`'s output by `models/build.rs` and written **by hand** in `models/src/lib.rs` — where `<Par as PartialEq>::eq` deliberately ignores `locally_free`, which no derive would do — and `Drop` is rustc's implicit glue with **no `impl` anywhere**. That is why the generated `DERIVE_DISPOSITION_REGISTRY` states in its own doc comment that it is a **lower bound**, and why a hand-picked list of four already missed `Hash` once (**DERIVED**, `7c74260d`, `44535d75`).

#### 5.3.2 The cold-store **encoder** — a single-walk trampolined serializer (`c28f4cf6`, `a169cc61`)

**The defect. DERIVED** — the derived `Serialize` recursed once per level, *and* `bincode::serialize` **traverses the term twice**: `serialized_size` then `serialize_into` (`bincode-1.3.3 src/internal.rs:25-37`). **MEASURED (q)**, `a169cc61`, by an in-binary control: `bincode_ser_derived` needed 8,191 B at depth 4 and **925,688 B at depth 4,096** — ~**224 B/level** release.

**The architecture: a pre-order op stack over borrows, with no value stack at all.**

```rust
// VERBATIM — models/src/rust/rholang/wire_encode.rs:150-170 (comments elided).
#[derive(Clone, Copy)]
enum Op<'a> {
    Node { node: &'a dyn WireNode, field: u16 },
    Seq { seq: &'a dyn WireSeq, index: u32, len: u32 },
    MapEntries,
}
```

★ **Why *no* value stack, when §5.1's driver needs one.** Serialisation is **pre-order into a contiguous buffer**: a node's bytes are complete the moment its last child has been written, so nothing has to be parked and recombined. This is the same structural observation that makes Cheney's algorithm work [[Cheney 1970](#ref-cheney1970)] — the output region *is* the accumulator — and it is why the encoder's per-level heap cost (**64 B**, §5.3.4) is a quarter of the decoder's (**304.7 B**).

★ **Why the table is generated from the protobuf `FileDescriptorSet` rather than from `#[prost(...)]` attributes.** The attributes describe the *protobuf* wire. This encoding is `bincode` over `serde`, whose layout is serde's **declaration order**, which serde's derive exposes **nowhere at run time**. The descriptor is the only artefact carrying it (**DERIVED**, `c28f4cf6`).

**Three bugs the suite found, each recorded where it was made** (**DERIVED**, `c28f4cf6`) — reported because they are the actual difficulty of this transformation:

1. **`prost` does not interleave oneofs with plain fields.** It emits every plain field first, then every oneof (`prost-build-0.14.3 code_generator.rs:270-291`). `TaggedContinuation` declares its oneof *before* `guard`, so the intuitive rule produced a **95-byte encoding with its halves exchanged — same length, same byte multiset**. No round-trip could see it; the *write* differential did.
2. **`&'static` slices with identical contents are merged by the linker.** `EPATHMAP_PROGRAM` is byte-for-byte `ELIST_PROGRAM`, so a downcast keyed on the program's *address* reinterpreted an `EList` as an `EPathMap` — **SIGSEGV (the segmentation-fault signal)**. Replaced by an explicit `WireNode::wire_as_pathmap`; the merge is now an asserted fact.
3. **A global allocation counter counts other test threads.** Made per-thread — the assertion had passed at `--test-threads=1` and failed in the suite.

**Why round-trip is not the property.** *A codec that encodes differently but decodes its own output round-trips — and forks.* The derived `Serialize` therefore **stays compiled** as the encode oracle, for the same reason `par_codec_differential` keeps the derived `Deserialize`: it is generated by the compiler and **cannot drift**. Anti-vacuity is executed, not asserted: `the_encode_differential_can_go_red` perturbs two field emissions and one variant index and requires the verdict to **reject**, naming the clause, with a control passing before and after.

**Results — space. MEASURED (q)**, `a169cc61`, release, by bisection with the pre-conversion body in the **same binary**:

| subject | depth 4 | depth 4,096 | slope |
|---|---:|---:|---:|
| `bincode_ser` (converted) | 8,191 B | **8,191 B** | **0 B/level** |
| `bincode_ser_derived` (control) | 8,191 B | 925,688 B | ~224 B/level |
| `bincode_de` (converted, Stage F) | 8,191 B | **8,191 B** | **0 B/level** |

**MEASURED (f)**: `bincode_ser` and `bincode_de` both flat at **12 KiB** at depth 4 and depth 4,096, release.

**MEASURED (q)**, `c28f4cf6`, `models/tests/wire_encode_space.rs`: the op stack is $`\Theta(\text{depth})`$ **not** $`\Theta(\text{size})`$ — **5 entries at width 4 and at width 65,536**; **2.000 entries per nesting level** (down from 4.000, because *a sequence's last element is a tail call, exactly as a node's last field is*); `size_of::<Op>() = 32` B.

★ **Independently confirmed for this report.** §5.3.4's massif series gives the op stack at depth 4,096 as **524,288 B** = 16,384 entries $`\times`$ 32 B for a high-water of ~8,194 entries — i.e. **2.00 entries per level**, arrived at by a completely different instrument.

#### 5.3.3 The cold-store **decoder** — an obligation stack with eighteen value stacks (`9a5521a2`)

**The defect. MEASURED (q)**, `9a5521a2`: `Par` decode is $`\Theta(d)`$ at **28,362 B/level debug / 12,894 release** — $`D_{\max}`$ **73 / 161** on a 2 MiB worker, **the shallowest member of the whole family**, 1.8–4.5 $`\times`$ below `<Par as Clone>::clone`. And, as §2.2 records, the only member whose failure is **permanent and replicated**.

★★ **Leg-2 made it reachable.** Before the substitution conversion, `substitute` capped the reducer at depth 75; afterwards `substitute` and the sorter are unbounded, so terms deep enough to break the decoder can now be *produced*. **A conversion can promote a latent defect to a live one**, and this is the campaign's worked example.

★ **Why a hand-written parser, and why wrapping `serde` cannot work.** The reduction is short and decisive: to *defer* a child you need its **byte extent**; in a **non-self-describing** format an extent requires a **schema-driven parser**; *that parser is this decoder*. Confirmed independently rather than argued: `bincode`'s `deserialize_ignored_any` returns `Err("Bincode does not support Deserializer::deserialize_ignored_any")`, killing `IgnoredAny`; and `DeserializeSeed` passes state *into* a child and never returns control *out of* a partially-completed visitor. ⚠ The one `serde` wrapper the ecosystem shipped for this problem, `serde_stacker`, **grows the stack** — the same non-solution as `stacker` in §5.2.2 (**DERIVED**, `9a5521a2`).

**Algorithm 3 (COLD-DECODE).** *An obligation stack of bounded opcodes plus per-type value stacks.*

```pseudocode
 1  procedure COLD-DECODE(bytes)
 2      ops  ← [ ParStart ]                    ▷ bounded opcode alphabet, LIFO
 3      vals ← 18 per-TYPE value stacks        ▷ pars, sends, receives, exprs, …
 4      while ops is not empty do
 5          op ← POP(ops)
 6          case op of
 7            XStart →                          ▷ read X's own scalar fields NOW
 8                (flags, counts) ← READ-HEADER(bytes)
 9                PUSH(ops, XBuild(flags, counts))          ▷ the resume point FIRST
10                for each child slot s of X, in REVERSE do
11                    PUSH(ops, Rep(kind(s), count(s)))
12            Rep(kind, n) →
13                if n > 0 then
14                    PUSH(ops, Rep(kind, n − 1))           ▷ ★ n ops are NEVER materialised
15                    PUSH(ops, START-OP(kind))
16            XBuild(flags, counts) →
17                kids ← SPLIT-OFF(vals[kind], |vals[kind]| − counts)
18                PUSH(vals[X], CONSTRUCT-X(flags, kids))   ▷ COMPLETE struct literal
19      assert exactly one value remains, on vals[Par]
20      return POP(vals[Par])
```

Line 14 is the hostile-input defence and deserves its own sentence. A counted repeat is re-pushed with $`\mathtt{remaining} - 1`$ **rather than expanded into $`n`$ opcodes**, so a declared `n = u64::MAX` costs $`O(1)`$ memory and dies on the first element that runs out of input. Combined with the fact that **every value costs at least one input byte**, allocation is $`O(\text{input})`$ for *every* input, including malformed ones. Line 17's `split_off` both preserves sibling **order** and is the same one-value-per-`Start` invariant as Algorithm 1. Line 18's *complete* struct literal — no `..Default::default()` anywhere in any decode path — makes a new schema field a **compile error** rather than a silently defaulted one.

**★ Teardown is iterative too, and that is not an afterthought.** `<Par as Drop>` is $`\Theta(d)`$ at 470 B/level, so a decode that **failed at the last byte of a deep term** would abort *while rejecting a hostile input* — the same `SIGSEGV`, on the error path. `Drop for Machine` funnels every value stack through `par_children::dismantle_all`.

**The obligation is language identity, not round-trip.** For every byte string $`b`$, `cold_decode` and `bincode::deserialize` must agree: same `Ok` value, **or both `Err`**. ★ The `Err` half is the consensus-visible one — *a node that accepts a byte string another rejects forks*.

**Validation. MEASURED (q)**, `9a5521a2`:

* differential against the **retained derived oracle** (compiler-generated, so it cannot drift) over an exhaustive corpus — one representative of all **36** `ExprInstance` and all **9** `ConnectiveInstance` arms, every `Par` field, both `EPathMap` serialize arms, every root type;
* a falsification experiment confirmed the differential **fails** on a single-field drift (`ETuple` reading a `remainder` it does not have) — **and that the three `generate_par` proptests stayed GREEN through it**, which is precisely why the constructed corpus exists;
* **1.88 M malformed inputs**, all agreeing: 117,601 truncations at every byte offset; 472,568 byte substitutions; 586,455 out-of-range variant indices; 701,898 hostile `u64` lengths (the family that would abort one side with an OOM (out-of-memory) kill without serde's `size_hint::cautious` cap, reproduced verbatim); trailing bytes **accepted** by both, because tightening that would *narrow the language*;
* depth 4,096 decoded on a **256 KiB** stack, and a *truncated* depth-4,096 term **rejected** on a 256 KiB stack — the error path proved too.

**A partition that is guarded, not asserted.** 47 types go on the machine; **16 keep the derived impl as bounded leaf calls**, on the principled ground that a type outside the `Par` SCC has fixed maximum nesting hence fixed maximum stack — the §2.1 result put to work.

⚠ **A trap worth recording**: the serde oneof numbering is **declaration order, not proto tag**. `EPathmapBody` is proto tag 32 and serde index 25, so reading tags as indices mis-decodes **12 of 36** arms.

#### 5.3.4 ★★ The malloc profile — where the allocations moved

![heap: where the allocations moved](figures/heap-where-allocations-moved.svg)

**Figure 5** — *`figures/heap-where-allocations-moved.puml`*. The heap result, from massif and DHAT.

All figures in this section are **MEASURED (f)**, 2026-07-29, one arm per process, pinned, via `models/benches/wire_encode_massif.rs`. Raw data in Appendix B.

**(a) Production shape** — the datum is depth 2, which is **95.43 %** of measured produces (**MEASURED (q)**, the distribution instrumented over five interpreter suites, 1,773 datums, `models/benches/wire_encode_bench.rs`). 20,000 encodes per arm.

| arm | total bytes | total blocks | blocks / call | peak heap (`t-gmax`) | heap **reads** | heap **writes** |
|---|---:|---:|---:|---|---:|---:|
| `derived` = `bincode::serialize` | 12,828,259 | 20,022 | **1.0011** | 6,187 B in 15 blocks | 39.88 MB | 12.83 MB |
| `machine` = `wire_encode::encode` | 12,834,467 | 20,026 | **1.0013** | **12,395 B** in 19 blocks | 45.74 MB | **41.65 MB** |
| `reused` = `wire_encode::with_encoded` | **14,466** | **26** | **0.0013** | 12,394 B in 19 blocks | 32.92 MB | 28.83 MB |

Reading the table:

* **Block count per call is *identical* between the derive and the like-for-like replacement — one allocation each.** `bincode::serialize` sizes then writes into a single exactly-sized `Vec`; `wire_encode::encode` writes into a pooled buffer and hands back one `to_vec`. The conversion is **malloc-neutral** in the like-for-like form.
* **Total bytes differ by exactly +6,208 B over the whole run** — the two thread-local arenas (`OUT` at 4,096 B and `OPS` at 2,048 B), allocated **once**, plus 64 B. Not per call.
* ★ **The peak heap doubles, 6,187 $`\rightarrow`$ 12,395 B, and that is deliberate.** The arenas are *retained*, which is the mechanism that makes the third row possible.
* ★★ **The third row is the actual malloc result.** Where the caller does not need ownership, 20,000 encodes cost **26 blocks and 14,466 bytes in total** — $`770\times`$ fewer blocks and $`887\times`$ fewer bytes than the derive. The pooling policy is bounded on purpose: `MAX_POOLED_OPS = 4096` entries (128 KiB, covering a 2,048-deep term), so *a single pathological encode cannot pin its high-water mark for the life of the thread*.
* ⚠ **The cost, stated plainly: the machine performs $`3.25\times`$ more heap writes** (41.65 MB vs 12.83 MB). Those writes are the op stack. They were previously **native-stack** traffic, which DHAT does not count — so this is a **relocation made visible**, not new work invented. It is nonetheless real memory traffic against real caches, and §5.4 shows it does not cost wall-clock time on this workload.

**(b) Deep shape** — a 4,096-deep `Par`, from the massif time series.

The encoder's working set decomposes **exactly**:

```math
\underbrace{4{,}133{,}992}_{\text{peak}} \;-\; \underbrace{3{,}347{,}560}_{\text{term alone}} \;=\; \underbrace{786{,}432}_{768\ \mathrm{KiB}} \;=\; \underbrace{524{,}288}_{\text{op stack}} \;+\; \underbrace{262{,}144}_{\text{output buffer}}
```

* **op stack 524,288 B** = 16,384 `Op` entries $`\times`$ 32 B, for a high-water of ~8,194 entries — **2.00 entries per level**, confirming `wire_encode_space.rs` by an independent instrument;
* **output buffer 262,144 B**, holding an encoding of **148,546 B** (from the arm's own printed `sink`, $`9{,}506{,}944 / 64`$) — a `Vec` doubling from 4,096 reaches 262,144 at that size, exactly;
* and the massif series shows a **524,288 B sawtooth** thereafter, which is the op-stack allocation being *freed and reallocated* every iteration — the `MAX_POOLED_OPS` policy working as designed, because 16,384 entries exceeds the 4,096-entry pooling cap.

The decoder's transient working set is **1,248,192 B**, obtained as the difference between each decode's peak and the plateau it settles to, and **reproduced to the byte on every observed decode**: peaks 8,364,155 / 11,444,347 / 14,524,539 against plateaux 7,115,963 / 10,196,155 / 13,276,347. That is **304.7 B per nesting level** of heap value stacks.

Also measured, and worth recording: a decoded 4,096-deep term occupies **3,080,192 B in 8,193 blocks** — *bit-for-bit the same heap footprint as one built directly*, which is a useful sanity check on the decoder's construction.

**(c) The migration, per nesting level — the number the user asked for**

| codec | native stack **before** | native **after** | heap **before** | heap **after** | bytes/level ratio |
|---|---:|---:|---:|---:|---:|
| cold-store **writer** | 329 B/level (release, `q`) | **0** | 0 | **64 B/level** | $`5.1\times`$ cheaper |
| cold-store **reader** | 12,894 B/level (release, `q`) | **0** | 0 | **304.7 B/level** | $`42.3\times`$ cheaper |

★ **This is the whole trade in one line.** The conversion does not *remove* the $`\Theta(d)`$ state — a depth-$`d`$ traversal must remember $`d`$ things. It **relocates** that state from a fixed 2 MiB region whose exhaustion is a `SIGSEGV` to a growable region whose exhaustion is an `Err` — and on these two codecs it happens to need **5–42 $`\times`$ fewer bytes per level** as well, because a hand-written work item carries only what the resumption needs, where a compiler-laid-out frame carries every live local of a 40-arm `match` (§5.1.1).

#### 5.3.5 The prost network encoder (`56fb1fd0`) — converted in *work*, not in *stack*, and dormant

★ **This fix is included because it is on the ser/de path and because its honest description is unusual: it does not make anything stack-safe.**

**The defect. DERIVED**, from `prost` source: `Message::encode_to_vec` calls `encoded_len()` — a full recursive walk — and then `encode_raw`; and `encoding::message::encode` calls `msg.encoded_len()` **again for every nested message it writes**. A node's subtree is therefore measured **once per ancestor**:

```math
\sum_{v} |\mathrm{subtree}(v)| \;=\; \Theta(d^2) \quad\text{on a depth-}d\text{ chain}
```

**The repair.** A memoised bottom-up pass measures each node **once** — $`\Theta(n)`$ work at $`\Theta(n)`$ **space** (4 B per message node) where prost's is $`O(1)`$ space. **That is the whole trade, stated as a trade.**

★ **One monotonic cursor suffices, and that is structural rather than lucky.** Pass 1 allocates pre-order ids *at the moment of descent*; pass 2 descends in the same order through the same generated walk, the same counted repeat and the same `BTreeMap` iterator — so the sequence of nodes needing a length prefix **is** the sequence in which ids were allocated. It is bounded on both sides: `open_child` panics naming the slot if the cursor runs **past** the table, and `finish_emit` panics if it stops **short** — the latter being what a pass-2 walk that skipped a child would do, producing a perfectly well-formed protobuf message **with a field missing**.

⚠ **Both passes are still $`\Theta(\text{depth})`$ in native stack.** No stack-safety claim is made and none should be read.

⚠ **It is dormant.** `prost_encode::` appears in **no `src/` tree** of `models`, `rholang`, `rspace++`, `casper`, `node`, `comm` or `shared` — verified mechanically, not by intention (**DERIVED**, `56fb1fd0`).

⚠⚠ **A named residual inside the fix.** `EPathMap` is an **opaque leaf**: its `encode_raw` has three arms — memcpy of interned canonical bytes, ground field-8, or the field walk — and *which* fires depends on a `OnceLock` another thread may fill. Both passes intercept it at exact parity with `prost::encoding::message::encode`. **Correct** and **not depth-independent** are two separate statements, and only the first is claimed.

★★ **The mutation proof runs at the generator, not at the verdict** — and this is the strongest methodological result in the campaign. Two near-misses earlier in this work were mutations that *reported green because they had not applied*. A byte-level mutation proves the **judge** can reject; only a generator-level one proves the **encoder** would have been caught. Three generator mutations, each rebuilt and each required to change `OUT_DIR/rhoapi_prost_wire.rs` before its verdict was accepted (**MEASURED (q)**, `56fb1fd0`):

| mutation | lines | verdict | why it is invisible to weaker checks |
|---|---:|---|---|
| M1 `sort_by_key(min_tag)` removed (declaration order) | 34 | **REJECTED** | `Par::all_par_fields` differs first at byte 925 — **both are 1,031 bytes**, a pure permutation. No length check, no round-trip, and **no protobuf decoder anywhere** can see it. |
| M2 sort key `(is_oneof, min_tag)` | 10 | **REJECTED** | `TaggedContinuation::par_body` differs at byte 0 — **both 1,140 bytes, same byte multiset, halves exchanged**. ⚠⚠ The mirror of the serde defect of §5.3.2(1) — and for protobuf the correct order is **the opposite** of that fix. |
| M3 skip-if-default $`\rightarrow`$ `if true` for `bool` | 68 | **REJECTED** | lengths 18 vs 14, 11 vs 9, 17 vs 11, 8 vs 6 across the corpus. |

⚠ **The $`\Theta(d^2) \rightarrow \Theta(n)`$ claim is checked structurally, never by timing** — a timing assertion in a test suite is a flake. The length table must grow **linearly** across $`d \in \{4, 8, 16, 32\}`$, i.e. constant entries per level; a growing per-level cost **is** the quadratic. **NOT MEASURED**: no wall-clock benchmark of `prost_encode` against `prost`'s own encoder exists, and none was constructed for this report, because the code is dormant and benchmarking a dormant path would report a number nobody can collect (§5.9).

---

### 5.4 Throughput and CPU profile of the codec conversion

#### 5.4.1 Wall clock

**Hypothesis, stated before the measurement** (and stated in the harness's own header): *"$`2\times`$ faster at depth 6,000 and 20 % slower at depth 3 is a NET LOSS."* The conversion's value is a class change, so the acceptance criterion was **not a speed-up** but *no regression on the production-weighted mix*. The measured distribution is 95.43 % depth 2 and **nothing deeper than 6 was observed**, so the verdict cell is deliberately the shallow one, where a per-node dispatch cost would show up worst.

⚠⚠ **RETRACTED IN PART, 2026-07-30 — read the retraction after the ratio table before using any figure in this subsection.** The interval $`\pm 0.005`$ is **OVERTURNED**, the three Welch statistics are **OVERTURNED**, and the *magnitude* is **NOW UNKNOWN**. The **sign** is unaffected. The whole of §5.4.1 is retained rather than rewritten, because what the instrument printed is the evidence for what was wrong with it.

**MEASURED (f)** — `/tmp/sd_wire_bench.log`, release, core 8, 3 independent whole-bench runs $`\times`$ 60 measured repetitions after 10 warm-up. Production-weighted mix, 2,001 datums/pass, 693 B/datum.

⚠ **This paragraph previously ended *"…after 10 warm-up, interleaved A/B within each repetition."* That clause is DELETED because it was false.** The harness's private `measure()` helper ran **all 60 repetitions of one arm**, returned, and was called again for the next — so the three arms were timed in **three disjoint time windows** on a host running several concurrent builds. The claim of interleaving was in the module header too, in the same words, and it was untrue there for as long as the file existed.

| run | `derived` mean ± sd (ns) | `machine` mean ± sd (ns) | `machine_reused` mean ± sd (ns) |
|---|---:|---:|---:|
| 1 | 1,043,789 ± 24,237 | 870,283 ± 10,301 | 830,347 ± 13,786 |
| 2 | 1,030,173 ± 22,662 | 864,163 ± 34,753 | 823,346 ± 32,306 |
| 3 | 1,026,084 ± 14,119 | 862,601 ± 10,952 | 835,020 ± 11,588 |
| **between-run mean ± sd ($`n=3`$)** | **1,033,349 ± 9,259** | **865,682 ± 4,072** | **829,571 ± 5,876** |

**Overlap check.** `derived` spans $`[1{,}011{,}965,\ 1{,}068{,}026]`$ ns across all runs at $`\pm 1`$ sd; `machine` spans $`[851{,}649,\ 905{,}036]`$. **The ranges do not overlap.** The harness's own Welch test [[Welch 1947](#ref-welch1947)] reported $`t = 51.03,\ 30.99,\ 70.87`$ at $`\mathrm{df} \approx 80\text{–}111`$. ⚠ **Those three statistics are OVERTURNED** — see the retraction below. A Welch $`t`$ divides by the *within-arm* standard error, which on blocked arms omits the dominant error term entirely, so it is not a test about the world here.

| comparison | run 1 | run 2 | run 3 | disposition |
|---|---:|---:|---:|---|
| `derived` $`\rightarrow`$ `machine` (like-for-like, owned `Vec`) | 1.199 $`\times`$ | 1.192 $`\times`$ | 1.190 $`\times`$ | **faster; magnitude NOW UNKNOWN, bracketed $`1.07\times`$–$`1.19\times`$.** ⚠ The interval this cell used to state, **$`1.194 \pm 0.005\times`$**, is **OVERTURNED** |
| `derived` $`\rightarrow`$ `machine_reused` (contract change) | 1.257 $`\times`$ | 1.251 $`\times`$ | 1.229 $`\times`$ | **faster; magnitude NOW UNKNOWN, bracketed $`1.07\times`$–$`1.25\times`$.** ⚠ The interval **$`1.246 \pm 0.015\times`$** is **OVERTURNED**, for the same reason |

★ **The prediction was that the conversion would cost throughput on shallow terms, and it was wrong in the favourable direction.** ★ **That conclusion STANDS** — the sign is the one thing this instrument could resolve. What does not stand is any statement of *how much*. §5.4.2 says why the direction is what it is.

##### ⚠★★ RETRACTED 2026-07-30 — a tight interval from BLOCKED arms is not a tight measurement

**The defect.** `measure()` did not interleave. Model one repetition's time as
$`t_{X,i} = \mu_X + \delta(w_i) + \varepsilon_{X,i}`$, where $`\mu_X`$ is arm $`X`$'s true mean,
$`\delta`$ is drift belonging to the *time window* $`w_i`$ rather than to the code, and
$`\varepsilon`$ is independent noise with variance $`\sigma_\varepsilon^2`$. Then:

```math
\operatorname{Var}\big(\widehat{\Delta}_{\text{blocked}}\big) \;=\; \frac{2\sigma_\varepsilon^2}{n} \;+\; 2\sigma_\delta^2,
\qquad\qquad
\operatorname{Var}\big(\widehat{\Delta}_{\text{paired}}\big) \;=\; \frac{2\sigma_\varepsilon^2}{n}
```

★★ **The term $`2\sigma_\delta^2`$ contains no $`n`$.** The 60 retained repetitions shrink
$`\sigma_\varepsilon`$ and do **nothing** to the window term, so a blocked design reports a
*tight-looking* result whose dominant error is never estimated. And the Welch denominator is built from
the within-arm variances — $`\sigma_\varepsilon`$ alone — while the window offset sits in the numerator,
so $`|t| \to \infty`$ as $`n`$ grows for **any** non-zero offset. $`t = 70.87`$ is therefore evidence of a
*quiet window*, not of a precise effect. This is the measurement-bias class of
[[Mytkowicz 2009](#ref-mytkowicz2009)]; [[Georges 2007](#ref-georges2007)] sets out the design discipline
that avoids it.

**★★★ Why the $`\pm 0.005`$ specifically was LUCK, stated as a number rather than as a worry.** The three
runs above agree to $`1.199 / 1.190 = 1.0076`$ — **0.76 %**. The *same instrument*, the *same three-run
construction*, on the *same bench*, later reported the same weighted owned-`Vec` ratio as
$`1.261\times`$, $`1.471\times`$, $`1.154\times`$ — $`1.471 / 1.154 = 1.2747`$, **27.5 %**:

```math
\frac{27.5\,\%}{0.76\,\%} \;\approx\; 36
```

$`\Rightarrow`$ **The instrument is capable of a 36× wider three-run spread than the one it happened to
deliver here.** Three draws that land close are not a precision; they are three draws that fell in similar
windows. ⚠ **A disposition is a value, not an absence** — so the cells above read *"NOW UNKNOWN, bracketed
$`1.07\times`$–$`1.19\times`$"* rather than being blanked, and the bracket is **not a replacement
interval**: its upper end is this instrument's most favourable blocked draw and its lower end is the paired
instrument's reading, so the true value is somewhere in it and no narrower claim is available.

**What the repaired instrument says.** Genuine per-repetition interleaving with order rotation and a paired
$`t`$ [[Student 1908](#ref-student1908)], in the shared `models/benches/paired.rs`:

| instrument | blocked, three runs | spread | paired, three runs | spread |
|---|---|---|---|---|
| this bench, weighted owned `Vec` | $`1.261`$, $`1.471`$, $`1.154`$ | **27 %** at loadavg 16.7 | $`1.092`$, $`1.078`$, $`1.073`$ | **1.9 %** at loadavg **31–37** |
| the sibling `term_ops_bench` | $`1.0748`$ then $`0.9461`$ — a **verdict flip** | **13 %** at loadavg 15.7 | $`0.962`$, $`0.956`$, $`0.954`$ | **0.8 %** at loadavg 19.8–23.8 |

★ **14× less spread at roughly double the load.** A quieter machine would have narrowed both columns; only
one narrowed, which is what identifies the *defect* rather than the *host* as the dominant term.

**What is UNAFFECTED, and why — because effect size decides, not provenance.** ⚠ Do not read this
retraction as invalidating §5.4 or the report.

| figure | status | why |
|---|---|---|
| the **sign** — the machine is faster on the production mix | ★ **UNAFFECTED** | Even the least favourable blocked draw is $`> 1`$, and the paired instrument agrees at $`1.073\times`$–$`1.092\times`$. Two instruments, one conclusion. |
| the $`770\times`$ **allocation** reduction (§1, and the row below) | ★ **UNAFFECTED** | A DHAT block count is **deterministic**. It is not a wall clock and no window term enters it. |
| every **B/level** slope and $`D_{\max}`$ ceiling in this report | ★ **UNAFFECTED** | Measured by stack-pointer differencing and by binary search on depth, neither of which is a timing. **The stack-safety results — which are what this report is for — do not depend on the retracted instrument at all.** |
| the $`3.25\times`$ heap-**write** and $`2\times`$ peak-heap costs | ★ **UNAFFECTED** | DHAT, deterministic, same reason as the allocation count. |
| the $`\pm 0.005`$ and $`\pm 0.015`$ intervals | ⚠ **OVERTURNED** | Three correlated draws, not a precision. |
| the three Welch $`t`$ values | ⚠ **OVERTURNED** | An unpaired statistic on drift-contaminated arms. |
| the **magnitudes** $`1.194\times`$ and $`1.246\times`$ | ⚠ **NOW UNKNOWN**, bracketed | Sign survives; the third significant digit was never real. |

$`\Rightarrow`$ ★ **The rule this establishes:** whether a figure from a bad instrument survives is decided
by **effect size against instrument spread**, not by how the figure was obtained. A 19 % effect against a
27 % spread cannot be *quantified*; it can still be *signed*, because two independent instruments agree on
the direction. A 2 % criterion against the same spread is not a criterion at all.

**The rest of the shape space** (run 3, one representative; all cells $`\alpha = 0.01`$ significant, all ranges non-overlapping):

| shape | `derived` $`\rightarrow`$ `machine` |
|---|---:|
| depth 1 | 1.205 $`\times`$ |
| depth 2 (95.43 % of production) | 1.203 $`\times`$ |
| depth 3 | 1.241 $`\times`$ |
| depth 4 | 1.171 $`\times`$ |
| depth 6 (deepest observed in production) | 1.171 $`\times`$ |
| depth 16 | 1.294 $`\times`$ |
| depth 64 | 1.389 $`\times`$ |
| `map(64 entries)` | 1.299 $`\times`$ |
| `map(1024 entries)` | 1.311 $`\times`$ |
| `wide(4096 siblings)` | 1.531 $`\times`$ |
| depth 256 (never weighted) | 1.521 $`\times`$ |
| depth 1024 (never weighted) | 1.501 $`\times`$ |

⚠ **The `depth 64` cell is the noisiest in the file** (relative sd 4.5–14.3 % against ~1–2 % elsewhere) and its ranges *do* overlap between runs, though not between arms within a run. It is reported with that caveat rather than smoothed.

#### 5.4.2 CPU profile — where the derived path's time actually goes

**MEASURED (f)** — `perf record -e cpu-clock -F 9999 --call-graph dwarf,16384` over the bench (⚠ described here as *"the interleaved A/B bench"*; it was **blocked** — see §5.4.1), 29,436 samples, 0 lost. Full flat profile at `/tmp/sd_perf/report.flat.txt`. ★ **A flat profile is a SHARE-of-samples attribution within one arm, so the blocking defect does not reach it**: it says where an arm spends its time, not how two arms compare, and the window term cancels in a ratio taken inside a single run.

⚠ **The gross per-arm totals are *not* a valid A/B comparison** and are not presented as one: the bench runs `derived` once but `machine` **and** `machine_reused`, so the `wire_encode` bucket covers two arms. The wall clock of §5.4.1 is the comparison. What the profile *does* establish is the **internal structure of the derived arm**, which no timing can show:

| bucket within the derived arm | % of total samples |
|---|---:|
| `SizeChecker` pass — i.e. `serialized_size` | **19.40 %** |
| `Serializer` pass — i.e. `serialize_into` | 14.54 % |
| `drop_in_place::<bincode::error::ErrorKind>` | **9.23 %** |
| derived total | 43.17 % |

Two findings:

1. ★★ **The traversal the single-walk machine deletes is the *more expensive* of bincode's two** — $`19.40 / 14.54 = 1.334\times`$. The claim in `c28f4cf6` that *"the win is deleting a whole traversal, not shaving a loop"* is thereby confirmed by profile, and it explains §5.4.1's direction: a $`1.33/2.33 \approx 57\%`$ reduction in serde work comfortably absorbs the per-node dispatch the op stack adds.
2. ★ **`drop_in_place::<bincode::error::ErrorKind>` accounts for 9.23 % of the whole profile — 21 % of the derived arm's own cost — on the *success* path.** Every `serde` call returns a `Result<_, Box<ErrorKind>>` and every one of them is destroyed. The op-stack machine returns `()` from its emit steps and pays none of it. This was not a designed win and is recorded as an observation.

The machine's own hot symbols are `encode_into` (21.89 %), `<Par as WireNode>::wire_emit` (16.96 %) and `<Expr as WireNode>::wire_emit` (8.25 %) — i.e. the driver loop and the two generated emit tables, which is what a well-behaved defunctionalised walk should look like.

---

### 5.5 Family D — the deploy path

![deploy path ceilings](figures/deploy-path-ceilings.svg)

**Figure 6** — *`figures/deploy-path-ceilings.puml`*. The deploy path end-to-end, with each traversal's measured ceiling.

#### 5.5.1 ★ A refuted hypothesis, reported as a result

**Hypothesis (E86, recorded before the measurement):** the deploy path's binding constraint is `Env::get`'s un-removable clone, because that is where a deep term enters the environment.

**MEASURED (q)**, `e72dcae8`, release, explicit 2 MiB worker, real runtime, deploy driven from source:

* `@"out"!([[…[0]…]])` — no binder at all — max depth **286**;
* `for(@x <- @"c"){@"out"!(x)} | @"c"!(…)` — the COMM'd binder — max depth **283**.

★★ **The hypothesis is refuted.** The binder shape costs **three levels of headroom**, not a new ceiling. Both shapes measure the *same* 7,253 B/level (identical growth, 1,740,800 B over 16 $`\rightarrow`$ 256); they differ only in a 20,480 B **intercept**. And the arithmetic estimate of ~289 levels ($`2\ \mathrm{MiB} / 7{,}247`$) lands near the right number **for the wrong reason**.

**What was actually binding, and nobody had recorded it.** `Substitute::substitute_and_charge` took `term: &A` and opened with `self.substitute(term.clone(), …)` — so **every substitution copied its input** through `<Par as Clone>::clone`, on the **ordinary send path**, with no binder, no COMM and no environment. Established by `gdb` at the overflow: a clean 3-frame repetition

```text
<Par as Clone>::clone
  → <ExprInstance as Clone>::clone
    → <Expr as ConvertVec>::to_vec
```

under frame #103 `substitute_and_charge::<Par>`, called from `eval_send`'s data-substitution `map`, on a `spawn_detached` worker.

#### 5.5.2 ★ A withdrawn claim, reported as a result

**Claim as recorded (E89):** the gRPC ingress teardown costs **84.3 B/level**, and the ingress ceiling *depends on the spelling of the discard* — `mk_term(..).map(drop)` measuring 135.5 B/level against the literal `match … Ok(_parsed_term) => …` at 84.3, a $`1.6\times`$ spread from source form alone.

**WITHDRAWN. MEASURED (q)**, `ee1dfdad`. The 84.3 came from a **two-point min-stack ladder whose low point is clamped at the 77,824 B parse/normalise floor**, which biases the estimate downward — hazard (1) of §2.3. Four fixed-stack depth bisections resolve it:

| stack | ceiling | pairwise B/level |
|---:|---:|---:|
| 256 KiB | 2,667 | — |
| 512 KiB | 5,398 | 95.988 |
| 1 MiB | 10,859 | 96.006 |
| 2 MiB | 21,782 | 95.997 |

Least squares: **95.999 B/level at $`r^2 = 1.0000`$**, intercept 6,106 B, largest residual **21 B**. The disassembly gives the same number independently: 48 B + 48 B per level across the two frames of the `Par` $`\leftrightarrows`$ `ExprInstance` cycle.

★ **And the shape-sensitivity conclusion is superseded too.** The *identical* function `drop_in_place::<models::rhoapi::Par>` is emitted with **5 pushes and no `sub rsp` (48 B)** in casper's binary and **7 pushes (64 B)** in rholang's, and the cycles bisect to **96 vs 144 B/level** — a $`1.5\times`$ spread from **register allocation alone, with no source difference**. E89's $`1.6\times`$ between two spellings sits inside that. The conclusion "the call shape matters" was an artefact of ordinary per-build codegen variance.

★ **Corrected record: 84.3 $`\rightarrow`$ 96.0 B/level.**

#### 5.5.3 The three repairs

**(a) The ingress discard (`a09f1de2` + `3b265eb7`).** `admit_deploy` and `admit_deploy_cosigned` validated a deploy by parsing its term and throwing the result away — `Ok(_parsed_term)`, bound with a leading underscore, never read, released when the match arm ends. `Par` is prost-generated, so that release is the derived `drop_in_place::<Par>` $`\leftrightarrows`$ `drop_in_place::<ExprInstance>` cycle across the `EList.ps: Vec<Par>` edge.

⚠ **The severity was *reachability*, not depth.** Ingress was the **highest** of the deploy path's three ceilings, not the binding one:

```text
env_get_deploy (reduction)          283   ← the binding constraint
plain_deploy   (reduction)        6,831
ingress admit_deploy_cosigned    21,781
```

What made it worth fixing first is *where it sat*. Every hop was read: `DeployService/doDeploy` $`\rightarrow`$ `deploy_grpc_service_v1.rs:256` $`\rightarrow`$ `block_api.rs:477 deploy_cosigned` (**synchronous, inline on the tokio worker, no `spawn_blocking`**) $`\rightarrow`$ `dispatch.rs:66` $`\rightarrow`$ `block_admission.rs:105`. So it fired **on unauthenticated network input, on the receiving node, before the deploy was stored, before consensus, and with no `RuntimeBudget` in scope** — cost accounting could not bound it, not because the charge would be too small but because **no charge exists yet**. The inbound cap is 16 MiB, **385 $`\times`$ more headroom than the attack needed**, and the signature is trivially generated with a fresh keypair.

The repair is one owner, `validate_deploy_term`, which parses the same way and hands the term to `par_children::dismantle` — an explicit `Vec<Par>` worklist. It is a **function** rather than three copies of one discard because there are two production call sites *and a measurement probe that must stay the same shape as production or its numbers describe something else*.

★ **Nothing observable moves, for three independent and individually sufficient reasons** (**DERIVED**, `3b265eb7`): the signature is over the **source** (`DeployData::to_message`'s `term` field is the source string, so the normalised `Par` is never in the signed payload); storage is the source (admission persists `Signed<DeployData>` plus the cosigner sidecar, and **no `Par` is written**); and the term is rebuilt later anyway (the proposer re-normalises at `acceptance.rs:321`).

**(b) The metering handshake (`9082d12c`).** `InterpreterImpl::inj_attempt`'s set-initial-cost phase handed the freshly normalised `Par` to `SignedProcess::metered`, then **eleven lines later asked for it back** with `source_process().cloned()`, and dropped the original at the closing brace. Two $`\Theta(d)`$ traversals of native stack where a **move** suffices.

★ **And neither bought anything.** `RuntimeBudget::reset_from_signed_process` reads only `SignedProcess::token()`, and `token()` returns `None` on the `Signed` arm — **so the metering handshake never observes `process` at all**. The clone was a read-back of the line above it.

The repair is `SignedProcess::into_source_process`, the by-move twin: it walks the `Par(Box, Box)` spine with an explicit worklist, **pushes RIGHT before LEFT so `pop` reproduces `source_process`'s left-biased `.or_else` order exactly**, returns the first `Signed`'s `process` **by move**, and hands any further `Signed` arms to `par_children::dismantle_all`.

**(c) The metered wrappers take their term by value (`64a5d2bc`, `94dc983f`, `d2591fa1`).** The `substitute_and_charge` copy of §5.5.1 is removed by making the wrapper take ownership, which requires ownership to run all the way from the task-spawn boundary. `d2591fa1` deletes the per-branch `Par` deep clone in `eval_par` — the spawned future is `'static` so the branch term must be **owned**, but it does not have to be **copied** — by re-typing `split` to take `term_count: usize` (its entire body read `terms.len()`) and `into_iter()`-ing the terms. `94dc983f` then carries ownership through `generated_message_eval`, removing **15 deep copies**, seven named by the plan and **eight more that fell out of the same plumbing because the sibling arms had the identical shape**.

★ **And one that was worse than a clone.** `eval_match`'s loop held `(Par, Vec<MatchCase>)` as state and rebuilt the tail with `case_rem.to_vec()` on every failed case: a deep copy of every **remaining** case, once per case tried — $`\Theta(n^2)`$ `Par` clones over an $`n`$-case `match`. Draining an owning iterator removes it.

#### 5.5.4 Results, and a control that behaved exactly as predicted

**MEASURED (q)** — `64a5d2bc`, `ee1dfdad`, `9082d12c`:

| subject | before | after | factor |
|---|---:|---:|---:|
| `subst_and_charge` (release) | 2,852 B/level | **146** | 19.5 $`\times`$ |
| `subst_and_charge` (debug) | 15,872 B/level | **1,462** | 10.9 $`\times`$ |
| `inj_attempt_clone` (release) | 2,852 B/level, $`D_{\max}=729`$ | **0**, $`\geq 1{,}048{,}576`$ | class change |
| ingress teardown (release) | 96.0 B/level, ceiling 21,781 | **0**, no ceiling $`< 262{,}144`$ | class change |
| ingress teardown (debug) | 429.9 B/level, ceiling 4,503 | **0** | class change |
| **`plain_deploy`**, end-to-end, 2 MiB worker | 286 levels | **6,831** | **23.9 $`\times`$** |
| **`env_get_deploy`**, end-to-end — **the control** | 283 levels | **283** | **1.00 $`\times`$** |

★★ **The control is the reason this result is credible.** `env_get_deploy` exercises `Env::get`'s clone, which *nothing in this repair touches*. It was predicted to be unmoved and it was unmoved — *by a single level*. A report quoting only `plain_deploy` would be false: what changed is **which shape is worst**. Before, `Env::get` cost 3 levels of headroom on top of a 286-level ceiling; now it is the **sole** ceiling of its shape, 24 $`\times`$ below the other. Any deploy whose deep value arrives over a channel is **unimproved by this repair**.

⚠ **A drift found by this report. MEASURED (f)** — `/tmp/sd_deploy_ceiling.log`, fresh bisection at HEAD `8853f839`:

```text
deploy depth ceiling on a 2 MiB worker: plain_deploy 6831, env_get_deploy 274
  plain_deploy    : depth 6831 runs, depth 6832 exits ExitStatus(unix_wait_status(134))
  env_get_deploy  : depth 274 runs, depth 275 exits ExitStatus(unix_wait_status(134))
```

`plain_deploy` reproduces **exactly** (6,831). **`env_get_deploy` reads 274, not 283** — a loss of **9 levels ($`-`$ 3.2 %)** since `64a5d2bc`. The candidate causes are the four `rholang` commits landed since (`8853f839` substituting a `matches` pattern at `depth + 1`, `eaa905fe` restoring bindings on a refused disjunction, `f5fd6c34` giving each matcher attempt its own `FreeMap`, `76de7d44` `cursor_kind`), each of which adds state on the binder path — but **the attribution was not established** and is not asserted. What *is* asserted is that the recorded 283 is stale at HEAD.

⚠ **A consequence for the gate.** `stack_depth_gate.rs`'s `BUILD_DEPTH_INVENTORY` carries **283** as a transcribed constant and computes its headroom ratio from it: the fresh run prints `tightest env_get_deploy at 283 vs widest read ceiling 33 — 8× (floor 8×)`. With the true 274 the ratio is $`274/33 = 8.30`$, still above the floor — so **the gate passes, and it passes on a stale number**. The inventory is the one place in this system where a figure is transcribed rather than derived, and it has drifted, exactly as §5.7.1 predicts of any transcribed figure.

#### 5.5.5 A second correction to the record

**MEASURED (q)**, `291bc217`: the recorded **219 B/level** release figure for `par_drop` *does not reproduce*; direct bisection gives **144** (debug's 470 reproduces at 464), making the profile ratio **3.22**, not the 2.1 that had been derived from the 219. **MEASURED (f)**: `par_drop` **144** B/level release at HEAD, on the gate's own 256 $`\rightarrow`$ 4,096 ladder, and **136** on the S0 16 $`\rightarrow`$ 256 ladder. **The 219 is withdrawn.**

#### 5.5.6 How bad it was before — the severity of what was fixed

Two numbers, both **MEASURED (q)**, that the "after" column would otherwise flatter:

* `291bc217`: on a 2 MiB worker, `normalize` survives depth $`\geq 39{,}960`$ but `par_drop` **aborts at 4,453**. $`\Rightarrow`$ **A source of 4,415 bracket levels — 8,831 bytes, one TCP (transmission control protocol) segment — normalises without difficulty and then aborts the process on teardown**, via `SIGSEGV` on the guard page. Not a panic, so no `catch_unwind` sees it; `inj_attempt`'s `ParserError` arm cannot run; and `build-normalized-term` precedes `set-initial-cost`, so **no budget exists to charge**. At 8 MiB the boundary is 17,929 / 17,987, which reproduces the originally reported "16,000 ok / 32,000 aborts" exactly.
* `3b265eb7`: a **depth-21,782 deploy — 43,565 bytes of source** — sent to `doDeploy` aborted the node with `SIGABRT`, exit 134, pre-storage and pre-consensus.

⚠ **And a bound on the severity, from the same measurement**: the **wire** path was *not* affected, because prost's `RECURSION_LIMIT` caps a network-delivered term at depth 33. **A `Par` too deep to arrive over the network can be built from source text.** That sentence is the asymmetry of §6.4, first observed here.

---

### 5.6 Family F — fixes originating in `mettail-rust`

`mettail-rust` is the companion repository that lowers guest-language terms into `rhoapi::Par`. Two of its findings are in scope because the fix originates there; three more are included because they are methodological results this report depends on.

#### 5.6.1 The lowering component (`3c0c3585`)

**The defect. MEASURED (q)**: `@"OUT"!([[[…1…]]])` aborted the `rhocalc` binary with `SIGSEGV` on the guard page at nesting depth **169**.

★ **The scope was the *component*, not the self-calls.** The reproducer never recurses through a self-call: its cycle is `lower_proc ▸ CastList ▸ lower_list ▸ lower_proc`, with twelve `core::iter` adapter monomorphisations in between. A Tarjan decomposition puts **87** functions in one component. **Converting the 18 direct self-call sites would have left the reported reproducer exactly as it was.**

⚠ **An earlier attempt is the control.** M-1 split `lower_proc`'s 89 arms into per-arm frames and bought **$`3.20\times`$ in debug and $`1.07\times`$ in release** — because at `opt-level=3` LLVM was *already* overlaying the arms M-1 hoisted. **The class was untouched.** This is §5.1.4's lesson in a second costume.

**Results. MEASURED (q)**, `3c0c3585`, by direct bisection of `RLIMIT_STACK` on the probe's **main thread** — not a spawned thread with a `stack_size`, because the member runs on the main thread and `RUST_MIN_STACK` cannot reach it — debug / release B/level:

| subject | before | after |
|---|---:|---:|
| `lower_depth` | 15,132 / 2,157 | **1 / 0** |
| `lower_leak` (the lowering *alone*) | — | **0 / 1** |
| `lower_add`, `lower_par`, `lower_neg` | — | 0–1 / 0–1 |
| `lower_width` (65,536 siblings) | — | 1 / 0 |
| whole `rhocalc` binary | — / 7,277 | — / **2,567** |

★ **Anti-vacuity changed a conclusion here.** `lower_depth` first read **252 B/level**, and the obvious reading — *"the conversion is incomplete"* — was **wrong**: `ast_drop`, which lowers nothing at all, read **254**. The slope was the teardown of the AST (abstract syntax tree) itself. `lower_leak` (lower, then `mem::forget` both sides) isolates the conversion and reads **0**.

**The named residue, with owners** (**MEASURED (q)**, debug / release): `par_drop` 368 / 95 · `ast_drop` 270 / 96 · `render` 3,665 / 911 · `lower_formula` 4,094 / 978. The two `Drop`s are the derived-impl class and **are not reachable by the pushdown transform applied here: `drop_in_place` has no text to rewrite.**

⚠ **`ast_drop` is that class with a twist worth recording.** The `language!` macro *does* emit a pooled iterative `Drop`, and a pure `Proc::Add(Arc<Proc>, …)` chain is flat under it — but `Proc::CastList(Arc<List>)` $`\leftrightarrows`$ `List::ListLit(Vec<Proc>)` **alternates types**, and the worklist does not follow the hop.

#### 5.6.2 The environment-as-delta result, reproduced independently

`3c0c3585`'s `BoundEnv` rides as a **delta** into an arena whose root is borrowed, materialised once per **binder site** — because *"an owned env per work item clones a `HashMap` per level and leaves the traversal $`\Theta(d)`$ in heap anyway"*. That is §5.1.2(b) arrived at independently in a different repository, which is the strongest available evidence that it is the *right* shape for this transformation rather than a local trick.

#### 5.6.3 ★★ A retracted claim — the parser is **not** depth-independent (`1339c1e2`, then `6275b0e2`)

**Claim as published (`1339c1e2`):** `Proc::parse_via_wpda` is depth-**independent**, on a ladder at depths 16 / 32 / 64 / 128 that read **471,040 bytes at every rung — identical to the byte**.

**RETRACTED within the hour.** `assert_no_slope`, which bisects at both ends of a 4 $`\rightarrow`$ 4,096 ladder rather than sampling a narrow one, failed with

```text
ZERO-SLOPE GATE FAILED for `parse_depth`: minimum stack grew 5264 KiB between
depth = 4 (460 KiB) and depth = 4096 (5724 KiB), which is 1317 B per step.
```

**MEASURED (q)**, `6275b0e2` — widening the ladder resolves it cleanly:

| depth | min stack (B) | pairwise B/level |
|---:|---:|---:|
| 128 | 471,040 | — |
| 256 | 499,712 | 224 |
| 512 | 815,104 | 1,232 |
| 1,024 | 1,536,000 | **1,408** |
| 2,048 | 2,977,792 | **1,408** |
| 4,096 | 5,861,376 | **1,408** |

Asymptote **1,408 B/level**, stable to the byte across the last three intervals. Flat to depth ~256, linear thereafter; ⚠ **the mechanism behind the knee is not established and is deliberately not guessed at.**

★ **Why the first measurement lied.** 471,040 B is the parser's **own fixed intercept** — ~460 KiB of generated recogniser tables and driver frame. It is not a harness artefact: the cheapest subject in the same binary bisects to 98,304 B. Below depth ~256 the per-level cost is entirely **masked** by that intercept.

★★ **The rule adopted, which generalises past this subject:** *both probe points of a slope measurement must sit clear of the subject's own floor, or the derived slope is understated — here, all the way to zero.* This is hazard (1) of §2.3, and it is the dual of the hazard the same file's header already warned about. **The superseded claim is left standing in the previous commit message rather than rewritten, because the retraction is part of the record.**

#### 5.6.4 Two further methodology corrections from the same repository

**A guessed ceiling, corrected by measuring (`d72740e6`).** `lowering_theta_depth_tripwire` carried `ceiling(23_000, 6_000)`; the 6,000 was extrapolated from a `ulimit` bisection of the **whole binary** before any release gate run existed. Measured release slopes at the tripwire's own probe points: `lower_depth` **2,157**, `lower_add` **2,450**, `lower_par` **1,206** B/level $`\Rightarrow`$ the correct release ceiling is ~4,000. **The committed 6,000 was both too loose for the subjects it gates and derived from the wrong scope.**

★★ **The binary's slope is not the lowering's slope**, and recording that in advance prevented a false claim later:

| measurement | release B/level |
|---|---:|
| gate `lower_depth` (lowering alone) | 2,157 |
| gate `parse_depth` (parser alone) | 304 |
| gate `reproducer` (parse + lower + iterative teardown) | 2,128 |
| `bisect_ulimit` on `target/release/rhocalc` (whole main thread) | **7,266** |

the `reproducer` subject and the `lower_depth` subject agree to within measurement noise, so parse and lower **do not sum** — the deeper dominates. But the **binary** costs ~5,100 B/level *more* than parse + lower together, and that residue is neither: it is rendering the observation, and the derived recursive `Drop` of the deep `Par`. $`\Rightarrow`$ *"The lowering is fixed" and "`rhocalc` is depth-independent" are different claims, and only the first was made.*

**An inert remedy retired (`73c774a3`).** Every run line in a demo run-sheet carried `RUST_MIN_STACK=134217728`, and a gate asserted the prefix was **present**. **MEASURED (q)**: all thirteen committed demos run to their documented exit status with **no `RUST_MIN_STACK` set**, at the default `ulimit -s`. ★ **And the prefix was never a correct remedy here**: `RUST_MIN_STACK` is read only by `std::thread`'s spawn path, so **it cannot resize a main thread** — and the binary is `#[tokio::main] async fn main`, which is where parsing and lowering run. *A sheet that recommends an inert knob teaches a presenter to mis-diagnose, which is worse than silence.* The gate was **inverted** rather than deleted, so that restoring the prefix under presentation pressure fails the build and points at the real gate.

**A gate that was red because it did not finish (`21c51d10`).** One test bisected four residue subjects at depths 512 and 4,096; at ~20 child processes per bisection it ran ~500 s and `cargo nextest` **terminated it at its 300 s per-test cap**. *A gate that is red because it did not finish is worse than no gate: it reports failure without having measured anything, and the natural response under pressure is to delete it.* Split one-per-test: 3.3 s, 3.4 s, 171 s, 206 s.

#### 5.6.5 ★ The twelfth generated driver, and the seven numerals beside it (`ed44c429`)

##### 5.6.5.1 The defect

**MEASURED.** `macros/src/gen/native/eval.rs` emitted `Some(__b) => Some(__b.as_ref().try_eval()?)` inside the `Visit` arm for a same-category optional child. The branch `continue`s before any Reduce frame is built, so the descent ran on the **host stack in both the recursive form and the PDA form** — a $`\Theta(\text{depth})`$ path inside a driver that advertised itself as converted. A user input reaching it is any term nesting through such a field; the workspace's one instance is `optsmoke::Int::IfElse`'s `*opt(e:Int)`.

**DERIVED.** A classifier-side census of the emitter — not of the generated artefacts — finds only **four** rules workspace-wide that put anything in a term-context `*opt(...)`: one same-category, two `Vec(Proc)` (no `native_type`, so the emitter `continue`s), one wrapping `?g:Guard`. Reading the classifier rather than `target/generated/` is what makes this total: it sees grammars whose output never reaches the tree.

##### 5.6.5.2 The architecture of the repair, and why THIS shape

**Shape: representation change** (a presence flag) **plus a refusal**, not a driver conversion.

`CrossKind::OptionalSameCat` replaces the recursive child with a `bool`, because the arm needs to know only *whether* the optional was present — the child's value is already reachable through the worklist. A Reduce frame would have been the alternative and was rejected: it widens `Step` for one instance of one shape in one language.

★ **The alternatives, and why each was rejected.** *Convert the arm to a worklist frame* — costs a `Step` widening for a single site. *Leave it and record a residual* — the residual is a live overflow path, not a cap. *Refuse the whole optional-same-category shape* — too broad; the shape is legitimate when the child is not a term.

The **capture-rule refusal** is a different judgement: a capture rule whose syntax binds token text *and* carries a same-category `Term` field would be $`\Theta(\text{depth})`$ with no way to express the presence flag. ⚠ **Its blast radius is provably zero** — the refused shape **fails to compile in the baseline too** (`E0061`, wrong arity from `term_generation.rs:68` / `random_generation.rs:101`). It never compiled. The refusal replaces an incomprehensible generated-code error with a named one.

**Invariant that keeps the stacks in step:** the presence flag is consumed in the same arm that would have consumed the recursive result, so the worklist's push/pop balance is unchanged.

##### 5.6.5.3 How the fix was made

VERBATIM, the discriminator (`macros/src/gen/native/eval.rs`):

```rust
fn capture_term_field_is_same_category(ty: &TypeExpr, category: &syn::Ident) -> bool {
    matches!(ty, TypeExpr::Base(base) if base == category)
}
```

The `CrossKind` variant and its three arms (`quote!{ #n: bool }`, `quote!{}`, `None`) are ELIDED; the refusal emits `compile_error!` via `quote_spanned!` on the rule label's span.

##### 5.6.5.4 Results

| metric | before | after | provenance |
|---|---|---|---|
| B/level, release | host recursion, unbounded | **0** | gate `ast_try_eval`, `ast_try_eval_cast` |
| B/level, debug | host recursion, unbounded | **0** | same |
| max depth, 2 MiB worker | overflow | no overflow | RED-then-GREEN, below |
| wall clock | **NOT MEASURED — no throughput claim is made** | | |
| heap: peak / blocks | **n/a — not a ser/de conversion** | | |
| where the allocations moved | nowhere; a `bool` replaces a recursive call | | |

**Byte-identity control CLEAN**: two detached worktrees, empty `target/` in both so `write_if_changed` could skip nothing, 2,281 files × 54 languages each — **exactly one generated file differs**, the intended target.

##### 5.6.5.5 What it cost

Throughput: unmeasured, and no claim is made. Allocation: none. Complexity: one enum variant and one refusal arm. Intercept: unchanged. ★ **A stack-safety fix that is slower is still correct — here the number simply does not exist, and saying so is the point.**

##### 5.6.5.6 What is still recursive

⚠⚠ **The cast lattice, and its bound is RHOLANG-SCOPED AND MEASURED — not a property of the generator.** Rholang's twelve `try_eval` sites are all cast arms, so its residue is bounded at five host frames (`BigRat ▸ BigInt ▸ Int ▸ UInt32 ▸ Bool`) for a term of any depth. **Workspace-wide the same census finds 63 eager non-cast sites** — `calculator` 59, `ledtest` 4, `optsmoke` 1 — including two live **cross-category** cycles (`Int::BoolToInt` ⇄ `Bool::EqInt`; `Num::PredToNum` ⇄ `Pred::EqNum`). The generator-wide repair is a category-dependency **graph refusing on any cycle**, and ⚠ it must be built over the **post-auto-injection** rule set: `Num::PredToNum` is synthesised by `ast/src/auto_inject.rs:321` and appears in no grammar source.

##### 5.6.5.7 Anti-vacuity

**The checker must reject the old emitter, and it was shown doing so.** The new test run against the pre-change emitter aborts with `fatal runtime error: stack overflow`, exit **101**. Against the new emitter: 18/18.

★ **A second anti-vacuity result, on the register itself.** `flat_generated_drivers_are_depth_independent` was a hand-written 26-name array beside a 33-row table that already knew the answer; `ast_try_eval` and `ast_try_eval_cast` reached the table and not the array, so both were held to an **8× looser bar** (≈32 vs ≈4 B/level) with no slope printed. The array is **deleted and derived**. ⚠ The predicate is *"the shape asserts depth-independence"*, **not** `Shape::Flat` — `FlatAndItsEqFreeTwinAgrees` is a flat assertion carrying an extra obligation, so matching `Shape::Flat` alone would have silently **dropped** `ast_subst` and `ast_normalize`: a narrowing disguised as a derivation. Measured: 30 + 2 = **32** checked against the array's 26; six gained, none lost. Its floor is derived from `MIN_DRIVER_SUBJECTS` minus the sloped rows, and its message prints count **and** membership.

★ Six further stale numerals in the same file were the same failure — *a transcribed count beside a table that can compute it* — and are repaired as one mechanism rather than six edits. Superseded readings are **annotated, never overwritten**.

#### 5.6.6 ★★ #174 attributed to `models`' `impl Hash for Par` (`3276c1ee`)

##### 5.6.6.1 The defect

#174 stood as *"hash-keyed collection literals cost 11.0× a list literal, and the figure matches no driver measured in isolation."* **Both halves were wrong**, and the second was the clue: it matched no driver because **it is not a driver**.

⚠ **Both filed figures are WITHDRAWN.** Re-measured on this build, on the very ladder the old numbers were taken on ($`16 \rightarrow 1{,}024`$): `map_pair_lower` **10,491 → 227** B/level (a 46× reduction) and `list_pair_lower` **950 → 0**. #162 and #189 converted the drivers stacked on top of the hash. ★ **A ratio against a control that now reads zero is not a number** — the 11.0× is withdrawn, not restated. The superseded values are kept here, named as superseded, per [Appendix G.4](#appendix-g--keeping-this-document-current) rule 2.

**It was never a parse-phase cost.** `list_pair_parse` / `map_pair_parse` / `set_pair_parse` read **0 / 1 / 0** debug and **−1 / −1 / 1** release. The whole residue is in the LOWER phase.

##### 5.6.6.2 The architecture of the repair, and why THIS shape

**Shape: measurement, not conversion.** The deliverable is an *attribution* plus a ceiling, because the traversal is not mettail's to convert — it is `models`'. DERIVED, following the one structural difference between an `EList` and an `EMap`/`ESet`:

```text
rholang_ast.rs:2460 new_emap_par → utils.rs:715 new_emap_expr
  → ParMapTypeMapper::par_map_to_emap(ParMap::new(…))
  → par_map.rs:18 ParMap::new → sorted_par_map.rs:30 SortedParMap::create_from_vec
        let map: HashMap<Par, Par> = vec.into_iter().collect();
  → models/src/lib.rs:284  impl Hash for Par     ← HAND-WRITTEN, HOST-RECURSIVE
```

`EList` takes the other branch — `EListBody(EList { ps: Vec<Par>, … })`, a plain vector: no hash, no sort, no `Ord`.

★ **The alternatives, and why each was rejected.** *Apportion the measured total across candidate sub-traversals* — refused; [§5.7](#57-family-e--the-instrument-and-what-it-caught-in-itself) records apportionment as the move that manufactures false zeros. *Convert the lowering* — wrong subject; the lowering is already flat. *Leave it unattributed* — it was, for as long as the figure stood.

##### 5.6.6.3 How the fix was made

Four probe subjects and a `lower_depth` control were added, plus two ceilinged gate rows and one **subtraction** assertion. ELIDED.

##### 5.6.6.4 Results

Discriminating window $`512 \rightarrow 4{,}096`$, where the parser's depth-independent ~483 KB floor no longer compresses the slope. B/level, debug / release:

| subject | what it runs | debug | release | provenance |
|---|---|---:|---:|---|
| `list_pair_lower` | parse + lower, **no hash** (the control) | 0 | −1 | gate |
| `par_hash` | `lower_depth` + `Hash for Par`, alone | **625** | **113** | gate |
| `par_hashmap` | the `HashMap<Par,Par>` collect | **636** | **113** | gate |
| `map_pair_lower` | the original #174 rung | 597 | 144 | gate |
| `set_pair_lower` | the #174 rung, plus the sort | 572 | 144 | gate |
| `lower_depth` | the identical pipeline, hash removed | 1 | 0 | gate |

Four subjects that share **only** that impl agree inside **±5.3 %** in debug.
wall clock: **NOT MEASURED — no throughput claim.** heap: **NOT MEASURED — n/a, no allocation moved.**

##### 5.6.6.5 What it cost

Nothing in production: the commit adds probe subjects and assertions only.

##### 5.6.6.6 What is still recursive

⛔ **`impl Hash for Par` and `impl PartialEq for Par` — see `SS-Y2`. Owner: `models`.** They are **consensus-adjacent**: `SortedParMap` feeds the canonical sort that `cost_accounting/sig.rs` signs. Same class as `par_drop` — an impl in `models`, not a `macros/src/gen/` traversal, which is exactly why no MeTTaIL driver measured in isolation ever matched the figure.

##### 5.6.6.7 Anti-vacuity

★★ **A SUBTRACTION control, not merely an invariant one.** `par_hash_excess_over_the_unhashed_pipeline_is_the_whole_slope` asserts `lower_depth` stays flat, so the two ceilinged rows **cannot go on passing while their attribution quietly becomes false**. Measured debug $`512 \rightarrow 4{,}096`$: `par_hash` 339,968 → 2,580,480 against `lower_depth` flat at ~73,728 ⇒ the excess **is** the whole slope, and it is the hash's.

⚠ `par_hash` and `par_hashmap` are kept as **two** rows rather than folded into one, because **the pair is the attribution**: `par_hash` runs the hash alone, `par_hashmap` runs it plus `Eq for Par` on collision. Their agreement (625 vs 636 debug; 113 vs 113 release) is what says the collect adds nothing of its own. **If they diverge, the `Eq` half has started to matter and the attribution needs revisiting.** This is [§8.6](#86--the-open-residual-register--what-this-report-does-not-establish)'s *"an invariant control is not sufficient"* satisfied in code.

#### 5.6.7 `SS-E1` — 3b's prerequisite instrument, and the two checks that were blind

**The defect.** Phase 3b converts the three self-contained sorter arms. Sorting is *order-defining*, so the usual "evaluation order is unobservable" argument does not apply, and `SortedParMap` feeds the canonical sort that `cost_accounting/sig.rs` signs. The epic therefore required an explicit identical-total-order argument **before** any conversion. It did not exist, and — measured here — **neither existing check could have gated the conversion.**

**Architecture, and why this shape.** The order is established in two places, not one. `combine_eset` never sorts: it maps elements through `sort_match` in the iteration order of `par_set.ps.sorted_pars`, `split_scored_terms` preserves that order into both halves, `SortedParHashSet::create_from_vec` then establishes the **term** order, and the score `Tree` keeps the **input** order. ⇒ Terms and scores are ordered by two different permutations.

Three obligations follow, and one is already discharged:

| # | obligation | status |
|---|---|---|
| **O1** | the score `Tree` must chain in *input* order, which a LIFO stack does not preserve | ★ **already mechanised** — `SortNode.idx`/`IxKont.idx` carry the source index and `SortTraversal::combine` runs `assert_children_are_in_source_order` **before any pop** |
| **O2** | the container's constructor is part of the canonical form; sorting elements directly is a second opinion | human obligation |
| **O3** | `sort_key_value_pair` keeps **only** the key's score and discards the value's | human obligation |

★ **Rejected alternative — a new `NodeKind` variant.** `NodeKind` has twelve variants and none is `ESet`/`EMap`/`EPathMap`, which suggested one was needed. It is not: `descend_expr` is already generic (children via `expr_child_pars`, pushed as `NodeKind::Par` under the existing `ExprK` kont). Recorded in `08e876fd`, superseding the earlier reading.

⛔ **Rejected alternative — using the existing checks as the gate.** Both are blind, for opposite reasons, and the composition is exactly 3b's target:

| check | sees | blind to |
|---|---|---|
| `sort_recursive.rs` (frozen oracle) | traversal shape | ⛔ the arms — *"the oracle shares `sort_combine` with the driver… cannot catch an error transcribed into the shared table itself"* (its own header). **These three arms are that shared table.** |
| `sorter_canonical_golden.rs` | the arms, incl. the **score** column | ⛔ depth $`\geq 2`$ — every collection in it was depth-1 and scalar-only, and its `EMap` was **monotone** |

**How the instrument was built.** `ad468163` adds three depth-$`\geq 2`$ rows — a set inside a map inside a set (the map nesting on *both* sides, so key and value descent are each exercised), an **anti-monotone** map (`3 → 90`, `9 → 30`), and a pathmap over a set — captured from the **pre-conversion** implementation.

**Results, with provenance.**

| quantity | value | provenance |
|---|---|---|
| corpus size, before → after | 114 → 120 entries | gate output, shown **RED before blessing** |
| golden fixture diff | **6 insertions, 0 deletions** | `git diff --numstat`; ⇒ every pre-existing canonical form byte-identical |
| anti-monotone row's score render | `(i9 i-1 (i999 (i2 i3) i0) (i999 (i2 i9) i0) i0)` | the fixture; chains **only** key scores ⇒ O3's defect would add atoms to this line |
| nesting depth reached | 3 levels (`i8` → `i9` → `i8`) | the fixture's set row |

**What it cost.** One example binary and three fixture rows. ⌀ on every runtime axis — nothing in `models/src` changed.

**What is still recursive.** All three arms. `SS-E1` is an instrument, **not** a conversion, and deliberately carries `class change: no`; per this report's own rule a traversal enters §0 as a class change *only by being converted*.

**Anti-vacuity.** The golden was shown RED (120 vs 114) before blessing; the register exemption was shown RED by perturbing its `reason`, which failed two clauses naming `UntypedExemption { commit: "5a744c66" }`. ⚠ Capture order is load-bearing and not recoverable: blessing **after** a conversion would pin whatever that conversion produced.

#### 5.6.8 `SS-Y3` — the collection arms re-score every element THREE times per level

**The defect.** `Ordering::sort_pars` (`ordering.rs:13-20`) is not a sort. It calls `ParSortMatcher::sort_match` on **every element** and returns the *sorted terms*, not its inputs. `combine_eset` reaches it three times for the same elements:

1. `eset_to_par_set(eset.clone())` → `ParSet::new` → `SortedParHashSet::create_from_vec` → `sort_pars` → `sort_match` per element;
2. `par_set.ps.sorted_pars.iter().map(ParSortMatcher::sort_match)` — again;
3. `create_from_vec(element_terms)` → `sort_pars` → a third time.

Each of those descends into that element's own collections, where the same three passes recur. ⇒ the re-entry is **multiplicative** in nesting depth. `combine_emap`/`combine_epathmap` have the same shape.

**Method.** `models/examples/sort_collection_reentry_probe.rs` builds $`\{\{\{\ldots\{0\}\ldots\}\}\}`$ — `depth` nested **single-element** `ESet`s — and calls `sort_match` on it exactly once. Under `callgrind`, total Ir is an exact deterministic function of depth with no timer involved. ⚠ One element per level is deliberate: width would confound the question, which is how many times a *single* element is re-scored as a function of how deep it sits.

★ **Invariant control.** `--control` builds the same shape from nested `EList`s. `EListBody` is **not** one of the three self-contained arms — its children go through `expr_child_pars` like any other node — so its cost must be linear.

**Results, with provenance.** Ir via `valgrind --tool=callgrind`; baseline is the control at $`d{=}1`$ (461,526 Ir of process startup).

| depth | subject Ir | control Ir | control ratio | subject $`-`$ baseline | **subject ratio** |
|---:|---:|---:|---:|---:|---:|
| 1 | 501,371 | 461,526 | — | 39,845 | — |
| 2 | 668,922 | 478,061 | 1.03 | 207,396 | 5.205 |
| 3 | 1,163,222 | 493,935 | 1.03 | 701,696 | 3.383 |
| 4 | 2,636,296 | 510,249 | 1.03 | 2,174,770 | 3.099 |
| 5 | 7,030,393 | 526,476 | 1.03 | 6,568,867 | 3.020 |
| 6 | 20,279,660 | 542,322 | 1.03 | 19,818,134 | **3.016** |

⇒ the ratio converges to **3.0** — one factor per pass, the three passes compounding per level — while the control holds flat at **1.03×** across the whole ladder. Fitted: $`\Theta(3^d)`$.

Native wall clock (no valgrind) tracks it, each $`+2`$ levels multiplying cost by $`\approx 9`$:

| depth | wall clock | ratio |
|---:|---:|---:|
| 10 | 0.16 s | — |
| 12 | 1.51 s | 9.4 |
| 14 | 13.63 s | 9.0 |
| 16 | **> 120 s** (killed) | — |

**What it cost.** ⌀ — nothing was changed; this row measures existing behaviour.

**What is still recursive.** All three arms, and the defect is **open**.

**Anti-vacuity.** The control is the load-bearing part: a harness artefact would move both arms of the ladder, and the control's ratio is flat to within $`0.03`$ at every rung while the subject's climbs from $`1.33`$ to $`2.88`$ raw. The probe also consumes its result so the subject cannot be dead-code-eliminated.

⚠ **Scope of the claim.** The exponent is measured on a **single-element-per-level** chain. Width is a separate axis and is **NOT MEASURED** here; a wider collection multiplies the per-level factor and the composed figure is unknown. ⇒ Do not quote a cost for a real term from this table.

★ **Consequence for 3b.** `sorted_pars` holds `sort_match`ed terms — **normalized values, not the message's elements** — so no message-borrowed `&'t Par` corresponds to a sorted element **in `sorted_pars` order**. That much stands.

> ⛔ **SUPERSEDED, same day, by a design review — recorded here rather than rewritten, per [Appendix G](#appendix-g--the-maintenance-contract)'s rule 2.**
>
> This section originally concluded: *"⇒ **3b needs the same owning driver that §3d's approved route (A) builds**, and the two should be built together rather than twice."* **That inference is wrong, and the refutation is already in the same file.**
>
> `combine_ezipper` (`sort_combine.rs:1150-1182`) **is already the converted form of exactly this shape**, landed and green: `expr_child_pars`'s `EZipperBody` arm pushes `zipper.pathmap.ps().iter()` — message-borrowed `&'t Par` in **wire** order — and `combine_ezipper` then hands `element_terms` to `EPathMap::new`, so the container constructor still establishes the order (O2 honoured) and each element is scored **exactly once**.
>
> ⇒ The missing step was this: **you do not have to push in `sorted_pars` order.** Push in wire order and let the *combine* apply the permutation, because by then it holds every child's `ScoredTerm`. The reordering is legal by a property already asserted at HEAD — `models/tests/scored_term_sort_test.rs:339`, `the_score_and_the_canonical_term_agree_with_each_other`.
>
> ⇒ **No owning driver, no arena, no relaxation of `Node<'t>: Copy` for 3b.** §3d still takes route (A); the two are **independent**, with one ordering coupling (§3d's repair of `dismantle`'s pathmap arms touches the same payload 3b-1 touches from the sorter side, so it lands first).
>
> ⚠ The superseded inference was carried for the length of one session and reached both this report and `sort_combine.rs`'s O4 block. It is corrected in both. ★ The lesson is the one [§1.2](#12-why-a-register-and-not-a-narrative) already argues: *the design usually already exists* — `combine_ezipper` had been the worked precedent for three hundred lines above the arm the whole time.

⚠ **A second correction from the same review, and this one weakens a claim rather than a plan.** `the_score_and_the_canonical_term_agree_with_each_other` asserts an **iff**, and it may be false: `combine_emap` chains only `sorted_key.score`, and nothing else in an `EMap`'s score tree depends on the values, so `{3 → 30}` and `{3 → 90}` are plausibly **distinct canonical terms with identical score trees**. If that witness holds, then `SortedParHashSet`'s `HashSet<Par>` iteration order (`sorted_par_hash_set.rs:22-24`) reaches `par_set_to_eset`'s emitted `ps` whenever two distinct elements tie on score — i.e. **the canonical form of such a term is process-dependent at HEAD**, a live consensus nondeterminism on the path `cost_accounting/sig.rs` signs. ⌀ **NOT YET MEASURED** — the deciding witness is a twenty-line hand-built test, and it is the first thing run before any arm is converted. It would enter this register as `SS-Y4`.

#### 5.6.9 `SS-Y4` — sibling order is not a total function of the term

**The defect.** Siblings are ordered by score. The score is **not injective on canonical terms**, and `ScoredTerm::sort_vec` is a **stable** sort — so where two distinct terms tie, their relative order is inherited from whatever fed the input vector. Two independent things feed it, and both are defects.

| site | input order | consequence |
|---|---|---|
| `SortedParHashSet::create_from_vec` (`sorted_par_hash_set.rs:22-24`) | `HashSet<Par>` iteration — `RandomState`, seeded **per process** | canonical form is a **coin flip** |
| `combine_par` (`sort_combine.rs:447-455`) | the message's own **field order** | ⛔ **deterministic**, and two spellings of one process sign differently |

**Method.** `models/examples/score_tie_witness.rs`, pinned as a test in `6bdd6ad7`.

**Results, with provenance.**

| claim | measurement |
|---|---|
| the score does not separate `{3→30}` from `{3→90}` | score trees byte-identical — both $`(999\ (9\ {-1}\ (999\ (2\ 3)\ 0)\ 0)\ 0)`$; the key `3` appears, the values appear nowhere |
| seeded nondeterminism | 40 independent processes, identical binary and term ⇒ **20 / 20** split across two byte strings |
| ⛔ deterministic fork | `{3:30} \| {3:90}` → `2a11ba010e…3c2a12ba010f…b401`; `{3:90} \| {3:30}` → `2a12ba010f…b4012a11ba010e…3c`. **Identical across runs**, different from each other |

⇒ **These are two different faults.** The seeded one moves bytes that are currently **undefined**; the deterministic one moves bytes that **are defined today**, since `|` is commutative and the two spellings denote one process. `permutation_collapse_survives_nesting` already asserts the property the second violates, and both are reachable from an ordinary deploy — `@"c"!({3:30} | {3:90})`.

**Why nothing caught it.** `sorter_canonical_golden.rs:88-101` uses pairwise-**distinct** scores *by construction*, saying so ("otherwise it would flake"); the frozen oracle shares `sort_combine` with the driver; and the one test that should have caught it asserted an **iff whose reverse is false**, passing on sampling luck. ⇒ The corpus was chosen to exclude the input class that breaks the property — the same shape [§5.7.3](#573-the-harness-prerequisite-that-was-totally-vacuous) records for the three tests that test replaced.

**How the fix was made.** ⌀ **NOT YET LANDED.** Route **β-total** is ruled — order siblings by $`(\text{score},\ \text{the bytes the element emits})`$ — but three questions gate it: β-total vs. β-narrow, whether an activation height is needed, and the `unverified_budget`. ★ Rejected alternatives are recorded now rather than after: **γ** (make the score injective) is a *complete-the-list* repair over at least three lossy paths (`EMap` values, `EZipper` cursor, `ReceiveBind.free_count`) and the list is not derivable; **δ** (drop the `HashSet`) is actively harmful **first**, because it greens the cross-process gate while leaving the permutation fork live — [CBR-L12](#cbr-l12)'s recorded ordering hazard, inverted.

**What it cost.** ⌀ — nothing changed; this row measures and pins existing behaviour.

**What is still recursive.** n/a — this is an ordering fault, not a depth fault.

**Anti-vacuity.** The pinned assertions are written in **current-state polarity**: they assert the *fault*, because the fault is what is true at the commit that pins them. The `assert_ne!` on the permutation pair becomes `assert_eq!` in the same commit as the repair, so that diff carries its own RED-to-GREEN evidence, and the failure message says not to delete the test to make the suite green. ⚠ The witness also carries a **vacuity assertion**: if `{3→30}` and `{3→90}` ever reach the same canonical term, it fails saying so rather than passing silently.

---

### 5.7 Family E — the instrument, and what it caught in itself

The measurements above are only admissible because the instrument is itself under test. Eight results from the instrument are load-bearing for this report.

#### 5.7.1 The register became DERIVED, because every transcription drifted

**MEASURED (q)**, `291bc217`: the converted list existed in **four places** and every copy had drifted — §11.4 named 7 subjects, §12.6 and §12.9 named 13, the gate carried 17, and §12.9 claimed two subjects were "commented out" when they were not. *Both prose copies had gone stale within the hour of being reconciled, twice.*

The direction of truth is now declared and **checked**: the gate's constants are the source, the audit carries one generated block, and `the_audit_agrees_with_the_gate` **fails at the commit that separates them**. The register cannot lie about itself either — `theta_depth_tripwire` **records** every subject it drives and refuses to finish unless that set is *exactly* `TRIPWIRE_DEPTH`.

★ **It was shown red four times before green**, and the fourth is the interesting one: *in the gate but not the audit `[normalize_drop, par_drop]`; in the audit but not the gate `[drop, pretty]`.* **Both counts were 9, so a count check would have missed it entirely.**

★ **A naming defect that hid a subject.** `drop_in_place::<Par>` had been a gate subject since the gate was written — under the name **`drop`**, which says *which operation* and never says *on what*. `mettail-rust`'s twin gate calls the identical traversal **`par_drop`**, so a cross-repository search for `par_drop` found the twin and **missed this one**. One name for one traversal, in both repositories.

★ **And the gate's own headline test was fixture-dependent.** `the_577_byte_reproducer_is_a_deploy_and_not_a_node_abort` certified source depth 100,000 **only because its fixture ended in `par_children::dismantle`**. No production caller does that. *The distance between "100,000 is fine" and "4,415 aborts the node" was one call in a fixture.*

#### 5.7.2 The canonical child-slot table

Four independent enumerations of the 36-variant `ExprInstance` schema already existed — substitute, the sorter family, the pretty printer, and a `collect_nodes` in a test — and **each worklist driver would have added another**. `models/src/rust/rholang/par_children.rs` makes drift a **compile error**: every match lists every variant with no `_` arm. It provides the structural children by reference, a by-move twin kept in step by `move_and_borrow_tables_agree`, the generalised iterative `dismantle`/`dismantle_all`, and `substitute_descends_into` — a *checkable record* that `EPathmapBody` and `EZipperBody` are **not** descended into by substitution today, because reproducing that verbatim is a requirement (descending would change substituted bytes, hence **signed** bytes), not an oversight.

#### 5.7.3 The harness prerequisite that was totally vacuous

★★ **MEASURED (q)**, `550b967a`: `generate_par` sized every collection with an **exclusive `0..1`**. proptest's `SizeRange::end_incl()` is `end - 1`, so `0..1` means **exactly zero elements, always**. `generate_par(d)` therefore produced, for every `d`, one of only **two** values — the empty `Par`, and the empty `Par` with `connective_used = true` — and `generate_send`/`_receive`/`_new`/`_match`/`_bundle`/`_connective` were **never invoked at all**. Everything driven by these generators was quantifying over a two-element set, **including the sorter's only property tests**.

⚠ **A finding that follows**: the sorter is a **normaliser** and is therefore **not injective**. Directly measured — `Par{exprs:[GInt 1, GInt 2]}` and `Par{exprs:[GInt 2, GInt 1]}` are `!=` yet sort to equal terms **and equal scores**. Three tests assert an *iff* that is consequently false; they pass only because two independent draws are unlikely to be permutations of each other. **They must not be relied on to gate the sorter's own conversion.**

#### 5.7.4 The checkers were given subjects they must reject (`dd0ba13f`)

Until this commit, **every assertion in the gate was a rejecter and none had ever been shown to refuse anything**. `theta_width_tripwire`'s *entire body* was a `println!` — and a `println!` cannot fail.

The prerequisite was a refactor: `assert_no_slope` and `assert_slope_below` welded their verdict to their measurement, so a verdict could only be run on data the gate could still produce — **and every real $`\Theta(d)`$ traversal had been converted**. Splitting them into pure `zero_slope_verdict` / `slope_below_verdict` over a `Ladder` observation is what made the rest possible.

| checker | `synthetic_sloped` (111 B/level) | `synthetic_flat` (0) |
|---|---|---|
| fixed-stack half (1 MiB) | **ACCEPTS** ⚠ | accepts |
| `zero_slope_verdict` | rejects ✓ | accepts ✓ |
| `slope_below_verdict` | rejects ✓ | accepts ✓ |

★ **The first row is not a defect — it is the measurement the gate had described in prose and never run**: a $`\Theta(d)`$ subject **passes** the fixed-stack half at depths 4..256, which is precisely why the zero-slope half exists.

Verified red by four controlled mutations, all reverted, each failing at a *named* assertion.

⚠ **One unexplained transient is recorded as unexplained**: a single early `theta_depth_tripwire` failure, never reproduced across 12 consecutive passes including under a concurrent release build, with every ladder bit-stable. The message was not captured. What *was* found is that the leg had a **zero-margin comparison** — two independent bisections required to land in the same 4 KiB bucket — and that is now bounded explicitly. *A guard whose margin is one bucket is a guard waiting to flake for a reason that is not a regression.*

#### 5.7.5 The trampoline-twin check stopped counting lines (`5dc1aad7`)

The check that the recursive twin is *"a rewrite, not a copy"* pinned `(name, pre lines, twin lines)` and read the **pre** column out of git and the **twin** column out of the **working tree**. A frozen blob cannot move; the live file moves whenever anyone edits it. An unrelated 1,057-line change to `reduce.rs` turned `11, 44, 167, 20, 28, 28` into `17, 47, 226, 24, 32, 32` and the test failed **having found nothing wrong with the twin**.

⚠ **The repair is not to update the numbers.** *A count is a proxy: it is not the property the test's name asserts, it merely correlated with it once. Updating it buys one refactor's worth of silence and costs the next reader the same afternoon.* (The numbers were verified correct at HEAD, so "just bump it" was available and was **declined on purpose**.)

The live claim is now checked structurally, three ways, none reading a line number: **token-for-token non-identity** per function (strictly stronger than byte identity — a twin re-copied and then reformatted is byte-different and token-identical); **the sharing, positively** — the pre-trampoline `reduce.rs` contains the token `combine_` **zero** times, while the twin calls **18** helpers every one of which is also called from **outside** the twin family; and **the lifting, as an inequality on tokens** — `eval_expr_to_expr` was **8,737** tokens and its twin is **2,320** ($`3.77\times`$), floored at $`3\times`$.

Both directions are demonstrated permanently in-suite: reformatting the twin leaves the verdict **green** (the case the line count failed), and replacing it with a renamed copy of the pre-trampoline body turns it **red on all three claims**, asserted individually so no single check carries the result.

#### 5.7.6 A control calibrated in the wrong profile (`0a7be7f4`)

`theta_depth_tripwire` was **red in release and green in debug**, at the leg demanding that the destructor control clear $`8 \times`$ the zero-slope tolerance.

★ **Not a regression: the two controls never shared a slope.** What a ladder *produces* is $`\text{slope} \times \text{span}`$, and the destructor control had borrowed the *function* control's span:

| control | debug | release |
|---|---:|---:|
| `synthetic_recurse` (a function) | 159 B/level | **111 B/level** |
| `DropChain`'s glue (a destructor) | 95 B/level | **32 B/level** |

At 32 B/level the destructor delivers 126,976 B over a 4 $`\rightarrow`$ 4,096 span — short of the 131,072 B bar by **exactly one 4,096 B bisection bucket, $`0.97\times`$**. Debug delivered $`2.97\times`$, which is why the leg had been calibrated there and shipped green.

★ **The gap is structural and could not be closed by enlarging the ballast.** `synthetic_recurse` observes its ballast with `black_box` **after** the recursive call, so the array is live across it and must hold a frame slot; `drop_in_place::<DropChain>` **never reads the ballast at all** (`[u8; N]` has no destructor), so at `-O2` the frame carries the tail pointer and the saved registers and nothing else — **32 B whatever $`N`$ is**.

So the **calibration** was fixed and the bar untouched: `DESTRUCTOR_CONTROL_HI = 16_384` restores parity of evidence ($`3.97\times`$ against the function control's $`3.47\times`$). ★ Lengthening a ladder raises every growth reading, so the hazard introduced is that the clause becomes satisfiable by **any** subject — the leg therefore runs the same clause on the **iterative twin over the same 16,384-rung ladder** and requires `false`, watched red before being trusted. **MEASURED (f)**:

```text
synthetic DESTRUCTOR (depth): recursive 31 B/level (12 KiB -> 520 KiB over 4 -> 16384), iterative 0 B/level -- checkers separate them
```

#### 5.7.7 The gate had never been run in release (`9ab6b0eb`)

Doing so produced the more interesting result:

| subject | debug B/level | release B/level |
|---|---:|---:|
| `substitute` | 195,754 | 27,136 |
| `sort` | 78,592 | 6,485 |
| `clone` | 15,872 | 2,852 |
| `encode` | 1,932 | 302 |
| `drop` | 464 | 144 |
| `reported_reproducer_…` | **RED at depth 10** | **PASSES at depth 70** |

★ **The reproducer fails in debug and passes in release. The class is present in both; only the constant decides which profile notices.** A gate validated in one profile would have reported success here — which is precisely why the real assertion asserts a **shape** and never a byte count.

The same commit settled the `RUST_MIN_STACK=134217728` workaround question **with data rather than caution**: measured depth ceilings, post-leg-1, bisecting depth at fixed stack — 2 MiB: 9 / 75 · 8 MiB: 41 / 307 · 128 MiB: 684 / >4,095 (debug / release). Derived and measured agree exactly: $`\lfloor (2{,}097{,}152 - 34{,}905)/27{,}179 \rfloor = 75`$.

#### 5.7.8 The read-ceiling registry, and a fifth site it can detect

`rholang/tests/par_read_ceiling_site_registry.rs` **scans production** for every bounded read of a `Par` and requires each to declare what feeds it: **22 sites, 11 file/reader pairs, 6 asymmetric**. It is a *scan* rather than a list precisely so that a fifth instance cannot be added without someone deciding what it is.

⚠ **Its exclusion rule is where the test could have quietly died.** The first rule tried was *"truncate each file at its first `#[cfg(test)]` module"*. `reduce.rs` has such a module at **line 225 of 10,053**, so that rule discarded **97 %** of the largest interpreter file — including all five of its `decode_trie_path` sites — and *the scan came back clean and wrong*. A calibration test now pins both directions.

---

### 5.8 The rejected candidate

**`cf35ab53` — "six `PartialEq` impls stop being exhaustive by fiat" — is NOT a stack-safety fix, and is excluded.**

**DERIVED**, from the commit and from `models/src/lib.rs`. The defect is that `_ => false` makes a `match` exhaustive **to the compiler**, so a 37th `ExprInstance` variant would compile, fall through to `false`, and compare **unequal to itself** — while `#[derive(Eq, Ord, PartialOrd)]` sits over the hand-written `PartialEq`, asserting $`x = x`$ for exactly the code that would stop honouring it. Live consumers are `HashSet<Par>` and `impl Eq for EPathMap`. The repair is a per-variant *residue* arm below the same-variant arms.

That is a **reflexivity and hashing correctness** defect. It has no depth axis, no `B/level`, no recursion, and no interaction with the stack. It belongs to a different family and is documented in its own commit. Including it here would have diluted the report's subject.

★ It is nonetheless **methodologically adjacent** in one respect worth a sentence: its sibling enumeration found **seven** catch-alls across all 1,089 `.rs` files, fixed six, and **allowlisted the seventh with its reason** (`DispatcherMessage`'s catch-all *delegates* rather than answering `false`, so a new variant keeps $`x = x`$ true by construction). That is the same discipline §5.7 applies to gate subjects, and it is why the exclusion is stated rather than silent.

### 5.9 Measurements that could not be obtained

Seven, each with its reason. None is estimated.

| # | what | why not |
|---|---|---|
| 1 | **`spawn_detached` per-spawn overhead** (`catch_unwind`, the atomic, the `Arc` clone) | No isolated micro-benchmark exists in the tree and none was constructed. The end-to-end CPU figure of §5.2.2 includes it but cannot separate it. |
| 2 | **`prost_encode` wall-clock vs `prost`'s own encoder** | The code is **dormant** (§5.3.5); a number from a path production does not execute would be misleading. The $`\Theta(d^2) \rightarrow \Theta(n)`$ claim is checked structurally instead. |
| 3 | **massif/DHAT profiles for the substitution, sorter, normaliser and evaluator conversions** | No heap-profiling harness exists for those subjects. Building four correct ones — each needing an off-thread $`\Theta(d)`$ set-up so the harness does not measure itself, per §5.7 — was out of scope for this report. Their heap costs are therefore **unquantified**; only their native-stack slopes are measured. |
| 4 | **`perf record --call-graph lbr`** | ⚠ **The stated reason was wrong — corrected in §4.4.** `--call-graph lbr` does fail on this part, but *not* because "the PMU refused every cycles event": plain `cycles` always counted, and what failed was the **precise** modifier `cycles:P`, whose Intel implementation (PEBS) **does not exist on this AMD Zen 3 host**. Substituted with software `cpu-clock` + DWARF, which remains a recorded deviation. ★ `perf record --call-graph dwarf -e cycles:P` now works (`perf_event_paranoid = 0`), and for byte-movement questions `valgrind --tool=cachegrind` is the better instrument because it is deterministic. |
| 5 | **A cycle-accurate CPU profile of the decoder** | ⚠ The "same PMU limitation" is likewise misattributed — see §4.4; a cycle-accurate profile IS available on this host (plain `cycles`, and `cycles:P` since `perf_event_paranoid = 0`). What genuinely blocks this row is the second clause: **no decode benchmark harness exists** (only the massif arm). |
| 6 | **Attribution of the `env_get_deploy` 283 $`\rightarrow`$ 274 drift** | Requires bisecting four `rholang` commits through a 6-second end-to-end runtime bisection each; the drift is *reported* (§5.5.4) and its cause is **not** asserted. |
| 7 | **The falsified prediction the commissioning brief cited as `#103`** (*"predicted d=6 at 55–60 ms, measured 103.57 ms; the cost model was wrong"*) | **Not found in this worktree.** `grep` over all `*.md` and `*.rs` for `103.57`, `55-60` and `55–60` returns nothing outside `target/`. Either it belongs to a different repository or a different document. Three falsified predictions **were** found and are reported (§5.5.1, §5.5.2, §5.6.3). |
| 8 | ~~Depth-axis slopes for 8 of the 9 `mettail` generated drivers~~ | ★★ **OBTAINED** 2026-07-29 — 18 new probe subjects (`ecbe352c`, `f8f71f4c`), both profiles, two ladders. **Eight of nine are SLOPED and it is a live defect class.** See [§5.10.10](#51010--the-gap-is-now-closed-by-measurement--and-eight-of-the-nine-drivers-are-sloped). |
| 9 | **A bisected $`D_{\max}`$ for the standalone `clone` subject** | §5.10.6's **640 levels** is an *extrapolation* from the S0 two-point ladder ($`c = 13\,472`$, $`B = 3254`$), not a bisection. The comparable bisected figure that exists — **729** — belongs to the `inj_attempt_clone` *composition*, a different subject. |
| 10 | ~~A heap profile of `mettail`'s `Arc`-shared AST at HEAD~~ | ★★ **PARTLY OBTAINED** 2026-07-29 — DHAT now shows a depth-4,096 clone allocating **0 additional bytes in 0 additional blocks** ([§5.11.4](#5114-results--space-time-and-the-malloc-analysis)). ⚠ The $`270\times`$ / $`57\times`$ `chain_*` peak-memory figures remain **MEASURED (q)** from `9c55d81d` and were **not** re-run: reproducing them needs the pre-Arc tree, which no longer exists. |
| 11 | **A settling ladder for `ast_semantic_hash_add`** | It reads **2.0 B/level** (two bisection buckets) on the pure chain — flat within the gate's four-bucket tolerance, but not decisively. A $`512 \rightarrow 32{,}768`$ ladder would settle it; not run. |
| 12 | ★★ **Heap profiles for the two fixes that removed QUADRATIC copying** — `SS-A1` (12 `.iter().map(p.clone())` sites, four `..p.clone()` FRUs) and `SS-D2` (the $`O(n^2)`$ `case_rem.to_vec()`) | Both were gated on **native stack only**. `SS-A1`'s own verdict claims it *"removes $`O(D^2)`$ heap churn"* — **quoted, never measured**. [§5.11.3](#5113--how-it-was-decided--falsified-on-time-accepted-on-space) argues this is now the highest-value unobtained measurement in the document, because it is the same shape as the defect the Arc fix found. |
| 13 | **Whether the eight sloped drivers are reachable from a deploy** | The slopes are measured; the *reachability* of a 198-level `ast_cmp` from Rholang source is not established. The same discipline `80f5e5d3` applied to #129 — measure the write side before asserting severity — has **not** been applied here. |

---

### 5.10 ★★ The generated trait implementations, and the `Clone` question

This section exists because a direct question was asked of the record — *was the conversion of `Par`'s `clone` in the stack-safety plan, and what happened to it?* — and because the answer that the campaign's **task tracker** gives and the answer that the **git diffs** give are not the same answer. Both tracker summaries are treated here as **hypotheses**; the diffs decide.

#### 5.10.1 The tracker's chain, and why it cannot be read literally

The tracker records:

| item | commit | closure summary, verbatim |
|---|---|---|
| **#76** | `291bc217` | *"`par_drop` gated + reachability measured — 8.8 kB of source aborts a node; **clone is worse $`\rightarrow`$ #77**"* |
| **#77** | `9082d12c` | *"**2,852 B/level $`\rightarrow`$ 0**; depth **729 $`\rightarrow`$ $`\geq`$ 1,048,576**"* |

Read literally that chain says: *the `Clone` implementation was identified as the worse defect, routed to #77, and converted.* ⚠ **It says something the commits do not.**

#### 5.10.2 What `291bc217` actually routed — a CALL SITE, and it says so

**DERIVED**, `291bc217`, quoted in full because the compression is the whole defect:

> ⚠ BOY SCOUT — a worse traversal on the same path: `inj_attempt` also `.cloned()`s the deploy term (`SignedProcess::Signed` holds it by value), i.e. `<Par as Clone>::clone` at 2,852 B/level release = 735 levels on a 2 MiB worker, ~1.5 kB of source. That is 20x tighter than the destructor's own bound and only 2.6x above the 288-deep AST the 577-byte reproducer already produces. **Logged with its number, not fixed: distinct call site, distinct repair, metering surface.**

★ The routed object is *"`inj_attempt` also `.cloned()`s the deploy term"*, and the commit names it a **distinct call site** in the same sentence. What was routed to #77 was **a call**, not an impl.

#### 5.10.3 What `9082d12c` actually converted — one line, and `models/` is untouched

**DERIVED**, `git show 9082d12c --stat`: four files — the audit, `accounting/mod.rs` (+318, the new by-move accessor), `stack_depth_gate.rs` (+223, the new subject and its guards), and **thirteen lines of `interpreter.rs`**. `git show 9082d12c -- models/` is **empty**: the crate that defines `Par` was not touched.

The entire production change is this (**VERBATIM**, from the diff):

```diff
                 signed_process
-                    .source_process()
-                    .cloned()
+                    .into_source_process()
                     .expect("metered deploy must retain source process")
```

$`\Rightarrow`$ **`9082d12c` deleted a *call to* `<Par as Clone>::clone`. It did not convert `<Par as Clone>::clone`.**

#### 5.10.4 Does any commit convert it? No — and the search is stated so it can be repeated

**DERIVED** — `git log -S … --all` over five spellings, whole history:

| needle | commits |
|---|---:|
| `impl Clone for Par` | **0** |
| `clone_iterative` | **0** |
| `iterative_clone` | **0** |
| `fn clone_drive` | **0** |
| `clone_no_recurse` | **0** |

Corroborated three ways: `models/src/lib.rs` contains **0** `impl Clone` blocks (against 62 `PartialEq` and 62 `Hash`); the generated declaration is `#[derive(Clone,  ::prost::Message)] pub struct Par`; and `models/build.rs`, which **does** strip `PartialEq`/`Eq`/`Hash` from prost's output so they can be hand-written, **does not strip `Clone`**. `Clone` is rustc's derive over `Box`/`Vec` children, and a derived `Clone` over a `Box` child must deep-copy — hence the recursion.

#### 5.10.5 The verdict, and the correction to the tracker

**The deliverable was not dropped, and the tracker's record is nonetheless wrong as written.**

* What #76 **routed** (the `.cloned()` at `inj_attempt`) is exactly what #77 **converted**. No work item was closed on unrelated work.
* But #76's summary compressed *"the `.cloned()` call at `inj_attempt`"* down to the bare word **"clone"**, and #77's summary reports **2,852 $`\rightarrow`$ 0** without saying *of what*. A reader inheriting those two lines concludes that `<Par as Clone>::clone` is converted. **It is not, and it never was in a plan to be**: its standing disposition, from the first audit onward, is *"derived impl — Leg-1 only: remove the call sites, not the impl."*
* ★ This is [§5.7.1](#571-the-register-became-derived-because-every-transcription-drifted)'s class for the third time in this campaign: a figure transcribed into a summary, detached from the subject it measured, and then trusted. The gate is immune to it — `CONVERTED_DEPTH` carries `inj_attempt_clone`, a name that says *which composition*, and `TRIPWIRE_DEPTH` still carries `clone` — but the tracker is not, because nothing checks the tracker.

**Corrected wording, for whoever updates the tracker:**

> **#76** $`\rightarrow`$ *"`par_drop` gated + reachability measured. ⚠ Routed to #77: the `.cloned()` **call site** in `inj_attempt`, whose composition costs 2,852 B/level — **not** `<Par as Clone>::clone` itself, which stays in `TRIPWIRE_DEPTH`."*
> **#77** $`\rightarrow`$ *"`inj_attempt`'s set-initial-cost phase converted by move: the **composition** 2,852 B/level $`\rightarrow`$ 0, ceiling 729 $`\rightarrow`$ $`\geq`$ 1,048,576. `<Par as Clone>::clone` untouched."*

★ **And the gate already carries a warning against exactly this misreading**, which means it has caught someone before. **DERIVED**, `rholang/tests/stack_depth_gate.rs:3050`, the `four_quadrant_s0_baseline` doc comment, verbatim:

> ⚠ **The audit's §12.6 constants (2,852 / 144 / 310 / 1,244) are NOT inherited here.** `9082d12c` removed a *call* to `<Par as Clone>::clone` at `inj_attempt`'s set-initial-cost phase and entered the COMPOSITION in `CONVERTED_DEPTH` as `inj_attempt_clone`; **`<Par as Clone>::clone` itself is untouched and still in `TRIPWIRE_DEPTH`**. And `tree_clone` / `tree_drop` are `score_tree::Tree<T>`, not `Par`. Every number this test prints is re-measured at HEAD.

$`\Rightarrow`$ **The executable artefacts are consistent and correct throughout; only the prose summaries drifted.** That is [§5.7.1](#571-the-register-became-derived-because-every-transcription-drifted)'s finding restated: the copy that is *checked* stays true, and the copy that is merely *written* does not.

#### 5.10.5a ★★ The strategy is CALL-SITE ELIMINATION, not impl conversion — and it should be argued, not inferred

This is the actual design decision of the whole `Clone` thread, and neither tracker line states it.

> **The campaign eliminates `Par`-clone *call sites* rather than converting `<Par as Clone>::clone`.**

**Why elimination beats conversion, per site.** A converted clone still *copies the term*: it would be $`O(1)`$ in native stack and still $`\Theta(n)`$ in time and in **allocation**. Deleting the call saves the traversal **and** the allocation **and** the peak heap. Where a caller can be given ownership instead of a copy, elimination strictly dominates.

**Two sites eliminated, both measured:**

| site | before | after | commit |
|---|---:|---:|---|
| `inj_attempt` set-initial-cost | 2,852 B/level, ceiling 729 | **0**, $`\geq`$ 1,048,576 | `9082d12c` |
| `substitute_and_charge`'s internal `term.clone()` | 2,852 release / 15,872 debug | **146** / **1,462** | `64a5d2bc` |

**$`\Rightarrow`$ The residual is therefore not a number, it is a question: *which callers remain?*** Two are named and measured, and both are named because they are *not* removable:

* **`subst_and_charge`** stays sloped **by design** at 146 B/level — what remains is `encoded_len`, which must walk the term because **its return value *is* the charge** ([§6.3](#63-neutrality-is-the-hard-part-not-the-driver)).
* **`substitute_deep_binding`** at 7,460 B/level — `Env::get` returns its value cloned, and **the copy *is* the meaning of substitution** ([§5.1.5](#515-the-named-residual-of-family-a)).

⚠ **The full caller set is NOT enumerated here, and that is deliberate.** Hand-enumerating callers is this campaign's single most-repeated failure class — [§7.4](#74-enumeration-completeness) records three enumeration methods each of which has a blind spot that was hit at least once, and [§5.10.7](#5107--what-the-mettail-rust-generator-emits-through-a-stack-safe-driver--the-complete-list)'s neighbour [§5.3.1](#531-the-baseline-what-had-never-been-measured) records a hand-picked list of four that missed `Hash`. A residual stated as *"these are the remaining callers"* would be a claim no method in this report can currently support. It is stated as **unenumerated**, and deriving it — a call-graph query for `<Par as Clone>::clone` call sites, in the idiom of the read-ceiling **scan** ([§5.7.8](#578-the-read-ceiling-registry-and-a-fifth-site-it-can-detect)) rather than a list — is named as work in [§8.3](#83--par-as-cloneclone--the-largest-unconverted-traversal-after-prost_de).

#### 5.10.5b ★ The conversion technique is in-tree and proven — it was simply never applied to `Par`

This converts *"we did not do it"* into *"here is the shape of the fix, and here is why it was not taken"*, which is a far more useful thing for a maintainer to inherit.

**DERIVED**, `CONVERTED_DEPTH` in `rholang/tests/stack_depth_gate.rs`:

```text
    // Stage C-1 — Tree's hand-written Clone   (an element of CONVERTED_DEPTH)
    "tree_clone",
```

★ **A hand-written iterative `Clone`, driven by an explicit worklist, already exists in this repository and is in the converted register** — for `sorter::score_tree::Tree<T>`, measured **1,578 / 485 B/level $`\rightarrow`$ 0** ([§5.1.3](#513-results)). The pattern is therefore not hypothetical, not borrowed from another project, and not blocked on a technique nobody has written: it is `SS-A3`, shipped, with its own differential oracle.

**What stops the same shape being applied to `Par`:**

* `Tree<T>` is a **hand-written** type in `models/src/rust/rholang/sorter/score_tree.rs` — its `Clone` is source that a person owns and can replace. `Par`'s `Clone` is **rustc's derive over prost-generated code**, regenerated from `RhoTypes.proto` on every build; replacing it means hand-writing and maintaining `Clone` for **57 message types** whose field lists move with the schema.
* `models/build.rs` already demonstrates the mechanism that would be needed — it **strips** `PartialEq`/`Eq`/`Hash` from prost's output so the 62 + 62 hand-written impls in `models/src/lib.rs` can take over ([§5.10.9](#5109-why-the-same-fix-does-not-transfer-to-par--two-types-two-layers)). Stripping `Clone` too is mechanically the same edit. **The cost is 57 more hand-written impls to keep in step with the schema**, against a defect whose call sites are being removed one at a time instead.
* $`\Rightarrow`$ **The choice is defensible and it is a choice, not an omission.** It should be recorded as such rather than left for the next reader to rediscover from a tracker line.

#### 5.10.6 Reconciling 729, 2,852 and 3,254 — three numbers, three subjects

They are not in conflict; they measure three different things, and saying which is which is the point.

| figure | subject | what it is |
|---:|---|---|
| **735** | arithmetic | $`\lfloor 2\,097\,152 / 2852 \rfloor`$ — an **upper bound**, circulated before anyone bisected it |
| **729** | `inj_attempt_clone` — the *composition* (clone **+** the derived drop of the original) | **MEASURED (q)**, `9082d12c`, bisected; depth 730 aborts with status 134 |
| **2,852** | the same composition's slope, at that call site | **MEASURED (q)** — the clone dominates it, so composition slope $`\approx`$ clone slope there |
| **3,254** | `clone` — the *standalone* gate subject, 16 $`\rightarrow`$ 128 ladder | **MEASURED (f)** at HEAD, **twice**, by two instruments in one binary |

The 2,852-vs-3,254 gap is **inlining context**, not disagreement: `9082d12c` itself records that 729 agreed *"to the level with the standalone `clone` subject's own bisected maximum"* on the ladder in use then. The S0 harness later drove the standalone subject on its own self-selected 16 $`\rightarrow`$ 128 ladder and got 3,254; both numbers are real, and the gate publishes the standalone one because that is the traversal that still exists.

**MEASURED (f)**, 2026-07-29, at HEAD, two independent readings in the same release binary:

```text
/tmp/sd_gate_release.log   clone: 3254 B/level (ceiling 5000)
/tmp/sd_s0_release.log     clone,clone,release,16,65536,128,430080,364544,3254
```

**Reachable depth, DERIVED** from the S0 row: intercept $`c = 65\,536 - 16 \times 3254 = 13\,472`$ B, so on the 2 MiB stack a `tokio` worker gets,

```math
D_{\max} \;=\; \left\lfloor \frac{2\,097\,152 - 13\,472}{3254} \right\rfloor \;=\; 640 \ \text{levels}
```

⚠ That is an **extrapolation from a two-point ladder, not a bisection**; it is offered as an order of magnitude and is listed in [§5.9](#59-measurements-that-could-not-be-obtained) as an unbisected figure.

#### 5.10.7 ★ What the `mettail-rust` generator emits through a stack-safe driver — the complete list

The companion generator solves the same problem for a *different* term family, and it solves it for more traversals. **DERIVED** from `macros/src/gen/term_ops/mod.rs`, `macros/src/gen/mod.rs`'s `spill_and_include` call list, and the emitted tree at `target/generated/rholang/`.

**Nine generated modules, each carrying a task enum, a thread-local pool and a `*_iterative` driver:**

| generated file | traits / operations implemented | driver entry point |
|---|---|---|
| `iterative_cmp.rs` | `PartialEq`, `Eq`, `PartialOrd`, `Ord` | `eq_iterative`, `cmp_iterative` |
| `iterative_hash.rs` | `std::hash::Hash` | `hash_iterative` |
| `iterative_drop.rs` | `Drop` | *(driver inlined in the impl)* |
| `debug.rs` | `std::fmt::Debug` | `debug_iterative` |
| `display.rs` | `std::fmt::Display` | `display_iterative` |
| `semantic_hash.rs` | $`\alpha`$-canonical identity (inherent) | `semantic_hash_iterative` |
| `subst.rs` | capture-avoiding substitution (inherent) | `subst_iterative` |
| `normalize.rs` | collection canonicalisation (inherent) | `normalize_iterative` |
| `match_pattern.rs` | pattern matching (inherent) | `match_pattern_iterative` |

**Count: 9 modules, covering 6 standard traits (`PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Drop`, `Debug`, `Display` — 8 counting each separately) and 4 inherent term operations.** Emitted once per language; the tree carries **76** copies of each of `iterative_cmp.rs`, `iterative_drop.rs` and `iterative_hash.rs`, one per generated language.

★★ **`Clone` is deliberately NOT among them, and that is the interesting part.**

#### 5.10.8 ★★ `Clone` was converted — and then the conversion was *deleted*, because the representation made it unnecessary

**DERIVED**, `macros/src/gen/types/enums.rs:249-263`, quoted verbatim from the generator:

> NOTE: Clone IS derived (ARC refactor, 2026-05-28). Recursive AST children are now `std::sync::Arc<Cat>` (was `Box<Cat>`), so derived `Clone` is `Arc::clone` per child — $`O(1)`$ and NON-recursive (it stops at the Arc boundary, never descending the subtree). … The old iterative work-stack clone (`gen/term_ops/iterative_clone.rs`) existed solely to avoid stack overflow on deep `Box` chains — obsolete now that `Arc::clone` does not recurse; that module was removed (2026-06-22).

**Confirmed in the emitted tree** (**DERIVED**, `target/generated/rholang/ast_enums.rs`): `#[derive(Clone, mettail_runtime::BoundTerm)]`, **206** occurrences of `Arc<Proc>`, and **`Box<` occurs 0 times**. `iterative_clone.rs` was removed by `651499e2`, whose body records it was already *"uncompiled"*.

★ **So mettail's `Clone` is stack-safe by *representation*, not by a driver.** A refcount increment cannot recurse. That is a strictly better answer than a trampoline — no driver to maintain, no differential oracle to keep in step, no work item to allocate — and it is available only because the AST is *shared*, which a term rewriter can be and a protobuf message cannot.

**And the arc it took is a two-step scientific record worth preserving in full**, because the first step reached the *opposite* conclusion and was right to.

| date | commit | verdict |
|---|---|---|
| 2026-05-23 | `ff506dc5` | **HYPOTHESIS FALSIFIED.** A Stage-1 profile gate measured AST clone at **0.78 % of total runtime** on `rhocalc_bench::replication/basic`, the rewrite-heaviest workload in the workspace — $`\approx 13\times`$ below the 10 % gate threshold. *"Even at 100 % elimination … the maximum achievable speedup is < 0.78 % wall-clock — below Criterion's intra-sample noise floor."* Do **not** proceed. Effort: 2 h against the 17–20 h the implementation would have cost. |
| 2026-05-28 | `9c55d81d` | **DONE ANYWAY — on the other axis.** `heaptrack` proved **96 % of `chain_1000` peak heap (288 / 300 MB)** was `clone_iterative` deep-copying the accumulated subtree at every chain step, $`N^2`$ calls, because children were `Box<Cat>`. |

★★ **The methodological lesson, and it generalises past this subject: the same hypothesis was correctly *refused* on the time axis and correctly *accepted* on the space axis.** A gate that profiles only wall clock will refuse a fix worth two orders of magnitude in memory. Neither decision was wrong; the first gate simply measured the wrong resource for the defect that was actually there.

**Results, MEASURED (q), `9c55d81d`, release:**

| workload | before | after | factor |
|---|---:|---:|---:|
| `chain_10000` peak memory | $`\approx`$ 30 GB (OOM) | **112 MB** | $`\approx 270\times`$ |
| `chain_2000` peak memory | 1.53 GB | **26.5 MB** | $`57\times`$ |
| `chain_2000` wall clock | 4.34 s | **0.11 s** | $`39\times`$ |
| asymptotic memory | $`O(N^2)`$ | $`O(N)`$ | class change |

★ It also **overturned a prior conclusion**: *"the 44.7 GB architectural ceiling requires a different parser algorithm"* was wrong — *"it was AST-clone, fixed by representation, no algorithm substitution."*

#### 5.10.9 Why the same fix does not transfer to `Par` — two types, two layers

| | `mettail`'s `Proc` | `f1r3node`'s `Par` |
|---|---|---|
| defined by | the `language!` macro | `prost-build` from `RhoTypes.proto` |
| recursive child | `std::sync::Arc<Cat>` (**206**, `Box<` = **0**) | `Box<T>` / `Vec<T>`, as prost emits |
| `Clone` | derived — `Arc::clone`, **non-recursive** | derived — **deep copy, recursive** |
| measured | $`O(1)`$ per node by construction | **3,254 B/level** |
| `PartialEq` / `Hash` | generated iterative drivers | **62 + 62 hand-written impls** in `models/src/lib.rs` |
| `Drop` | generated iterative driver ⚠ (see below) | rustc's implicit glue, **144 B/level** |

$`\Rightarrow`$ **The repair that fixed `Clone` in `mettail` is a change of *data representation*, and on the `f1r3node` side that means changing what `prost-build` emits.** `Box` $`\rightarrow`$ `Arc` for recursive protobuf fields is a codegen change in a third-party crate, and it would alter the public type signature of every `Par` field — consensus-adjacent and upstream. It is the one repair that would retire `clone` (3,254), `par_drop` (144), `eq` (221), `hash` (136) and `ord` (438) **simultaneously**, and it is not this campaign's to take. Recorded in [§8.3](#83--par-as-cloneclone--the-largest-unconverted-traversal-after-prost_de) as the standing alternative to converting five traversals one at a time.

#### 5.10.10 ★★ THE GAP IS NOW CLOSED BY MEASUREMENT — and eight of the nine drivers are SLOPED

The previous revision of this section said the eight undriven modules were **DERIVED**-safe and not **MEASURED**-safe, and listed the gap in §5.9. ★ **That gap has been closed, and the drivers failed.**

**Instrument.** Eighteen new subjects in `rholang-runtime/src/bin/stack_depth_probe.rs` (commits `ecbe352c`, `f8f71f4c`), driven by `RLIMIT_STACK` bisection on the **child's main thread** at 4 KiB resolution — the idiom the gate's existing fifteen subjects use, because `RUST_MIN_STACK` cannot reach a main thread. Ladder $`16 \rightarrow 4{,}096`$, **both profiles**. Every subject `mem::forget`s its terms so no ladder carries `ast_drop`'s own slope, and every subject carries an anti-vacuity assertion forced by its own shape (`eq` twins must compare **equal** or the walk short-circuits; `cmp` twins differ **only at the leaf**; `hash` must give two digests for two leaves; the printers must emit more bytes than the depth; `subst`'s variable sits at the **leaf**; `match_pattern` must **match**).

★★ **The experiment is a 2 $`\times`$ 2: nine drivers $`\times`$ two ladders that differ only in the SHAPE walked.**

| ladder | shape | does a cross-type hop exist? |
|---|---|---|
| **A — alternating** | `nested_list`: `Proc::CastList(Arc<List>)` then `List::ListLit(Vec<Proc>)` | **yes, at every level** |
| **B — pure chain** | `nested_add`: `Proc::Add(Arc<Proc>, Arc<Proc>)` | **no** |

⚠ **The polarity matters and is stated because it is easy to invert:** the *list* ladder is the alternating one — `Proc` $`\rightarrow`$ `List` $`\rightarrow`$ `Proc` at every level, confirmed from `target/generated/rholang/ast_enums.rs:52` (`CastList(Arc<List>)`) and `:3014` (`ListLit(Vec<Proc>)`) — and the *add* ladder is the pure one (`:81`, `Add(Arc<Proc>, Arc<Proc>)`).

![generated drivers, two ladders](figures/generated-drivers-two-ladders.svg)

**Figure 7** — *`figures/generated-drivers-two-ladders.puml`*. The 2 $`\times`$ 2 that identifies the mechanism.

**MEASURED (f)**, 2026-07-29. B/level, **debug / release**. Logs: `/tmp/sd_mettail_debug_alt.log`, `/tmp/sd_mettail_release.log`, `/tmp/sd_mettail_debug_pure.log`, `/tmp/sd_mettail_release_add.log`, `/tmp/sd_mettail_debug_clone.log`, `/tmp/sd_mettail_release_clone.log`.

| driver / module | **ladder A — alternating** | **ladder B — pure chain** | verdict |
|---|---:|---:|---|
| `ast_cmp` — `iterative_cmp.rs` (`Ord`) | **10,592.4 / 335.3** | $`\approx 0`$ / 0.0 | ⚠ **SLOPED** |
| `ast_debug` — `debug.rs` | **10,544.2 / 463.8** | $`\approx 0`$ / 0.0 | ⚠ **SLOPED** |
| `ast_eq` — `iterative_cmp.rs` (`PartialEq`) | **6,144.0 / 175.7** | $`\approx 0`$ / 0.0 | ⚠ **SLOPED** |
| `ast_normalize` — `normalize.rs` | **6,144.0 / 176.7** | $`\approx 0`$ / 0.0 | ⚠ **SLOPED** |
| `ast_match_pattern` — `match_pattern.rs` | **6,138.0 / 174.7** | $`\approx 0`$ / 0.0 | ⚠ **SLOPED** |
| `ast_subst` — `subst.rs` | **6,134.0 / 174.7** | $`\approx 0`$ / 0.0 | ⚠ **SLOPED** |
| `ast_hash` — `iterative_hash.rs` | **1,216.8 / 206.8** | $`\approx 0`$ / 0.0 | ⚠ **SLOPED** |
| `ast_semantic_hash` — `semantic_hash.rs` | **1,214.7 / 206.8** | 2.0 / 0.0 | ⚠ **SLOPED** |
| `ast_drop` — `iterative_drop.rs` | **254.0 / 96.4** | $`\approx 0`$ / 0.0 | ⚠ **SLOPED** *(already on record)* |
| `ast_display` — `display.rs` | **0.0 / 0** | $`\approx 0`$ / 0.0 | ★ **FLAT on both** |
| `ast_clone` — **no driver**, `Arc::clone` | **2.0 / 0** | 1.0 / 0.0 | ★ **FLAT on both** |
| `build_twins` — **control, runs no driver** | 1.0 / 0.0 | $`\approx 0`$ / 0.0 | control flat |
| `build_one` — **control, no clone** | — | 0.0 / 0 | control flat |

#### 5.10.10a ★★★ The mechanism is confirmed, 9 for 9 — and this is a live defect class

**Every driver is sloped on ladder A and flat on ladder B. The controls are flat on both.** So the slope is the **driver's own**, not the builder's, not the `Arc` bumps', and not the harness's.

$`\Rightarrow`$ **The generated work stacks are typed per category and do not follow an edge that leaves their own type.** The hypothesis stated in `ast_drop`'s own note is confirmed for **nine of nine** drivers, not merely for `Drop`.

★ **Instrument validated against the one figure already on record.** `ast_drop` bisects to **254.0 B/level debug**, reproducing `3c0c3585`'s recorded 254 **to the byte**, and **96.4 release** against the recorded 96. A harness that disagreed with the one pre-existing measurement would have produced a table nobody could reconcile.

⚠⚠ **These eight are a LIVE DEFECT CLASS in `macros/src/gen/`, not a documentation gap.** Depth-independence is these modules' entire purpose — they exist *because* the derives would overflow. `ast_cmp` at 10,592 B/level debug reaches its ceiling on a 2 MiB stack at roughly **198 levels**; `ast_debug` at 10,544 likewise. Filed as such, with slopes, mechanism and owner, rather than buried in a table.

★★ **`ast_display` is the most valuable cell in the experiment.** It is flat on **both** ladders — so `display.rs`'s driver **does** follow the cross-type hop. The defect is therefore **not inherent to the generator's approach**, and there is an **in-tree reference implementation** to copy. That converts "eight drivers are broken" into "eight drivers should be rewritten the way the ninth already is", which is a far more actionable finding.

⚠ **One cell needs a wider ladder to settle.** `ast_semantic_hash_add` reads **2.0 B/level** (growth 8,192 B = two bisection buckets). The gate's own zero-slope tolerance is **four** buckets, so it is flat within resolution; it is recorded as *flat within tolerance* rather than silently rounded, and a $`512 \rightarrow 32{,}768`$ ladder would decide it. Added to [§5.9](#59-measurements-that-could-not-be-obtained).

⚠ **Both claims are now reconciled, and neither is left standing against the other.** The earlier text carried *"the drivers are DERIVED-safe"* and *"`ast_drop` is not flat"* simultaneously. Measurement resolves it in favour of the second: **the drivers are safe only on shapes that stay inside one category**, and the family's own most common shape — a collection literal — does not.

### 5.11 ★★ SS-G1 — the Arc fix: eliminating a traversal instead of converting it

Every other fix in this report **converts** a recursive traversal into an iterative one. This one **removes the traversal**, and it is the most effective fix in the corpus by two orders of magnitude. It earns its own section because its mechanism is unique here and because the way it was decided is the report's best methodological lesson.

#### 5.11.1 The defect

**MEASURED (q)**, `9c55d81d`: `heaptrack` attributed **96 % of `chain_1000`'s peak heap — 288 MB of 300 MB —** to `clone_iterative` deep-copying the accumulated subtree at **every chain step**, i.e. $`\Theta(N)`$ calls each costing $`\Theta(N)`$, hence $`\Theta(N^2)`$ total. **Root cause, DERIVED**: the generated AST enums held recursive children as `Box<Cat>`, so every `.clone()` deep-copied the whole subtree.

★ At `chain_10000` this was **~30 GB and an OOM (out-of-memory) kill** — and it had been *misdiagnosed as an architectural limit*: a standing conclusion held that *"the 44.7 GB ceiling requires a different parser algorithm."*

#### 5.11.2 The architecture of the repair, and why this shape

Recursive AST children become `std::sync::Arc<Cat>`. Derived `Clone` is then `Arc::clone` per child — **$`O(1)`$, non-recursive, stopping at the `Arc` boundary and never descending**. `iterative_clone.rs`, the work-stack driver that existed *solely* to keep deep `Box` chains off the native stack, became dead and was deleted (`651499e2`, its body recording that the module was already *"uncompiled"*).

**Why representation rather than a driver — and this is the general point:** a converted clone is still $`O(1)`$ in native stack and still $`\Theta(n)`$ in **time** and in **allocation**, because it still copies. Sharing makes the copy *not happen*. There is no driver to maintain, no differential oracle to keep in step, and no work item to allocate.

⚠ **It is available only because the AST is immutable and shared.** A term rewriter can share subterms; §5.11.6 is why a protobuf message cannot.

**DERIVED**, `target/generated/rholang/ast_enums.rs`: `#[derive(Clone, mettail_runtime::BoundTerm)]`, **206** occurrences of `Arc<Proc>`, and **`Box<` occurs ZERO times**.

#### 5.11.3 ★★ How it was decided — falsified on time, accepted on space

This is the pedagogical centrepiece of the report.

| date | commit | axis | verdict |
|---|---|---|---|
| 2026-05-23 | `ff506dc5` | **time** | **HYPOTHESIS FALSIFIED.** A Stage-1 profile gate measured AST clone at **0.78 % of total runtime** on `rhocalc_bench::replication/basic`, the rewrite-heaviest workload in the workspace — $`\approx 13\times`$ below the 10 % gate threshold. *"Even at 100 % elimination … the maximum achievable speedup is < 0.78 % wall-clock — below Criterion's intra-sample noise floor."* Do **not** proceed. Cost: 2 h, against the 17–20 h the implementation would have taken. |
| 2026-05-28 | `9c55d81d` | **space** | **DONE, and it won two orders of magnitude.** `heaptrack` on the memory axis found the 96 % attribution above. |

★★ **The same hypothesis was correctly refused on the time axis and correctly accepted on the space axis. Neither decision was wrong.** The first gate was a well-run experiment that measured *the wrong resource for the defect that was actually there*.

> **The general lesson: a wall-clock-only acceptance gate will refuse a fix worth $`270\times`$ in memory.** An acceptance gate must name the resource it is gating, and a "no motivation" verdict is only as broad as the resource profiled.

⚠★ **So the question this raises has to be asked of the rest of the register, not just admired here.** Which other fixes were gated on **time alone** and might deserve a space re-examination?

| fix | what gated it | space-examined? |
|---|---|---|
| **SS-C2** cold-store encoder | wall clock **and** massif **and** DHAT ([§5.3.4](#534--the-malloc-profile--where-the-allocations-moved)) | ✅ yes — that is where the $`770\times`$ block-count win was found, and it was **not** the reason the fix was undertaken |
| **SS-C1** cold-store decoder | native stack; heap measured here for the first time | ✅ yes — 304.7 B/level of value stacks |
| **SS-A1** substitution leg-1 de-clone | native stack B/level only | ⚠ **NO** — and it is the strongest candidate: it removed 12 `.iter().map(p.clone())` sites and four `..p.clone()` functional-record-updates from a $`\Theta(d)`$ type. Its own verdict says it *"removes $`O(D^2)`$ heap churn"* — **quoted, never measured.** |
| **SS-D2** ownership to the substitution | native stack; the $`O(n^2)`$ `case_rem.to_vec()` reasoned about, not profiled | ⚠ **NO** — same shape as the Arc defect: a quadratic *copy* pattern |
| **SS-A2/A5/A7, SS-B1** the worklist conversions | native stack B/level only | ⚠ **NO** — [§5.9](#59-measurements-that-could-not-be-obtained) #3 |

$`\Rightarrow`$ **Two fixes in this register removed quadratic copying and neither has a heap profile.** That is now the highest-value unobtained measurement in the document, and it is recorded in [§8.4](#84-not-measured-and-worth-measuring) as such rather than as a nice-to-have.

#### 5.11.4 Results — space, time, and the malloc analysis

**MEASURED (q)**, `9c55d81d`, release:

| workload | before | after | factor |
|---|---:|---:|---:|
| `chain_10000` peak memory | $`\approx`$ 30 GB (OOM kill) | **112 MB** | $`\approx 270\times`$ |
| `chain_2000` peak memory | 1.53 GB | **26.5 MB** | $`57\times`$ |
| `chain_2000` wall clock | 4.34 s | **0.11 s** | $`39\times`$ |
| asymptotic memory | $`O(N^2)`$ | $`O(N)`$ | class change |

★ **MEASURED (f)** — the allocation analysis, obtained for this report and not previously available. Valgrind DHAT, debug, comparing `ast_clone` (build a term **and clone it**) against `build_one` (build the identical term, **no clone**), so the cost of one clone is a **subtraction** rather than an inference from the type:

| subject | depth | total bytes | total blocks |
|---|---:|---:|---:|
| `build_one` | 16 | 4,487 | 44 |
| `ast_clone` | 16 | **4,487** | **44** |
| `build_one` | 4,096 | 461,449 | 8,204 |
| `ast_clone` | 4,096 | **461,449** | **8,204** |

★★ **Byte-for-byte and block-for-block identical at both depths. Cloning a depth-4,096 term allocates ZERO additional bytes in ZERO additional blocks.** The clone is refcount increments and nothing else — the mechanism confirmed by measurement, not by reading `Arc`'s documentation.

**And the native stack, MEASURED (f)** (ladder $`16 \rightarrow 4{,}096`$, debug / release): `ast_clone` **2.0 / 0** B/level on the **alternating** ladder — flat within the gate's four-bucket tolerance — where on that same ladder `ast_eq` is **6,144.0** and `ast_cmp` is **10,592.4**. `Clone` is flat on the shape that defeats every generated driver, *because it does not traverse at all*.

#### 5.11.5 What it cost

* **Sharing is now observable.** `Hash`/`Eq`/`semantic_hash` had to be deref-transparent, and binder operations became copy-on-write via `Arc::make_mut` (**DERIVED**, `9c55d81d`).
* **`Drop` had to change with it** — `iterative_drop` uses `Arc::into_inner`, which is $`O(1)`$ for a *shared* subtree. ⚠ And [§5.10.10](#51010--the-gap-is-now-closed-by-measurement--and-eight-of-the-nine-drivers-are-sloped) shows that teardown is still **254.0 / 96.4** B/level across the cross-type hop, so the Arc fix did not make `Drop` flat; it made the *clone* free.
* **A secondary residual was named at the time and not fixed:** `emit_sppf_subforest` recurses $`\approx N/2`$ deep, so `chain_10000` needed a large stack (**DERIVED**, `9c55d81d`).
* **Blast radius.** It touched `enums.rs`, the semantic actions, `subst`, `normalize`, `iterative_drop`, the binder and congruence passes, `eval`, `ast::pattern`, every `test_gen` emitter, a hand-written grammar, `numeric_dispatch`, and 12 test files.

#### 5.11.6 ⚠ Why it does NOT transfer to f1r3node's `Par` — three independent reasons

Each is sufficient on its own, and [§8.3](#83--par-as-cloneclone--the-largest-unconverted-traversal-after-prost_de)'s residual depends on all three.

1. **`Par`'s recursion runs through `Vec<T>`, not `Box<T>`.** The Arc trick collapses `Box` *chains*; a `Vec<Send>` clone must clone **every element** regardless of what wraps `Send`. The repeated fields are the recursion, and sharing the wrapper does not remove the element copies.
2. **`Par` is mutated in place.** `Message::clear` and `merge_field` mutate, and there are **38** `Par { .., ..Default::default() }` functional-record-update sites in `models` **alone** (**MEASURED (q)**, `44535d75`'s E0509 probe). Every one would need copy-on-write.
3. **`prost-build` cannot emit it.** `Config` offers **`boxed(path)`** and **no `arc` equivalent**, and prost's generated `Message` impl is written against the concrete field types — an `Arc` rewrite would not compile.

$`\Rightarrow`$ **The representation route is closed for `Par`.** The traversal route is the one being taken; [§8.3](#83--par-as-cloneclone--the-largest-unconverted-traversal-after-prost_de) records the programme.

#### 5.11.7 What is still recursive

`Drop` across the cross-type hop (254.0 / 96.4 B/level, [§5.10.10](#51010--the-gap-is-now-closed-by-measurement--and-eight-of-the-nine-drivers-are-sloped)); the eight sloped generated drivers, which the Arc fix does not touch because they *traverse* rather than copy; and `emit_sppf_subforest`'s $`N/2`$ recursion.

---

## 6. Discussion

### 6.1 Why the explicit-worklist shape, and why it is *smaller* than what it replaces

Every conversion in §5 is the same transformation: make the continuation a **first-class value** and drive it with a loop [[Reynolds 1972](#ref-reynolds1972); [Ager et al. 2003](#ref-ager2003)]. The choice among the family's variants was made per traversal, and the discriminating question each time was *what does the resumption actually need to remember?*

| traversal shape | driver shape | why |
|---|---|---|
| pre-order into a contiguous buffer (encoder) | **op stack only, no value stack** | the output *is* the accumulator, so nothing needs parking — cf. [[Cheney 1970](#ref-cheney1970)] |
| bottom-up reassembly (decoder, substitution, sorter) | **work stack + value stack(s)** | a parent cannot be constructed until its children exist |
| post-order fold with *interleaved* checks (expression evaluator) | **work stack + value stack + an `Extract` continuation** | error **position** is observable, so per-operand checks must fire between siblings (§5.2.1) |
| a `Par` spine with a left-biased search (`into_source_process`) | **work stack, push RIGHT before LEFT** | reproduce the `.or_else` order exactly |
| a discard (ingress, teardown) | **worklist, no reconstruction at all** (`dismantle`) | nothing reads the value; only the frees are reordered |

★ **The counter-intuitive empirical result is that the heap cost per level is far *below* the stack cost it replaces** — $`5.1\times`$ for the encoder and $`42.3\times`$ for the decoder (§5.3.4). The explanation is §5.1.1's `gdb` attribution: a compiler-laid-out frame for a 40-arm `match` reserves slots for **every** arm's live locals and does not overlap mutually exclusive arms at `-O0`, whereas a work item is a hand-designed 32-byte `enum`. Trampolining is usually presented as trading space for safety; here it bought both, and the reason is that the thing it replaced was *fat*, not that the thing it introduced is *thin*.

### 6.2 Why the SCC is the unit of conversion

Three independent instances say the same thing.

* **Substitution**: converting a subset would have left the cycle intact.
* **Lowering** (§5.6.1): the reproducer's cycle passes through **twelve `core::iter` adapter monomorphisations**; the 18 direct self-call sites are *not* the component, and converting them would have left the reproducer exactly as it was.
* **The sorter** (§5.1.4): converting `sort_match` alone would have left `compare_score` $`\Theta(d)`$ **and the gate would still have passed**, because a linear chain sorts as a one-element vector on which `sort_by` performs zero comparisons.

$`\Rightarrow`$ **Scope by Tarjan, not by intuition**, and — §5.1.4 again — remember that a Tarjan run over the *schema* cannot see a recursive Rust type that is not a schema message.

### 6.3 Neutrality is the hard part, not the driver

Every one of these traversals is on a consensus-visible path. The drivers were the easy half; the expensive half was proving that nothing observable moved.

The obligations differ by traversal and were derived per traversal rather than assumed:

* **Substitution** is cost-neutral **by construction**: `substitute_and_charge` charges `encoded_len()` of the **result**, once, *outside* the recursion, and the SCC contains **zero** `reserve_*`/`Cost::` calls — asserted mechanically over all 97 corpus cases, not left as prose.
* **The sorter** takes no metering handle, which makes the *cost* obligation vacuous and therefore makes the **result** obligation the whole obligation: `cost_accounting/sig.rs` signs `sort_match(&par).term.encode_to_vec()`, so **a one-element reordering is a fork**. `sort_by` is kept and `sort_unstable_by` refused, because the comparator returns `Equal` for distinct terms with equal scores and an unstable sort would be free to reorder them.
* **The codecs** owe *byte identity* on the write side and *language identity* — including the `Err` half — on the read side.
* **The async driver** owes COMM order, checked by a differential on the ordered `(BillableKind, weight)` trace, not on a final state.

★ **And `encoded_len` is called out as the one member not covered by the neutrality argument**, because **its return value *is* the charge**. That is why `subst_and_charge` remains sloped at 146 B/level after a $`19.5\times`$ improvement, and why it will stay sloped.

### 6.4 ★ The write/read asymmetry — one section, because it is one class

The campaign kept re-discovering one shape:

> A `Par` is **written by an unbounded encoder** and later **read by a bounded decoder**, with the two ends on different paths, different processes, or different times.

**Both halves matter**, and that is not pedantry: the **bincode** wire is iterative and depth-unlimited in *both* directions, measured flat from depth 4 to 4,096, and is therefore **not in the class** — which is why the cold store does not appear in the registry at all.

Four independent instances (**DERIVED**, `rholang/tests/par_read_ceiling_site_registry.rs:1-11`):

| # | where | shape |
|---|---|---|
| **#120** | the audit's §7.3 | a term can be built, reduced and serialised but **not read back** |
| **#129** | `output_value` | play accepts a depth-34 payload; replay refuses it |
| **#130** | the trie's value slot | `decode_trie_path` is not total on `encode_trie_path`'s image |
| **#4** | `EPathMap`'s own prost wire, tag 8 | a map encodes at entry depth 34 and refuses to decode |

**#129 was downgraded by measurement, and that is a result.** The claim under test was *"a proposer-controlled consensus divergence"*, resting on a fixture that **spliced** a depth-34 `Par` into a recorded log — and that fixture's own header said splicing proves the decode asymmetry and *not* reachability. **Nobody had measured the write side.** Driving all seven reachable non-deterministic operations through real deploys on a real runtime (**MEASURED (q)**, `80f5e5d3`):

```text
rho:ai:gpt4 / dalle3 / textToAudio    depth 0
rho:ollama:chat / generate            depth 0
rho:ollama:models                     depth 1   ← the deepest
rho:io:grpcTell                       depth 0
rho:chroma:…:query (compiled out)     depth 2   recorded, not driven
```

Read ceiling 33; **tightest margin 31 levels**. $`\Rightarrow`$ **#129 is LATENT, not live.**

★ **And the defect that *is* real is not the one in the title.** The previous guard was a *sentence* — "all eight of those operations' return constructions were read" — with a hand-listed denominator. Add a ninth operation and the sentence stays grammatical, the fixture stays green, and the block-killing behaviour is live with no compile error anywhere. The denominator is now `non_deterministic_ops()` itself, read from production at run time.

**A second axis nobody had enumerated (`0e0f9719`).** Everything above concerns **depth**. `EMinus { Par p1 = 1; }` is proto3, so the field is optional **on the wire** and `prost` returns `None` for an absent one **without error**. An absent required child is therefore not something `Par::decode` *refuses*; it is something `Par::decode` **returns**. **Ten bytes** — `[2a 08 52 06 12 04 2a 02 10 04]` — are accepted by `Par::decode` and fault four independent consumers: `has_locally_free.rs` (94 sites), `spatial_matcher.rs` (55), `par_map_type_mapper.rs` (1), and the FFI (foreign function interface) rows. The disposition is settled once: an absent required child is an **internal invariant violation**, not a decidable negative, so the sites assert (`.expect`) and the fix belongs at the boundary admitting foreign bytes — because `spatial_match` returns `Option<()>`, whose inhabitants are *matched* and *did not match*, and answering `None` would make a malformed term indistinguishable from a well-formed non-matching one.

⚠ **Reachability of that axis came out split**, and is reported split: the first fault is the normaliser's own path, not the matcher; on the consensus path it is **LATENT for a structural reason** (on replay the tuple space is trace-driven, so the only pattern a spliced value ever meets is one that already matched a well-formed value at play time — five deploy-writable receive shapes all replay clean); and through the FFI it is **worse than a thread panic**, because the implicit non-unwinding shim **aborts**, so `catch_unwind` cannot intercept and the whole process dies. Severity is bounded only by there being **no in-tree caller**: a live public ABI (application binary interface) with no live consumer.

### 6.5 What a "fix" for the asymmetry would have to be

Three options exist and **none is this campaign's to take**:

1. **Cap the writer.** A construction bound is **consensus-visible** — it changes the set of accepted terms — so it is F1r3node's decision, not a repair.
2. **Uncap the reader.** `RECURSION_LIMIT` is private and unconfigurable in `prost`; raising it *widens* the accepted byte-string set, which is equally consensus-visible and needs a coordinated version bump.
3. **Convert the prost paths**, as the cold store was converted. This is the only option that changes no accepted-input set — the S2 encoder (§5.3.5) is the first half of it, done and dormant. The **decoder** half is not started.

The gate therefore asserts what is *true* rather than what is *desired*: **the wire is the binding constraint, and every build path clears it with headroom**, tripwired on the **ratio** ($`283/33 = 8.57`$, floored at $`8\times`$) so that it fires when a build path regresses **or** a read ceiling rises. ⚠ As §5.5.4 records, that inventory's 283 is stale at 274 and the ratio is now 8.30 — still passing, on a number that has drifted.

---

## 7. Threats to validity

### 7.1 The observable is a proxy

`B/level` is the slope of *minimum surviving stack*, bisected to 4,096 B. It is **not** a direct measurement of frame size, and it aggregates every frame in the cycle. Two specific consequences:

* A traversal with $`B < 4096/\text{span}`$ is indistinguishable from $`B = 0`$ by this instrument. The gate's zero-slope ceiling over a 4 $`\rightarrow`$ 4,096 span is **4 B/level in integer arithmetic**, with the sloped control ~40 $`\times`$ above it — so the margin is real but finite.
* The estimator is affine-by-assumption. §5.6.3 exhibits a subject that is **flat then linear**, with a knee at depth ~256 whose mechanism is *not established*. Any subject measured only below its knee would be mis-classified, which is why the gate's ladders end at 4,096 and its width ladders at 65,536.

### 7.2 Measurement environment

* ⚠★ **The measured tree is HEAD *plus uncommitted concurrent work*, not clean HEAD.** `rholang/tests/stack_depth_gate.rs` — the instrument behind every `B/level` figure in [§5](#5-results) — carried **uncommitted modifications by a concurrent agent** at measurement time ($`+76`$ inserted, $`14`$ deleted; file mtime 11:47, i.e. before this session began; **this report modified no source file**). The binaries measured were built from that working tree. Three things bound the risk, and they are stated rather than assumed: the diff touches **zero** lines of `CONVERTED_DEPTH`, `TRIPWIRE_DEPTH` or any `assert_slope_below` call (checked by `git diff | grep -c`); it is predominantly `rustfmt`-shaped reflow plus an S0 doc block; and **the S0 run reproduced the independently recorded baseline table byte-exactly in all ten rows** ([§5.3.1](#531-the-baseline-what-had-never-been-measured)), which a perturbed instrument would be unlikely to do. $`\Rightarrow`$ The figures are treated as sound, and **a re-run at clean HEAD is the honest way to confirm it** — named here rather than glossed.
* **One machine, one microarchitecture.** All figures are Zen 3, `target-cpu=native`. Frame layouts are compiler- and target-specific; §5.5.2's finding that the *same* function is emitted at 48 B in one binary and 64 B in another (a $`1.5\times`$ spread **from register allocation alone**) is direct evidence that these constants do not transfer between builds, let alone between targets.
* ⚠ **Not at maximum frequency, and not on a quiet machine.** §4.2: core 8 at **77.2 %** of maximum boost, load average ~13 on 32 cores, concurrent agent workloads throughout. The **relative** timings of §5.4.1 are protected by within-repetition interleaving and by the non-overlap check; the **absolute** nanosecond figures are not, and should not be quoted as throughput characteristics of the hardware.
* **The settle was 302 s of *quieter*, not of *quiet*.** The $`\geq 300`$ s discipline was met arithmetically; the machine was never idle.
* ⚠ **`perf` used a software event.** See §4.4. Symbol attribution is unaffected; sampling fidelity is.
* **Heap figures are architecture-independent but allocator-dependent.** massif and DHAT observe the actual allocator; a different global allocator would move the block counts of §5.3.4 (though not the op-stack arithmetic, which is $`16{,}384 \times 32`$ regardless).

### 7.3 Workload representativeness

The throughput verdict of §5.4.1 is weighted by a **measured** distribution — 1,773 datums from five interpreter suites, 95.43 % at depth 2 — and that is far better than a uniform sample. But it is five *test* suites, not a production node, and it measures only `ListParWithRandom::stable_hash_bytes`'s datum leg. ⚠ A production workload with a different depth profile would move the verdict; the per-depth table of §5.4.1 is provided so a reader with a different distribution can re-weight it.

The deploy ceilings of §5.5 are measured on **synthetic maximally-nested** source. That is the right shape for a *ceiling*, and the wrong shape for an average.

### 7.4 Enumeration completeness

The traversal inventory is **derived by three methods** — Tarjan over the schema, a call-graph search over 4,503 functions with an explicit trait-dispatch pass, and measurement as the discriminator of last resort — and each has a blind spot that was hit at least once:

* Tarjan over `RhoTypes.proto` **cannot see a recursive Rust type that is not a proto message**; it missed `score_tree::Tree<T>` and its five traversals (§5.1.4).
* A `#[derive]` scan **cannot see hand-written impls**; `PartialEq` and `Hash` are stripped from prost's output and written by hand, and a hand-picked list of four **already missed `Hash` once** (§5.3.1). The generated registry therefore declares itself a **lower bound**.
* A call-graph search cannot see rustc's **implicit `Drop` glue**, which has no `impl` anywhere.

$`\Rightarrow`$ **The list of $`\Theta(d)`$ traversals in this system should be read as a lower bound, not a census.** The read-ceiling registry (§5.7.8) exists for exactly this reason: it is a **scan**, so a fifth site cannot be added without someone deciding what it is.

### 7.5 DERIVED-but-not-MEASURED claims

Named, as required:

1. **Consensus neutrality of the ingress repair** (§5.5.3(a)) rests on three *read* arguments — signature over source, storage of source, re-normalisation by the proposer. Individually sufficient, jointly strong, **not executed** as an end-to-end differential.
2. **`prost_encode`'s dormancy** (§5.3.5) is a mechanical `grep` over `src/` trees at one commit; it is DERIVED, and a future wiring would silently invalidate the "no stack-safety regression" reading.
3. **`RECURSION_LIMIT = 100`** is read from `prost-0.13.5/src/lib.rs:30`. The workspace `Cargo.lock` lists prost 0.12.6, 0.13.5 **and** 0.14.3; the *effective* version for `models` was not separately confirmed for this report, though the derived $`D_{\max}`$ values (33/32/31) **were** measured end-to-end by the gate and agree with the formula.
4. **The 87-member lowering component** (§5.6.1) is a Tarjan result from a script, not re-run for this report.
5. **The claim that no `codegen-backend = "cranelift"` is configured** is a grep over three file classes; a workspace-external `~/.cargo/config.toml` override was **not** checked.
6. **The attribution of the `env_get_deploy` drift** to the four intervening commits is explicitly *not* asserted (§5.9, #6).

### 7.5a ⚠★ A measurement's provenance includes its BUILD OVERLAY — and this repo's overlay is deliberately untracked

**DERIVED**, `Cargo.toml:59-69`. This repository carries a `[patch]` block marked **`HELD LOCAL — DO NOT COMMIT`** pointing the `rholang-rs` parser crates at an **unpublished** local worktree commit — the one carrying `Proc::SignedTerm` / `TokenStack` / `Bind::Signed` that the `cost_accounting` normalizers require — and `Cargo.lock` is held out of git with `git update-index --skip-worktree`.

$`\Rightarrow`$ ★★ **An archived HEAD does not compile, by design.** `git archive` strips the overlay, and a sibling agent concluded *"HEAD is broken"* from exactly that. Anyone pinning a baseline for comparison must:

1. copy the held-local `Cargo.toml` into the export;
2. ⚠ note that its `path =` dependencies are **relative**, so the export must sit as a **sibling** of the target worktree or the paths must be rewritten absolute; and
3. **verify the baseline builds before attributing any figure to it.**

⚠ In `mettail-rust` the mirror hazard is worse in one respect: the build pin lives in a **harness-wipeable scratchpad** and can vanish on its own, intermittently breaking every build with no local change to explain it.

$`\Rightarrow`$ **A B/level figure is a property of (source, toolchain, profile, *overlay*).** This report's figures were taken with the overlay in place; a re-run that strips it will not reproduce them, and will fail to build rather than disagree.

### 7.5b ★★ When a premise looks refuted, suspect the instrument ONCE before suspecting the claim

Three of this session's apparent refutations turned out to be instrument problems, not findings:

| apparent refutation | what it actually was |
|---|---|
| *"the subjects do not exist"* — the probe rejected all eleven names | **zsh did not word-split `$ SUBJ`**; every name arrived as one argument. The exact hazard `e72dcae8` records ("a zsh `set --` that does not word-split"), reproduced by me one session after reading it. |
| *"the generated tree is missing `ast_enums.rs`"* — the build failed outright | a **concurrent agent was regenerating** `target/generated/` at that moment. One retry, one minute later, succeeded unchanged. |
| *"`models` does not compile"* — E0425 in `sorter/sort_drive.rs` | a **concurrent agent's mid-edit** in the sibling worktree. Also fixed by retrying. |

★ And the general form of the lesson is already in the report twice — [§5.6.3](#563--a-retracted-claim--the-parser-is-not-depth-independent-1339c1e2-then-6275b0e2)'s parser read *"0 B/level"* because the ladder never cleared its own intercept, and [§5.7.3](#573-the-harness-prerequisite-that-was-totally-vacuous)'s generator quantified over **two values** for every input. Both looked like results.

$`\Rightarrow`$ **The rule adopted: a premise that looks refuted buys the instrument exactly one audit before the claim is doubted.** Cheap to apply, and it would have saved three false conclusions in this session alone. ⚠ It is *one* audit, not unlimited — the opposite failure, explaining away a real result as instrument error, is how [§5.5.2](#552--a-withdrawn-claim-reported-as-a-result)'s 84.3 B/level survived as long as it did.

### 7.6 The instrument shares the codebase it measures

The gate's fixtures build and tear down terms using `par_children::dismantle` — production code that the campaign itself introduced. A defect in `dismantle` would corrupt both the subject and the measurement. This is mitigated by the synthetic controls (§4.6, §5.7.4), which are self-contained and independent of `par_children`, and by the differential oracles, which are compiler-generated. It is not eliminated.

---

## 8. Residuals and future work

### 8.1 The prost network format — the largest residual

**MEASURED (f)**: the prost **encoder** is `TRIPWIRE_DEPTH` member `encode` at **302 B/level** release (**1,937 debug**), giving $`D_{\max} \approx 6{,}900`$ on a 2 MiB worker. **MEASURED (f)**: the prost **decoder** is **4,096 B/level** release — the most expensive traversal in the family — but is *capped first* by `RECURSION_LIMIT`, at term depth **33 / 32 / 31** per envelope.

⚠ **The commissioning brief's figure of "28 `merge_field` call sites" could not be reproduced.** `merge_field` is generated by `#[derive(::prost::Message)]` at compile time and appears **zero** times in the generated `rhoapi.rs` text; what *is* countable there is **57 `::prost::Message` derives and 5 `::prost::Oneof` derives**, of which the recursive SCC has **37** members. The residual is therefore stated as *57 generated message codecs, 37 of them in the recursive component, all recursive on both sides* — see Appendix D.

Neither side of prost is converted, and §6.5 records why the choice is not this campaign's to make.

### 8.2 The derived `Drop` — REFUTED by measurement, and it stays a residual

`drop_in_place::<Par>` is **144 B/level** release / **464 debug** (**MEASURED (f)**), and it is the residual ceiling behind several other results — the evaluator's ~75k–100k limit (§5.2.1), the mettail binary's unexplained ~5,100 B/level (§5.6.4), the `normalize_drop` composition (§5.5.6).

★★ **The obvious repair — `impl Drop for Par` with an iterative body — is REFUTED, and the refutation is a measurement rather than a preference.** `Drop` therefore stays on `par_children::dismantle_all` at the call sites, and `par_drop` stays in `TRIPWIRE_DEPTH`. ⚠ This is the one place where the Stage F-4 programme of [§8.3](#83--par-as-cloneclone--the-largest-unconverted-traversal-after-prost_de) does **not** reach: a generated `Clone` is a *method* the generator can emit, whereas `Drop` would have to be an `impl` on a type whose fields are moved out by 61 existing call sites. **MEASURED (q)**, `44535d75`: adding the impl and compiling produced **353 diagnostics across 61 unique source lines in `models` alone** — 38 struct-literal / functional-record-update, 23 partial move, **0 destructure**. ★ The design's premise was **wrong about the syntax**: it counted `let Par { … }` destructuring, of which there are **zero**; E0509 here is driven by `Par { …, ..Default::default() }` and by partial field moves. ⚠ **And the count is a floor** — cargo aborted at `models`, so `rholang`, `rspace++`, `casper` and `node` were never checked, *including the very files the design named*. The impl was reverted and verified reverted.

Call-site interception is measurably incomplete as an alternative: `Compiler::normalize_term` dismantles its intermediate and **returns the sorted term to a caller that does not**.

### 8.3 ★★ `<Par as Clone>::clone` — the largest unconverted traversal after `prost_de`

**It was never converted, no commit anywhere converts it, and the task tracker reads as though one did.** The full evidence is [§5.10](#510--the-generated-trait-implementations-and-the-clone-question); the residual is stated here so it sits beside the prost paths where it belongs.

**MEASURED (f)**, 2026-07-29, twice in one release binary: **3,254 B/level** — the **second-worst row of the eight-traversal S0 baseline**, behind only `prost_de` at 4,096, and above `debug` (1,243), `ord` (438), `prost_ser` (302), `eq` (221), `par_drop` (144) and `hash` (136). **DERIVED** extrapolation: $`D_{\max} \approx 640`$ levels on a 2 MiB worker (⚠ unbisected — [§5.9](#59-measurements-that-could-not-be-obtained) #9).

★★ **STATUS CHANGED: it is now SCHEDULED, not declined.** The ruling since this section was first written is that **all modelled types get derived impl methods through the SAME stack-safe driver as the derived ser/de**. `<Par as Clone>::clone` **is being converted** — **Stage F-4** of an approved eight-stage programme, filling the deliberately-empty `emit_term_ops_source()` slot in `models/build/wire_schema.rs` (the slot [§5.3.2](#532-the-cold-store-encoder--a-single-walk-trampolined-serializer-c28f4cf6-a169cc61) records was emitted, included as a module and left empty precisely so the pipeline a later stage fills was exercised from the stage that built it). $`\Rightarrow`$ The generator that already emits the bincode and prost tables from one schema walk will emit the term-op drivers too, so the 57-hand-written-impls objection below is answered by **generating** them rather than writing them.

**Why it was not converted EARLIER** — the reasons were real, and they are what the programme now routes around:

* Its standing disposition from the first audit is *"derived impl — **Leg-1 only: remove the call sites, not the impl**"*, and that programme was executed: [§5.5.3(c)](#553-the-three-repairs) removed **15** call sites, which is what moved `plain_deploy` by $`23.9\times`$, and [§5.5.3(b)](#553-the-three-repairs) removed the `inj_attempt` one, which is what `9082d12c` did.
* Converting the impl itself means **replacing rustc's derive with a hand-written iterative `Clone`** on a prost-generated type — the same E0509-adjacent surgery [§8.2](#82-the-derived-drop--refuted-by-measurement-and-it-stays-a-residual) documents for `Drop`, on 57 message types, against a derive that regenerates from the schema on every build.
* ★★ **The strategy actually in force is CALL-SITE ELIMINATION, not conversion** ([§5.10.5a](#5105a--the-strategy-is-call-site-elimination-not-impl-conversion--and-it-should-be-argued-not-inferred)). Deleting a call saves the traversal **and** the allocation, where a converted clone would still copy the term. Two sites are gone and measured; the residual is **which callers remain**, and that set is ⚠ **unenumerated on purpose** — hand-enumeration is this campaign's most-repeated failure class ([§7.4](#74-enumeration-completeness)). **Named work:** derive it with a call-graph *scan* in the idiom of the read-ceiling registry ([§5.7.8](#578-the-read-ceiling-registry-and-a-fifth-site-it-can-detect)), never a hand-written list.
* ★ **The conversion technique is already in this repository and in the converted register**: `tree_clone`, a hand-written iterative `Clone` for `score_tree::Tree<T>`, **1,578 / 485 $`\rightarrow`$ 0** ([§5.10.5b](#5105b--the-conversion-technique-is-in-tree-and-proven--it-was-simply-never-applied-to-par)). What blocks the same shape on `Par` is that `Tree<T>` is hand-written source while `Par`'s `Clone` is rustc's derive over prost output — it would mean hand-writing and schema-tracking `Clone` for **57** message types.
* ⚠★ **The representation route — `mettail`'s Arc fix (`SS-G1`) — is CLOSED for `Par`, for three independent and individually sufficient reasons** ([§5.11.6](#5116--why-it-does-not-transfer-to-f1r3nodes-par--three-independent-reasons)): `Par`'s recursion runs through **`Vec<T>`**, so element copies survive any wrapper change; `Par` is **mutated in place**, with **38** functional-record-update sites in `models` alone; and `prost-build`'s `Config` has **`boxed(path)` and no `arc` equivalent**, with the generated `Message` impl written against concrete field types. **$`\Rightarrow`$ The traversal route is the one being taken.**

**The other tripwire members**, release B/level: `substitute_deep_binding` **7,460** — `Env::get`'s copy is *semantically required*, because the copy **is** substitution ([§5.1.5](#515-the-named-residual-of-family-a)); `sort_nested_map` **7,509**; `sort_nested_set` **4,778**; `clone_nested_set` **4,096**, the derived floor the two sorter arms are measured against.

### 8.4 Not measured, and worth measuring

The heap cost of the four non-codec conversions (§5.9 #3). Each moved $`\Theta(d)`$ state from stack to heap and **none has a heap profile**. Given that the codec conversions turned out to need 5–42 $`\times`$ *fewer* bytes per level in heap than they had used in stack, the analogous figures for substitution — whose frames were **195 kB per level** in debug — would be the most interesting number in the campaign, and it does not exist.

### 8.5 The `mettail-rust` residue

`render` 3,665 / 911 and `lower_formula` 4,094 / 978 B/level (debug / release), each with its own gate subject and named owner.

> ★ **SUPERSEDED in part, wording kept.** This paragraph previously ended: *"; `ast_drop` at 270 / 96, blocked on the cross-type hop (§5.6.1)."* **`ast_drop` is converted** — it is `Shape::Flat` in `EXPECTED_DRIVER_SHAPE` at `mettail-rust@7fad51db`, converted by `a21b0bf9` (#162). The *cross-type hop* was the real mechanism and it was **repaired at the classifier**, not routed around; see [§8.6.1](#861--162189--the-eleven-generated-drivers-converted-and-the-root-cause-that-unifies-154-with-162). `render` and `lower_formula` are unaffected and remain live.

---

## 8.6 ⚠★★★ THE OPEN RESIDUAL REGISTER — what this report does NOT establish

**This section exists because the rest of the document reads as a completion report and the work is not complete.** Everything above is true; it is not *all* of the truth, and a results document whose advertised coverage exceeds its real coverage is worse than none — the same standing finding this campaign applied to its gates ([§5.7](#57-family-e--the-instrument-and-what-it-caught-in-itself)) now applied to itself.

**Every coordinate below is pinned at a fixed commit SHA (Secure Hash Algorithm digest — a git object name), never at `HEAD`.** `at = "HEAD"` citations go stale; this campaign burned roughly twenty line-number re-pins before adopting the rule. The two pins in force are **`mettail-rust@7fad51db`** and **`f1r3node-rust-mettail@8bf298ba`**.

**Provenance tags** are the document's own ([§Evidentiary convention](#evidentiary-convention)), with one addition made explicit here: **read from source** is a sub-case of **DERIVED** in which the evidence is the text of a named file at a named SHA, and it is used heavily below because most of these residuals are *absences* — a missing repair is established by reading the tree, not by running it.

⚠ **No figure in this section was re-measured for this revision.** Nothing here required a build: every claim is **read from source**, **DERIVED** from a commit body, or **MEASURED (q)** — quoted from the commit or harness that recorded it, with that origin named. Where the brief commissioning this revision supplied a number, it was checked against source or a commit body before being written, and the three cases where it did not survive that check are in [Appendix D](#appendix-d--corrections-to-the-commissioning-brief).

### The register

| # | residual | status at the pinned SHA | evidence |
|---|---|---|---|
| **#162 / #189** | all eleven generated `ast_*` drivers | ✅ **CONVERTED**, 0 B/level both profiles | [§8.6.1](#861--162189--the-eleven-generated-drivers-converted-and-the-root-cause-that-unifies-154-with-162) |
| **#197** | the optional-collection shape inside #162's own commit | ⛔ **live and unrepaired** — blocks `--all-targets` repo-wide | [§8.6.1a](#861a--197--the-defect-inside-162s-own-commit-live-at-head) |
| **#43** | f1r3node's hand-written `Par` traversals | ⚠ **21 converted, 8 tripwire members REMAIN**; **NOT** superseded by #162/#189 | [§8.6.2](#862--43--f1r3nodes-hand-written-par-traversals-8-tripwire-members-remain) |
| **#124** | the three `spliced_event_bytes` event-hash legs | ⚠ **MEASURED, NOT CONVERTED**; repair already implemented and never called | [§8.6.3](#863--124--the-event-hash-legs-measured-not-converted-and-quadratic-in-time) |
| **#189** residual | `try_eval` | ⚠ **PARTIAL** — `Int` has a worklist, **15 categories do not** | [§8.6.4](#864--189-residual--try_eval-is-partially-converted-and-the-ratchet-that-says-so) |
| **#119 / #120** | the prost depth-33 read ceiling | ⚠ **DESIGNED, NOT BUILT** — and naive removal *introduces* a `SIGSEGV` | [§8.6.5](#865--119120--the-prost-read-ceiling-and-the-trap-in-removing-it) |
| **#174** | 10,491 B/level of parse-phase cost | ⚠ **UNATTRIBUTED** — matches no driver measured in isolation | [§8.6.6](#866--174--10491-blevel-that-belongs-to-no-measured-driver) |
| **#157** | the ceiling inventory's transcribed value | ⚠ **STALE AND STRUCTURALLY INVISIBLE** — the tripwire cannot see the drift | [§8.6.7](#867--157--a-transcribed-ceiling-and-a-tripwire-that-cannot-see-it-drift) |
| **instrument** | the 12,288 B floor; the 3,254 B/level shape | ⚠ **two method corrections**, one repaired in only one of the two repositories | [§8.6.8](#868--the-instrument-floor-and-the-wrong-shape--two-corrections-that-change-the-method-not-just-a-number) |
| **#121** | `eval_stable_par` ⇄ `eval_stable_expr` | ✅ converted — ★ found by **bisection**, present in **no audit and no tripwire list** | [§8.6.9](#869--121--the-gate-built-to-demonstrate-a-fix-overflowed-and-the-frame-was-not-the-encoder) |

---

### 8.6.1 ★★ #162/#189 — the eleven generated drivers converted, and the root cause that unifies #154 with #162

**MEASURED (q)**, `mettail-rust` #162 (`fab6de24`, `6e4abbd8`, `a21b0bf9`, `dc104aa3`, `4ee48db9`) and #189 (`844364d2`). **Read from source** at `mettail-rust@7fad51db`, `rholang-runtime/tests/stack_depth_gate.rs`, `EXPECTED_DRIVER_SHAPE`: every one of the eleven carries `Shape::Flat`.

| driver | $`B_0`$ (debug) | $`B_1`$ | note |
|---|---:|---:|---|
| `ast_cmp` | 10,590 | **0** | |
| `ast_debug` | 10,542 | **0** | |
| `ast_eq` | 6,144 | **0** | |
| `ast_match_pattern` | 6,136 | **0** | free via `cmp` — one repair, two subjects |
| `ast_term_depth` | 3,408 | **0** | |
| `ast_is_ground` | 2,225 | **0** | see the proof below |
| `ast_hash`, `ast_semantic_hash`, `ast_drop`, `ast_subst`, `ast_normalize` | — | **0** | |

★★ **THE ROOT CAUSE, and it is the pedagogical centre of this whole report.** `VariantKind::CollectionLiteral` was introduced (`5bdd0a24`, an *identity* refactor) precisely so that consumers would **declare** whether a variant is a leaf or a container. **Every sloped driver was one that still shared `Literal`'s arm** — that is, one **treating a container OF SUB-TERMS as an OPAQUE LEAF**. The leaf arm reaches children by host recursion, which is correct for a `Literal` (it has none) and is a $`\Theta(d)`$ defect for a `Vec<Proc>`.

$`\Rightarrow`$ **#154 and #162 are ONE defect seen from two angles**, and this is why neither could be fully fixed without the other:

* **#154** met it as a **correctness** leak — a binder inside a collection literal leaked its run-varying `unique_id` into a *consensus-visible* fingerprint (`6e4abbd8`);
* **#162** met it as a **stack-safety** slope — the same arm, the same containers, $`\Theta(d)`$ native stack.

⚠ The interaction is recorded in the commits themselves rather than inferred: `dc104aa3`'s subject is *"the #154 fix traded a leak for a slope; both are now held"* (**DERIVED**, commit body). A repair aimed at one axis moved the other, in both directions, which is the signature of a shared cause.

![The `CollectionLiteral` vs `Literal` arm divergence](figures/collection-literal-arm-divergence.svg)

*Figure 8: the arm divergence that was the root. Source: [`figures/collection-literal-arm-divergence.puml`](figures/collection-literal-arm-divergence.puml). **Diagram type: a dispatch-divergence diagram (annotated decision tree)**, chosen because the defect is not a wrong computation but a wrong **arm selection** — one classifier, two consumers, and the consumers disagreed about which arm a container belongs to. A decision tree is the only shape that shows an arm being *taken* rather than a value being computed.*

#### Lesson 1 — per-element pushes were NOT sufficient, and the task enum is why

★★ **A work-stack driver can only replace recursion for work its task enum can represent.** `CmpTask` and `HashTask` held only *descents*: "compare these two pointers". Neither could express *"resume this comparison once the child has answered"*, which is exactly the state a container needs when its elements must be compared in order and the first inequality decides the result. So "push the elements" was **not expressible** until the enum itself grew a resumption variant.

$`\Rightarrow`$ That is why those two emitters carry roughly **130** and **60** lines of the author meeting the same wall (**DERIVED**, `fab6de24` diff extent on `iterative_cmp.rs` and the hash emitter). **The task enum is the real interface of a converted driver, not the push site** — and a conversion plan that budgets only for "add a push" will under-budget by the size of the enum extension.

#### Lesson 2 — `ast_is_ground`'s before-figure would have been a production `SIGSEGV`

**DERIVED** from the recorded ladder: `ast_is_ground` at **2,225 B/level** reaches

```math
2{,}225 \times 4{,}096 \;=\; 9{,}113{,}600\ \text{B} \;\approx\; 8.69\ \text{MiB}
```

at depth 4,096, which **exceeds the 8 MiB default main-thread stack** (`ulimit -s` 8,192 KiB $`=`$ 8,388,608 B). $`\Rightarrow`$ It would not have degraded; it would have **`SIGSEGV`**'d — signal 11 on the guard page, surfacing as `SIGABRT`/134, uncatchable, taking the node process (see the glossary entry for **`SIGSEGV` vs `SIGABRT`**).

★ **The conversion carried a proof, and the proof is why it was cheap.** Let $`\mathrm{desc}(n)`$ be the descendants of node $`n`$ and $`\mathrm{base}(m)`$ the ground-ness of $`m`$'s own payload. Then

```math
g(n) \;=\; \bigwedge_{m \,\in\, \mathrm{desc}(n)} \mathrm{base}(m)
```

This is a **conjunction over a set**, and three properties follow immediately, each of which removes a piece of machinery a general driver would need:

| property of $`\bigwedge`$ over a set | machinery it removes |
|---|---|
| **associative** | no *result stack* — partial results need no nesting structure |
| **commutative** $`\Rightarrow`$ order-agnostic | no `dist`, and no need to preserve child order |
| **idempotent**, with identity $`\top`$ | no *combine* step — a single accumulator suffices |

$`\Rightarrow`$ The driver is an accumulator over a flat worklist, is `OrderAgnostic`, and therefore **every container shape converts uniformly** — `Vec`, `Option<Vec>`, keyed maps alike. Contrast `cmp`, where the operation is *lexicographic* and hence **neither** commutative nor idempotent, which is precisely why `cmp` needed the resumption variant of Lesson 1 and `is_ground` did not.

#### The conversion algorithm, in literate form

Presented in Knuth's literate style: the prose *is* the specification, and each fragment is named and refined.

**Algorithm 4 (CONVERT-TRAVERSAL).** *Turning a derived recursive traversal into a work-stack driver, with the driver's shape DERIVED from the algebra of its combining operation rather than chosen by taste.*

```pseudocode
⟨Convert one derived traversal to a work-stack driver⟩ ≡
    ⟨Classify every variant: leaf, or container-of-sub-terms⟩
    ⟨Choose the driver shape from the combining operation's algebra⟩
    ⟨Extend the task enum until every unit of work is representable⟩
    ⟨Emit the driver: one pop, one dispatch, zero self-calls⟩
    ⟨Prove flatness against a control that is NOT flat⟩

⟨Classify every variant: leaf, or container-of-sub-terms⟩ ≡
    for each variant v of the modelled type:
        kind(v) ← CollectionLiteral   if v holds a collection of sub-terms
                  Literal             if v holds no sub-terms at all
                  Structural          otherwise
    ── THE DEFECT LIVED HERE: a CollectionLiteral falling through to
    ── Literal's arm is the whole of #154 and #162.  The classifier must be
    ── consulted, never re-derived at the use site.

⟨Choose the driver shape from the combining operation's algebra⟩ ≡
    let ⊕ be the operation the traversal folds with
    if ⊕ is associative ∧ commutative ∧ idempotent then
        shape ← ACCUMULATOR          ── is_ground: no result stack, no combine
    else if ⊕ is associative only then
        shape ← OBLIGATION + VALUE STACK   ── cold-store decoder: 18 value stacks
    else
        shape ← RESUMABLE TASKS      ── cmp/hash: lexicographic, needs Lesson 1
    ── The algebra of ⊕ DETERMINES the machinery.  This is the step that makes
    ── the conversion derivable rather than inventive.

⟨Extend the task enum until every unit of work is representable⟩ ≡
    repeat
        w ← a unit of work the driver must perform
        if w is not expressible as a task variant then
            add the variant                ── Lesson 1: this is the real cost
    until every w is expressible
    ── A push site cannot be written before the enum can spell what it pushes.

⟨Emit the driver: one pop, one dispatch, zero self-calls⟩ ≡
    push(root)
    while stack is not empty:
        t ← pop()
        dispatch on t:
            leaf         ⇒ fold its payload into the accumulator
            container    ⇒ for each element e: push(task(e))    ── NOT recurse(e)
            resumption   ⇒ combine the child answers already available
    ── INVARIANT: the driver contains no call to itself.  This is mechanically
    ── checkable and is what `EXPECTED_DRIVER_SHAPE` adjudicates.

⟨Prove flatness against a control that is NOT flat⟩ ≡
    assert slope(subject) = 0                    over the ladder d_lo → d_hi
    assert slope(control) > 0                    ── ANTI-VACUITY, and it is the
                                                 ── only leg that makes the first
                                                 ── assertion mean anything
    ── §8.6.1b: with all eleven converted, the sloped set went EMPTY, and an
    ── empty sloped set makes the classifier itself vacuous.
```

#### 8.6.1b ★ The conversion emptied the sloped set, which was itself a hazard

⚠ **When #162 converted the tenth driver, the sloped set became EMPTY** — and an empty sloped set means `measured_shape` could return `Flat` for *every* subject even if the classifier were broken, so the whole partition would pass **vacuously**. The prior calibration anchors were `ast_drop` at 94 B/level and then `ast_term_depth` at 207; **both were converted, so the calibration lost its anchor** (**read from source**, `mettail-rust@7fad51db`, `rholang-runtime/tests/stack_depth_gate.rs`, the comment block above `ast_recursion_control`).

★ The repair is a **deliberately unconvertible control**: `ast_recursion_control`, a host-recursive walk of the same `CastList`/`ListLit` ladder, owned by `stack_depth_probe.rs`, carrying `Shape::Sloped` and **never to be converted**. It measures nothing about the generated drivers; it proves the **classifier can still tell the two shapes apart**. $`\Rightarrow`$ *Censusing the classifier, not the artefact* — a census over generated output is blind to what the generator declined to generate.

---

### 8.6.1a ⛔★★ #197 — the defect inside #162's own commit, LIVE at HEAD

**This is the single most important entry in this section**, because it is a defect *in the fix* that the rest of §8.6.1 reports as a success.

**Read from source**, `mettail-rust@7fad51db`: **`fab6de24` is still the most recent commit to `macros/src/gen/term_ops/iterative_cmp.rs`** (`git log --oneline -- macros/src/gen/term_ops/iterative_cmp.rs` heads with it, followed by `b0027f61`, `5bdd0a24`). **No repair exists.**

`fab6de24` — **#162's own commit** — mishandles the **optional-collection** shape, `Option<Vec<Proc>>`:

| diagnostic | what the generated code does | why it cannot compile |
|---|---|---|
| **E0624** | calls `Option::len` | `len` is not a public method of `Option`; the length belongs to the *inner* `Vec`, reachable only after the `Option` is destructured |
| **E0606** | casts `&Vec<Proc>` as `*const Proc` | a `&Vec<T>` is not a `&[T]`: casting the container reference to an element pointer is an invalid primitive cast, not a deref coercion |

**Both are the same underlying slip**: the emitter reached for the *container's* accessor while holding the *`Option`'s* reference — a variant of the very confusion §8.6.1 is about, one level up. Where the root defect treated a container as a leaf, this treats an `Option`-wrapped container as a bare container.

⚠ **Blast radius, DERIVED from the failing targets:** `promoted_corpus_class3opt`, `class3_opt_smoke`, `display_roundtrip_regression_tests`, and — because a proc-macro that fails to expand fails every crate that depends on it — **`--all-targets` repo-wide**.

$`\Rightarrow`$ **The honest reading of §8.6.1 is therefore: the eleven conversions are real, measured, and correct in the shapes they cover, and the repository that reports them cannot currently build `--all-targets`.** Those two facts are both true and the second does not cancel the first — but a reader who is told only the first has been misled about the state of the tree. ★ This is also why lints are last in this campaign ([#86](#appendix-a--reproduction-commands)) rather than a matter of taste: with `--all-targets` blocked, a lint pass could not run even if it were wanted.

---

### 8.6.2 ⚠★★ #43 — f1r3node's hand-written `Par` traversals: 8 tripwire members REMAIN

**Read from source**, `f1r3node-rust-mettail@8bf298ba`, `rholang/tests/stack_depth_gate.rs`: `CONVERTED_DEPTH` holds **15** entries and `CONVERTED_WIDTH` holds **6**, so **21 traversals are converted**; `TRIPWIRE_DEPTH` holds **8**, and `TRIPWIRE_WIDTH` is **empty**.

The eight, with their release slopes:

| member | B/level | why it is still here |
|---|---:|---|
| `subst_and_charge` | **146** | the metered wrapper; `encoded_len` remains. An $`19.5\times`$ improvement that is correctly **not** a class change (`SS-D5`) |
| `substitute_deep_binding` | **7,460** | ★ **now the largest tripwire member.** `Env::get`'s copy is *semantically required* — the copy **is** substitution. Its `HashMap<i32, Par>` walk is a **different traversal** from the one stage F-4 converted |
| `par_drop` | **144** | the derived `drop_in_place::<Par>`; the iterative-`Drop` repair is **REFUTED by measurement** ([§8.2](#82-the-derived-drop--refuted-by-measurement-and-it-stays-a-residual)) |
| `normalize_drop` | — | the deploy composition: **flat in debug, sloped in release** |
| `encode` | **302** | the prost encoder ([§8.6.5](#865--119120--the-prost-read-ceiling-and-the-trap-in-removing-it)) |
| `sort_nested_set` | **4,778** | Stage C-2 residual, self-contained set arm |
| `sort_nested_map` | **7,509** | Stage C-2 residual, self-contained map arm |
| `clone_nested_set` | **4,096** | the derived floor the two sorter arms are measured *against* |

⚠★★ **#43 IS NOT SUPERSEDED BY #162/#189, and the conflation is easy enough that it nearly happened.** The two are disjoint on every axis that matters:

| axis | #43 | #162 / #189 |
|---|---|---|
| repository | `f1r3node-rust-mettail` | `mettail-rust` |
| subject family | **hand-written** `Par` traversals | **generated** `ast_*` drivers |
| gate file | `rholang/tests/stack_depth_gate.rs` | `rholang-runtime/tests/stack_depth_gate.rs` |
| adjudicating constants | `CONVERTED_DEPTH` / `CONVERTED_WIDTH` / `TRIPWIRE_DEPTH` | `EXPECTED_DRIVER_SHAPE` |
| what a fix touches | a `.rs` file in `models/` or `rholang/` | an **emitter** in `macros/src/gen/` |

$`\Rightarrow`$ **No commit in either repository can discharge the other's residual**, because no gate in either repository can *see* the other's subjects. ★ The mitigation already in the document is that cross-repository register rows name their source gate ([Appendix G.3](#g3-what-the-check-deliberately-does-not-cover-and-the-residual-risk)); the mitigation this section adds is that the two families are now drawn apart explicitly, so a future reader cannot merge them by resemblance.

![Converted vs tripwire across both repositories](figures/converted-vs-tripwire-cross-repo.svg)

*Figure 9: the cross-repository status map. Source: [`figures/converted-vs-tripwire-cross-repo.puml`](figures/converted-vs-tripwire-cross-repo.puml). **Diagram type: a package diagram partitioned by repository and by gate constant**, chosen over a flat table because the load-bearing fact is a **containment** one — the two repositories are adjudicated by different constants in different files. Packages make the disjointness structural; a table would let a reader slide the two families together, which is exactly the error the section warns about.*

---

### 8.6.3 ⚠★★ #124 — the event-hash legs: MEASURED, NOT CONVERTED, and quadratic in TIME

**Read from source**, `f1r3node-rust-mettail@8bf298ba`, `casper/tests/event_hash_leg_depth_probe.rs` and `models/src/rust/spliced_event_bytes.rs`.

Three legs of the event hash are still $`\Theta(d)`$ in native stack. **MEASURED (q)** from that probe:

| leg / path | debug | release |
|---|---:|---:|
| **direct** (`bincode::serialize`, the 95.43 % case) | **3,040** | **160** |
| **spliced** (`emit_<leg>`, hand-written) | **1,008** | **208** |
| ★ the **`*-cold` control** (`ColdStoreEncode::cold_encode`) | **0.0** | **0.0** |

★ **The control reading 0.0 in both profiles is what makes the other four numbers mean anything** — the slope is the *leg's own*, not the harness's or the fixture's.

**⚠ The stake is consensus liveness, not robustness.** The event hash reaches consensus twice over: it is the RSpace event identity carried into `ProcessedDeploy::deploy_log` $`\rightarrow`$ `Body.deploys` $`\rightarrow`$ the **block hash**, and it is the key the replay space rigs against (`ReplayRSpace::rig`).

★★ **The repair is nearly free, and that is the finding.** `ColdStoreEncode` is **already implemented for all three legs' root types** — `Par`, `BindPattern`, `ListParWithRandom`, `TaggedContinuation`. The legs simply never started calling it: `direct()` is still `bincode::serialize`. The **channel** leg *was* routed through the new encoder (`00ff9187`); **these three were not.** $`\Rightarrow`$ This is not an unsolved problem; it is a **solved problem with three un-migrated call sites**, which is a materially different (and cheaper) residual than the prost one.

#### ★ And the spliced path is $`\Theta(d^2)`$ in TIME

`contains_par` is the dispatch scan that decides whether any filled `EPathMap` cell exists below a node. To answer **false** — the 95.43 % case — it **must visit every descendant**: absence admits no short circuit. And it is re-invoked at **every level**. Summing the descendants visited over all levels of a chain of depth $`d`$:

```math
\sum_{i=1}^{d} (d - i) \;=\; \frac{d\,(d-1)}{2} \;=\; \Theta(d^2)
```

$`\Rightarrow`$ **A time cost, distinct from and additional to the stack cost**, and one that no B/level figure anywhere in this report would reveal, because slope measures *stack per level* and is blind to *work per level*. ★ It is worth stating as a general caution: **a campaign that measures only stack will not notice a quadratic it introduced**, and this one did not notice this until the leg was read rather than measured.

![The $`\Theta(d^2)`$-in-time spliced walk](figures/spliced-walk-quadratic-time.svg)

*Figure 10: the spliced walk's quadratic time. Source: [`figures/spliced-walk-quadratic-time.puml`](figures/spliced-walk-quadratic-time.puml). **Diagram type: a sequence diagram with an explicit per-level repetition frame**, chosen because the quadratic cost is a fact about **call multiplicity over nesting levels** — `contains_par` is re-invoked once per level and each invocation descends the whole remaining subtree. A sequence diagram is the only type that makes "once per level, each descending everything" legible; a structure diagram would show the term but not the repetition.*

---

### 8.6.4 ⚠ #189 residual — `try_eval` is partially converted, and the ratchet that says so

**Read from source**, `mettail-rust@7fad51db`, `macros/src/gen/native/eval.rs:1259` (`pub fn try_eval`). `try_eval` is **partially** converted: the **`Int` category has a worklist; fifteen categories do not.**

⚠ **A partially converted traversal is the most dangerous shape in this whole report**, and it deserves saying plainly: it presents a *converted* name and a *converted* commit message, and it is flat on exactly the ladder its own subject exercises. The fifteen unconverted categories are invisible to a gate whose subject only walks `Int`.

★ **The instrument that keeps this honest is a ratchet, not an enumeration**: `UNMEASURED_TRAVERSALS = 7` (**read from source**, `mettail-rust@7fad51db`, `rholang-runtime/tests/stack_depth_gate.rs:1492`). It is a **live ratchet** — a pinned count of the known-unmeasured population, so the population cannot grow silently. It does not claim to name its members; that is the point. Where enumeration is itself the failure mode ([§7.4](#74-enumeration-completeness)), a count that must be edited downward is the honest instrument.

---

### 8.6.5 ⚠★★ #119/#120 — the prost read ceiling, and the trap in removing it

The protobuf **reader** is capped at term depth **33 / 32 / 31** per envelope by a private `prost` constant, while the **writer** has no cap at all — the *write/read asymmetry* of [§6.4](#64--the-writeread-asymmetry--one-section-because-it-is-one-class). The redesign is **designed, not built**.

⚠★★★ **The trap, and it is the reason "just raise the limit" is wrong.** **Read from source**, `prost-0.14.4/src/encoding.rs:178–189`: `skip_field`'s `StartGroup` arm **recurses**.

```text
WireType::StartGroup => loop {
    let (inner_tag, inner_wire_type) = decode_key(buf)?;
    match inner_wire_type {
        WireType::EndGroup => {
            if inner_tag != tag {
                return Err(DecodeErrorKind::UnexpectedEndGroupTag.into());
            }
            break 0;
        }
        _ => skip_field(inner_wire_type, inner_tag, buf, ctx.enter_recursion())?,
    }
},
```

*(**VERBATIM** from `prost-0.14.4/src/encoding.rs`, the `StartGroup` arm of `skip_field`. ⚠ Tagged `text`, not `rust`, and deliberately: a bare `match` arm is **not standalone Rust** and the `code-snippets-valid` checker correctly refuses it. This is the same disposition §E.2 records for two earlier blocks — retagged because the check refused them, recorded rather than quietly accommodated. It compiles in its own crate, where it is quoted from.)*

The `loop` is iterative across *siblings*, but the `_ =>` arm calls `skip_field` **on itself** for a nested group, and the only thing bounding that recursion is `ctx.limit_reached()?` at the function's head plus `ctx.enter_recursion()` on the way down.

$`\Rightarrow`$ **The recursion limit is not merely a *cap on legitimate depth*; it is also the sole guard on an attacker-controlled recursion in the SKIP path** — the path taken for *unknown* fields, which an attacker chooses freely. **Removing the recursion limit without first making group-skip iterative would not lift a restriction; it would INTRODUCE a `SIGSEGV`** on a message the node does not even understand. ★ Note the asymmetry in cost: making the *known* fields iterative is a large schema-driven change, whereas the `StartGroup` arm is one function — so the guard must be replaced *before* the cap is touched, not after.

---

### 8.6.6 ⚠ #174 — 10,491 B/level that belongs to no measured driver

**Read from source**, `mettail-rust@7fad51db`, `rholang-runtime/src/bin/stack_depth_probe.rs:820–821`: bisected debug, ladder $`16 \rightarrow 1{,}024`$, `list_pair_lower` reads **950 B/level** while `map_pair_lower` reads **10,491** — an **11.0$`\times`$** jump from replacing a list literal with a hash-keyed one.

⚠ **The figure matches no driver measured in isolation.** Every generated `ast_*` driver reads 0 ([§8.6.1](#861--162189--the-eleven-generated-drivers-converted-and-the-root-cause-that-unifies-154-with-162)), and no single measured subject accounts for 10,491. $`\Rightarrow`$ The cost is **real, reproducible, localised to the parse/lowering phase of hash-keyed collection literals, and UNATTRIBUTED**.

★ **It is recorded as unattributed rather than apportioned**, because apportioning it would mean assigning a measured total to un-measured parts — the exact move [§5.7](#57-family-e--the-instrument-and-what-it-caught-in-itself) documents as producing false zeros. The honest next step is a *composition* measurement (bisect the lowering phase with each candidate sub-traversal stubbed), not an estimate.

---

### 8.6.7 ⚠★ #157 — a transcribed ceiling, and a tripwire that cannot see it drift

**Read from source**, `f1r3node-rust-mettail@8bf298ba`, `rholang/tests/stack_depth_gate.rs`: `BUILD_DEPTH_INVENTORY` (line 4579) carries `BuildCeiling::Bisected(283)` (line 4586) for `env_get_deploy`. **The tree measures 274** ([§5.5.4](#554-results-and-a-control-that-behaved-exactly-as-predicted)) — a **9-level drift**, in a value the inventory's own source ruled must not be pinned. There are **$`\geq`$ 6** transcribed sites.

⚠★★ **The tripwire is STRUCTURALLY BLIND to this class of drift, and the arithmetic shows why.** The assertion is $`d > \texttt{widest\_read}`$, with `widest_read` derived from `READ_CEILING_ENVELOPES` (line 4629) as **33**. The guarded band is therefore $`[264,\ 297)`$ at the inventory's resolution, and

```math
264 \;\leq\; 274 \;<\; 283 \;<\; 297
```

$`\Rightarrow`$ **283 and 274 both sit inside the band**, so the assertion passes identically for the stale value and the true one. Its **resolution is 33 levels** and the **drift is 9** — the check cannot resolve a difference four times smaller than its own granularity.

★ **The lesson is about instrument design, not diligence.** A check whose resolution is coarser than the drift it is meant to catch is not a weak check; it is **no check at all** for that failure mode, and it will report green forever. The repair is not "look harder" but **derive the value instead of transcribing it** — the same conclusion [§5.7.1](#571-the-register-became-derived-because-every-transcription-drifted) reached when the converted-subject list existed in four places and every copy drifted.

---

### 8.6.8 ⚠★★★ The instrument floor, and the wrong shape — two corrections that change the METHOD, not just a number

These two are in the register because **a reader who adopts this report's method without them will produce wrong numbers with correct-looking provenance.**

#### (1) 12,288 B is an INSTRUMENT FLOOR, and it is provable from the algorithm

`min_stack_for` begins its exponential probe at `PROBE_START = 16 * 1024`. A subject that **survives** that first probe never enters the doubling loop, so the bisection runs on $`[8{,}192,\ 16{,}384]`$; with $`\texttt{RESOLUTION} = 4{,}096`$ and $`16{,}384 - 8{,}192 = 8{,}192 > 4{,}096`$, the loop executes and terminates on its **first** midpoint:

```math
\mathrm{mid} \;=\; \frac{8{,}192 + 16{,}384}{2} \;=\; 12{,}288
```

$`\Rightarrow`$ **12,288 is the smallest value the bisection can ever emit**, for any subject, regardless of what the subject does. **Twelve of fifteen subjects read that same floor — which is ONE artefact, not twelve agreeing measurements.**

★ **The repair makes the floor unspellable.** `min_stack_for` now returns `MinStack{Bytes, BelowResolution}` and renders `<12 KiB (BELOW THE INSTRUMENT FLOOR)` — **never a number** (`mettail-rust@125065a8`, #187; **read from source** at `7fad51db`, `rholang-runtime/tests/stack_depth_gate.rs:334–343`, `371`, `483–494`).

⚠ **What survives the floor and what does not**, because the distinction is the whole practical content: a **slope** is a *difference* of two endpoint readings, so a constant common to both cancels and **slope readings are unaffected** — when both ends read the floor the slope is exactly $`0`$, which is correct. **Endpoint byte values at the floor are not measurements** and must never be divided by. A row reading $`(12{,}288 - 12{,}288)/\mathrm{span} = 0.0`$ looks exactly like a result.

⚠★★ **AND THIS REPAIR IS PRESENT IN ONLY ONE OF THE TWO REPOSITORIES.** **Read from source**, `f1r3node-rust-mettail@8bf298ba`, `rholang/tests/stack_depth_gate.rs`: `fn min_stack_for(name: &str, depth: usize) -> usize` — a **bare `usize`**, starting at `let mut hi = 16 * 1024`. The `MinStack` type does not exist in this repository. $`\Rightarrow`$ **Every endpoint byte figure in this report that came from f1r3node's own gate is still floor-affected**, and this document is the one that must say so, because it is the document that publishes those figures. The slopes stand; the endpoints at 12,288 do not.

★ **The defect neither prior analysis named — and its status is narrower than the diagnosis.** `runs_within` mapped an `execve` refusal to `Err(_) => false`, *the same verdict it gives a genuine overflow*, so the instrument could not distinguish **"needs more stack"** from **"cannot ask"**. **Read from source**, `mettail-rust@7fad51db`, `rholang-runtime/tests/stack_depth_gate.rs:132–180`: **this is now repaired for the case that mattered** — a missing probe binary raises `ErrorKind::NotFound` and **panics** rather than being read as a fault — and the surviving `Err(_) => false` arm carries an explicit justification: at the bottom of the exponential probe the rlimit can be too small for the kernel to lay out the child's stack *at all*, and treating that as "did not survive at this bound" is what keeps the bisection **monotone**. $`\Rightarrow`$ The conflation is closed where it could produce a false measurement; the remaining arm is a **deliberate, documented** choice, not an oversight. *(This corrects the framing that reached this revision as an open, unnamed defect — see [Appendix D](#appendix-d--corrections-to-the-commissioning-brief) row 13.)*

![The bisection instrument and its 12,288 B floor](figures/bisection-instrument-floor.svg)

*Figure 11: the instrument floor. Source: [`figures/bisection-instrument-floor.puml`](figures/bisection-instrument-floor.puml). **Diagram type: an activity diagram over an annotated interval ladder**, chosen because the floor is a **control-flow** fact — a subject that survives the first probe never enters the doubling loop — so the claim is only visible if the branch that is **not** taken is drawn. A bar chart of the resulting numbers would hide precisely the mechanism.*

#### The bisection instrument, in literate form

**Algorithm 5 (MIN-STACK-BISECT).** *The instrument itself — exponential probe, bisection, and the refusal that makes the floor unspellable.*

```pseudocode
⟨Measure the minimum surviving stack of a subject at a depth⟩ ≡
    ⟨Probe upward until the subject survives⟩
    ⟨Bisect the bracketing interval to RESOLUTION⟩
    ⟨Refuse to answer below the instrument floor⟩

⟨Probe upward until the subject survives⟩ ≡
    hi ← PROBE_START                        ── 16 * 1024.  THE FLOOR'S CAUSE.
    while hi ≤ 512 MiB ∧ ¬ runs_within(hi, depth, subject):
        hi ← 2 · hi
    if hi > 512 MiB: fail "needed more than 512 MiB"
    lo ← hi / 2                             ── survives at hi, unknown at lo

⟨Bisect the bracketing interval to RESOLUTION⟩ ≡
    while hi − lo > RESOLUTION:             ── RESOLUTION = 4096
        mid ← (lo + hi) / 2
        if runs_within(mid, depth, subject) then hi ← mid else lo ← mid
    ── ⚠ When the FIRST probe already succeeded, lo = 8192 and hi = 16384, so
    ── the very first mid is 12288 and the loop ends there.  NO SUBJECT
    ── INFORMATION reaches that answer.

⟨Refuse to answer below the instrument floor⟩ ≡
    if hi ≤ SMALLEST_POSEABLE_STACK then
        return BelowResolution              ── #187: renders "<12 KiB (BELOW
                                            ── THE INSTRUMENT FLOOR)", never a
                                            ── number.  Unspellable ⇒ un-divisible.
    else
        return Bytes(hi)

⟨Decide whether the subject survived a given bound⟩ ≡        ── runs_within
    spawn the probe binary with RLIMIT_STACK = bound, RLIMIT_CORE = 0
    case exit:
        clean exit                    ⇒ true
        fault / non-zero              ⇒ false
        ErrorKind::NotFound           ⇒ PANIC       ── a MISSING probe binary is
                                                    ── not a measurement
        other spawn error             ⇒ false       ── execve refused the rlimit:
                                                    ── still "did not survive at
                                                    ── this bound"; keeps the
                                                    ── bisection MONOTONE
```

#### (2) 3,254 B/level was the wrong SHAPE, so the budget built on it was a ratio of two artefacts

The budget constraint $`k \leq 12288/3254`$ divided **an instrument floor** (numerator, §8.6.8(1)) by **a slope of the wrong subject** (denominator). Both factors are void, so the constraint never carried information — and, per the correction in [§0](#0-the-fix-register--the-scannable-index), the traversal it described now measures **0**, which would make the quotient undefined in any case.

★ **`k = 3` nevertheless survives, for entirely independent reasons**, and it is worth separating the *conclusion* from the *discarded derivation*: the modal datum is **3 cut-set levels**, `k = 3` covers **96.11 %** of observed cases, and the gate's growth is **exactly zero for $`k \leq 4`$**. $`\Rightarrow`$ A right answer that had a wrong proof. The wrong proof is recorded here rather than deleted, because a later reader who finds only the surviving justification cannot tell whether the constraint was ever checked.

---

### 8.6.9 ★★★ #121 — the gate built to DEMONSTRATE a fix overflowed, and the frame was not the encoder

This entry is a lesson about **where a defect is**, and it is the strongest single argument in the report for measuring rather than reasoning.

#121's gate existed to *demonstrate* a fixed encoder. **It still overflowed.** Bisection then showed the frame was **not the encoder at all**: it was `models/src/rust/pathmap_crate_type_mapper.rs`'s **`eval_stable_par` ⇄ `eval_stable_expr`**, mutually recursive and unbounded through `EList.ps` / `ETuple.ps` (**read from source**, `f1r3node-rust-mettail@8bf298ba`, `models/src/rust/pathmap_crate_type_mapper.rs:420`, `443`, `483`).

⚠★★ **It is the GROUND-DOMAIN gate**, so it runs on **every segment of every trie key** — one of the hottest paths in the system — **and it appeared in NO depth audit and NO tripwire list.** $`\Rightarrow`$ Two independent enumeration instruments both missed a traversal on a hot, attacker-reachable path. That is not a gap in either list; it is evidence that **enumeration by inspection does not converge**, which is [§7.4](#74-enumeration-completeness)'s thesis and the reason this document prefers derived registers and ratchets to hand-maintained inventories.

★ **Its honest price, reported because a stack-safety fix that is slower is still correct** (**MEASURED (q)**, `models/benches/trie_key_bench.rs:100–108`):

| depth | ratio (converted ÷ recursive) | reading |
|---:|---:|---|
| 1 | **0.264$`\times`$** | ⚠ **3.79$`\times`$ SLOWER**, $`+278\ \%`$ — a real regression: $`\approx 96`$ ns $`\rightarrow \approx 363`$ ns per escape payload |
| 8 | 1.071$`\times`$ | **crossover** lies between depth 1 and depth 8 |
| 1024 | **233.874$`\times`$** | faster by more than two orders of magnitude |

$`\Rightarrow`$ The trade is **a constant-factor loss on shallow terms for unbounded safety on deep ones**, on a path where the shallow case is the common one. ★ Reporting the 3.79$`\times`$ is not a caveat but the substance: it is the number a reader needs in order to disagree, and a report that published only the 233.874$`\times`$ would be advocacy rather than measurement.

---

### 8.6.10 The verdict of this section

1. **The eleven generated drivers are converted and measured flat.** That is real and it is not the whole state of the tree.
2. **#197 is live and unrepaired at `mettail-rust@7fad51db`**, inside #162's own commit, and it blocks `--all-targets` repo-wide.
3. **Eight `TRIPWIRE_DEPTH` members remain in f1r3node**, and **#43 is not superseded** by any `mettail-rust` work — disjoint families, repositories, and gate constants.
4. **Three event-hash legs are measured and not converted**, with the repair already implemented and never called, and the spliced path additionally $`\Theta(d^2)`$ **in time**.
5. **`try_eval` is partially converted** — one category of sixteen — and `UNMEASURED_TRAVERSALS = 7` is a live ratchet, not a closed list.
6. **The prost read ceiling is designed, not built**, and removing it naively would **introduce** a `SIGSEGV` through `skip_field`'s recursive `StartGroup` arm.
7. **10,491 B/level is reproducible and unattributed**; **$`\geq`$ 6 transcribed ceiling sites** are stale and the tripwire's 33-level resolution cannot see a 9-level drift.
8. **Two instrument corrections change the method**: 12,288 B is a floor whose repair exists in `mettail-rust` only, and the 3,254 B/level budget was a ratio of two artefacts.

$`\Rightarrow`$ **This report establishes a large, mechanically-checked class change over the traversals it names, and it does not establish that the `Par` family is stack-safe.** The precedent it follows is the census that prints *"bundled languages measured: 51"* — **not 54** — with its reason inline: **the number that is defensible, stated with what it excludes, in the same breath.**

---

## 9. Conclusions

1. **The class change is real and is mechanically enforced.** **Twenty-one traversals — 15 depth, 6 width** (*nineteen* before stage F-4; **read from source** at `f1r3node-rust-mettail@8bf298ba`) — hold their minimum surviving stack *identical* across a 1,024-fold change in nesting depth and a 16,384-fold change in sibling width, in both build profiles, checked by a gate whose registers are the single source of truth and whose checkers are shown in-suite to reject a $`\Theta(d)`$ control. ⚠ **This is a claim about 21 named subjects, not about the family**; the 8 that remain are [§8.6.2](#862--43--f1r3nodes-hand-written-par-traversals-8-tripwire-members-remain).

2. **The two headline availability defects are closed.** A term that could be *built* and not *destroyed* (8.8 kB of source aborting a node) and a pre-consensus ingress teardown reachable from unauthenticated gRPC (43.5 kB of source aborting a node) are both $`0`$ B/level with no ceiling below the search bound.

3. **The trampolined codecs are faster, not slower** on the exact production-weighted distribution — because the transformation deletes bincode's *sizing* traversal, which the CPU profile shows to be the more expensive of its two. ⚠★★ **The DIRECTION is the claim; the magnitude is NOW UNKNOWN, bracketed $`1.07\times`$–$`1.19\times`$.** This conclusion previously read *"$`1.194 \pm 0.005\times`$ … ranges non-overlapping, $`\alpha = 0.01`$"*, and the interval and the significance test are both **OVERTURNED**: the harness timed its arms in **disjoint time windows** while claiming per-repetition interleaving, and the same instrument later spread **27 %** over three runs where these three agreed to 0.76 % — a **36×** difference in three-run spread, which is what makes the $`\pm 0.005`$ luck rather than precision. §5.4.1's retraction derives it. ★ The sign survives because **two independent instruments agree on it**: even the least favourable blocked draw exceeds $`1`$, and the repaired paired harness reads $`1.073\times`$–$`1.092\times`$ at higher load. **And they allocate $`770\times`$ fewer blocks** in the reused-buffer form. The cost is $`3.25\times`$ more heap **writes** and $`2\times`$ peak heap on the shallow shape, both of which are relocations of previously-uncounted native-stack traffic. ★ **Those three figures are UNAFFECTED**: DHAT block counts are deterministic and are not a wall clock.

   ⚠★ **And nothing in conclusions 1, 2 or 4 depends on the retracted instrument.** Every B/level slope and every $`D_{\max}`$ ceiling in this report is obtained by stack-pointer differencing and by binary search on depth — neither is a timing — so the **stack-safety** results, which are what this report exists to establish, are untouched. The retraction is confined to the *throughput* claim, and it is confined to its *magnitude*.

4. **The `tokio` work is two fixes, not one**, and the report says which is which: inline `.await` nesting was a **native-stack** $`\Theta(d)`$ chain that had been *fed* by `stacker` rather than removed, and the awaited-parent chain was a **heap** $`\Theta(N)`$ chain of parked futures. Detaching both removed the `stacker` dependency entirely and cut the reference contract's work by $`\approx 3.2\times`$ in the one clock that is invariant to machine contention.

5. **The residual is larger than the fixed part on the network path.** The protobuf codec is recursive on both sides, capped on one, and is the standing home of the write/read asymmetry. Three routes exist to close it; **all three are consensus decisions and none was taken here.**

6. **The instrument produced more findings than the fixes did.** A totally vacuous generator quantifying over two values; four false zeros from probes that measured nothing; a headline test whose result came from one call in a fixture; a control calibrated in the wrong profile; a slope estimate biased to zero by its own intercept; a check that counted lines instead of testing its claim. ★ **Every number in §5 is worth exactly as much as the anti-vacuity leg standing behind it**, and where such a leg does not exist — §5.9's seven entries — the report says *not measured* rather than guessing.

7. ⚠★★★ **THE WORK IS NOT COMPLETE, AND THIS CONCLUSION IS PART OF THE RESULT RATHER THAN A CAVEAT ON IT.** [§8.6](#86--the-open-residual-register--what-this-report-does-not-establish) is the register; its eight-line verdict is [§8.6.10](#8610-the-verdict-of-this-section). The headline items a reader must not lose:

   * **#197 is live and unrepaired** at `mettail-rust@7fad51db` — **inside `fab6de24`, one of the very commits [§8.6.1](#861--162189--the-eleven-generated-drivers-converted-and-the-root-cause-that-unifies-154-with-162) reports as a success** — and it blocks `--all-targets` **repo-wide**. The eleven conversions are real *and* the repository that reports them cannot currently build all targets. Both halves are true and the second is the one a completion claim would suppress.
   * **Eight `TRIPWIRE_DEPTH` members remain** in f1r3node, and **#43 is not superseded** by #162/#189 — disjoint repositories, families and gate constants, so no fix on either side discharges the other.
   * **Three event-hash legs are measured, not converted**, on a path that reaches the block hash; the spliced walk is additionally $`\Theta(d^2)`$ **in time**, a cost no B/level figure in this report can see.
   * **`try_eval` is one category converted of sixteen**; the prost read ceiling is **designed, not built**, and naive removal would *introduce* a `SIGSEGV`; 10,491 B/level remains **unattributed**; and the ceiling inventory is **stale in a band its own tripwire cannot resolve**.
   * **Two instrument corrections change the method**, and one of them — the `MinStack` floor repair — **exists in `mettail-rust` only**, so this repository's endpoint byte figures at 12,288 remain floor-affected. The *slopes*, which are what conclusions 1–5 rest on, are differences and are unaffected.

   $`\Rightarrow`$ **The defensible claim is the one this report should be cited for:** *a large, mechanically-checked class change over the traversals named in [§0](#0-the-fix-register--the-scannable-index)* — **not** *"the `Par` family is stack-safe"*. The standing finding of this campaign is that **a gate whose advertised coverage exceeds its real coverage is worse than none**; that applies to documents exactly as it applies to tests, and this section is where this document submits to its own rule.

---

## References

Abbreviations used in the entries below:

| abbreviation | expansion |
|---|---|
| ACM | Association for Computing Machinery |
| SIGPLAN | ACM Special Interest Group on Programming Languages |
| SIGACT | ACM Special Interest Group on Algorithms and Computation Theory |
| PPDP | Principles and Practice of Declarative Programming |
| POPL | Principles of Programming Languages |
| PLDI | Programming Language Design and Implementation |
| ICFP | International Conference on Functional Programming |


###### ref-ager2003

**[Ager et al. 2003]** Ager, M. S., Biernacki, D., Danvy, O., & Midtgaard, J. (2003). *A functional correspondence between evaluators and abstract machines.* In Proceedings of the 5th ACM SIGPLAN International Conference on Principles and Practice of Declarative Programming (PPDP '03), 8–19. [doi:10.1145/888251.888254](https://doi.org/10.1145/888251.888254)

###### ref-cheney1970

**[Cheney 1970]** Cheney, C. J. (1970). *A nonrecursive list compacting algorithm.* Communications of the ACM, 13(11), 677–678. [doi:10.1145/362790.362798](https://doi.org/10.1145/362790.362798)

###### ref-danvy2001

**[Danvy & Nielsen 2001]** Danvy, O., & Nielsen, L. R. (2001). *Defunctionalization at work.* In Proceedings of the 3rd ACM SIGPLAN International Conference on Principles and Practice of Declarative Programming (PPDP '01), 162–174. [doi:10.1145/773184.773202](https://doi.org/10.1145/773184.773202)

###### ref-debruijn1972

**[de Bruijn 1972]** de Bruijn, N. G. (1972). *Lambda calculus notation with nameless dummies, a tool for automatic formula manipulation, with application to the Church–Rosser theorem.* Indagationes Mathematicae, 75(5), 381–392. [doi:10.1016/1385-7258(72)90034-0](https://doi.org/10.1016/1385-7258(72)90034-0)

###### ref-felleisen1987

**[Felleisen & Friedman 1987]** Felleisen, M., & Friedman, D. P. (1987). *A calculus for assignments in higher-order languages.* In Proceedings of the 14th ACM SIGACT-SIGPLAN Symposium on Principles of Programming Languages (POPL '87), 314–325. [doi:10.1145/41625.41654](https://doi.org/10.1145/41625.41654)

###### ref-ganz1999

**[Ganz et al. 1999]** Ganz, S. E., Friedman, D. P., & Wand, M. (1999). *Trampolined style.* In Proceedings of the Fourth ACM SIGPLAN International Conference on Functional Programming (ICFP '99), 18–27. [doi:10.1145/317636.317779](https://doi.org/10.1145/317636.317779)

###### ref-knuth1984

**[Knuth 1984]** Knuth, D. E. (1984). *Literate Programming.* The Computer Journal, 27(2), 97–111. [doi:10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97)

###### ref-landin1964

**[Landin 1964]** Landin, P. J. (1964). *The mechanical evaluation of expressions.* The Computer Journal, 6(4), 308–320. [doi:10.1093/comjnl/6.4.308](https://doi.org/10.1093/comjnl/6.4.308)

###### ref-meredith2005

**[Meredith & Radestock 2005]** Meredith, L. G., & Radestock, M. (2005). *A reflective higher-order calculus.* Electronic Notes in Theoretical Computer Science, 141(5), 49–67. [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)

###### ref-milner1992

**[Milner, Parrow & Walker 1992]** Milner, R., Parrow, J., & Walker, D. (1992). *A calculus of mobile processes, I.* Information and Computation, 100(1), 1–40. [doi:10.1016/0890-5401(92)90008-4](https://doi.org/10.1016/0890-5401%2892%2990008-4)

###### ref-nethercote2007

**[Nethercote & Seward 2007]** Nethercote, N., & Seward, J. (2007). *Valgrind: a framework for heavyweight dynamic binary instrumentation.* In Proceedings of the 28th ACM SIGPLAN Conference on Programming Language Design and Implementation (PLDI '07), 89–100. [doi:10.1145/1250734.1250746](https://doi.org/10.1145/1250734.1250746)

###### ref-reynolds1972

**[Reynolds 1972]** Reynolds, J. C. (1972). *Definitional interpreters for higher-order programming languages.* In Proceedings of the ACM Annual Conference, 717–740. Reprinted in Higher-Order and Symbolic Computation, 11(4), 363–397 (1998). [doi:10.1023/A:1010027404223](https://doi.org/10.1023/A:1010027404223)

###### ref-schorr1967

**[Schorr & Waite 1967]** Schorr, H., & Waite, W. M. (1967). *An efficient machine-independent procedure for garbage collection in various list structures.* Communications of the ACM, 10(8), 501–506. [doi:10.1145/363534.363554](https://doi.org/10.1145/363534.363554)

###### ref-tarjan1972

**[Tarjan 1972]** Tarjan, R. (1972). *Depth-first search and linear graph algorithms.* SIAM Journal on Computing, 1(2), 146–160. [doi:10.1137/0201010](https://doi.org/10.1137/0201010)

###### ref-georges2007

**[Georges et al. 2007]** Georges, A., Buytaert, D., & Eeckhout, L. (2007). *Statistically rigorous Java performance evaluation.* In Proceedings of the 22nd ACM SIGPLAN Conference on Object-Oriented Programming Systems, Languages and Applications (OOPSLA '07), 57–76. [doi:10.1145/1297027.1297033](https://doi.org/10.1145/1297027.1297033) — the design discipline §5.4.1's retraction failed to follow: an interval must be reported with the method that produced it, because a between-run spread over blocked arms estimates a different quantity from a within-run one.

###### ref-mytkowicz2009

**[Mytkowicz et al. 2009]** Mytkowicz, T., Diwan, A., Hauswirth, M., & Sweeney, P. F. (2009). *Producing wrong data without doing anything obviously wrong!* In Proceedings of the 14th International Conference on Architectural Support for Programming Languages and Operating Systems (ASPLOS '09), 265–276. [doi:10.1145/1508244.1508275](https://doi.org/10.1145/1508244.1508275) — ★ the class §5.4.1's defect belongs to: measurement bias that **reverses a conclusion** while every visible part of the method looks correct. The title is the finding.

###### ref-student1908

**[Student 1908]** "Student" (Gosset, W. S.) (1908). *The probable error of a mean.* Biometrika, 6(1), 1–25. [doi:10.2307/2331554](https://doi.org/10.2307/2331554) — the **paired** $`t`$-test, which is the valid statistic for a two-arm throughput comparison, because pairing cancels the window term $`\delta(w_i)`$ *before* the test sees the data rather than attempting to model it afterwards.

###### ref-welch1947

**[Welch 1947]** Welch, B. L. (1947). *The generalization of "Student's" problem when several different population variances are involved.* Biometrika, 34(1–2), 28–35. [doi:10.1093/biomet/34.1-2.28](https://doi.org/10.1093/biomet/34.1-2.28) — ⚠ the **unpaired** test whose three statistics §5.4.1 retracts. Sound for its own assumptions; those assumptions are violated by blocked arms, because its denominator is built from the within-arm variances and therefore omits $`\sigma_\delta`$ entirely.

★ **Every DOI above was resolved against the Crossref API on 2026-07-29** and its title, container and year confirmed to match the citation as written. Command in Appendix A.8.

---

## Appendix A — reproduction commands

All commands are run from the repository root, `/home/dylon/Workspace/f1r3fly.io/f1r3node-rust-mettail`.

**A.0 — build (⚠ *without* `-D warnings`; see §4.3).**

```bash
systemd-run --user --scope -p MemoryMax=28G --quiet \
  cargo build --release -p models --benches --tests
systemd-run --user --scope -p MemoryMax=28G --quiet \
  cargo build --release -p rholang --test stack_depth_gate --test deploy_depth_ceiling
```

**A.1 — the whole gate, release.**

```bash
GATE=$(find target/release/deps -maxdepth 1 -name 'stack_depth_gate-*' \
        -type f -executable -printf '%T@ %p\n' | sort -rn | head -1 | cut -d' ' -f2)
systemd-run --user --scope -p MemoryMax=28G --quiet \
  taskset -c 16-23 env RUST_MIN_STACK=8388608 "$GATE" \
  --test-threads=4 --nocapture 2>&1 | tee /tmp/sd_gate_release.log
```

**A.2 — the S0 four-quadrant baseline** (an `#[ignore]`d test in the same binary).

```bash
taskset -c 16-23 env RUST_MIN_STACK=8388608 "$GATE" \
  four_quadrant_s0_baseline --ignored --nocapture --test-threads=1 \
  2>&1 | tee /tmp/sd_s0_release.log
```

**A.3 — the end-to-end deploy ceiling.**

```bash
DDC=$(find target/release/deps -maxdepth 1 -name 'deploy_depth_ceiling-*' \
       -type f -executable -printf '%T@ %p\n' | sort -rn | head -1 | cut -d' ' -f2)
systemd-run --user --scope -p MemoryMax=28G --quiet taskset -c 24-27 "$DDC" \
  the_deploy_depth_ceiling_at_a_production_worker_stack \
  --nocapture --test-threads=1 2>&1 | tee /tmp/sd_deploy_ceiling.log
```

**A.4 — massif heap profiles**, one arm per process, in parallel on distinct cores.

```bash
BIN=target/release/deps/wire_encode_massif-*        # the built bench binary
for i in 0 1 2 3 4; do
  arm=$(echo "derived machine reused deep decode" | cut -d' ' -f$((i+1)))
  MASSIF_ARM=$arm taskset -c $((4+i)) valgrind --tool=massif --time-unit=B \
    --detailed-freq=1 --max-snapshots=200 \
    --massif-out-file=/tmp/sd_massif/massif.$arm.out $BIN &
done; wait
for a in derived machine reused deep decode; do
  ms_print /tmp/sd_massif/massif.$a.out > /tmp/sd_massif/ms.$a.txt
done
```

**A.5 — DHAT allocation counts**, same arms.

```bash
for i in 0 1 2 3 4; do
  arm=$(echo "derived machine reused deep decode" | cut -d' ' -f$((i+1)))
  MASSIF_ARM=$arm taskset -c $((10+i)) valgrind --tool=dhat \
    --dhat-out-file=/tmp/sd_dhat/dhat.$arm.json $BIN \
    2>/tmp/sd_dhat/summary.$arm.txt &
done; wait
grep -E 'Total:|At t-gmax:|At t-end:|Reads:|Writes:' /tmp/sd_dhat/summary.*.txt
```

**A.6 — the throughput bench**, three whole-bench runs pinned to one core.

```bash
BENCH=target/release/deps/wire_encode_bench-*
for run in 1 2 3; do
  echo "### RUN $run ($(date -Is), load $(cut -d' ' -f1 /proc/loadavg))"
  systemd-run --user --scope -p MemoryMax=28G --quiet taskset -c 8 "$BENCH"
done 2>&1 | tee /tmp/sd_wire_bench.log
```

**A.7 — the CPU profile** (⚠ software event; see §4.4).

```bash
taskset -c 12 perf record -e cpu-clock -F 9999 --call-graph dwarf,16384 \
  -o /tmp/sd_perf/perf.bench.data -- "$BENCH" > /tmp/sd_perf/bench_under_perf.txt 2>&1
perf report -i /tmp/sd_perf/perf.bench.data --no-children \
  --percent-limit 0.8 --stdio > /tmp/sd_perf/report.flat.txt
```

**A.8 — DOI verification.**

```bash
for d in 10.1093/comjnl/6.4.308 10.1023/A:1010027404223 10.1145/888251.888254 \
         10.1145/363534.363554 10.1145/362790.362798 10.1145/317636.317779 \
         10.1137/0201010 10.1093/biomet/34.1-2.28 10.1093/comjnl/27.2.97 \
         10.1145/1250734.1250746 10.1145/773184.773202 \
         "10.1016/0890-5401(92)90008-4" 10.1016/j.entcs.2005.05.016 \
         "10.1016/1385-7258(72)90034-0" 10.1145/41625.41654; do
  curl -s -A 'doi-check/1.0' "https://api.crossref.org/works/$d" \
    | python3 -c "import sys,json; m=json.load(sys.stdin)['message']; \
        print(m['title'][0], '|', m['container-title'][0])"
done
```

**A.9 — rendering the figures**, and checking that they are not empty.

```bash
cd docs/design/stack-safety/figures
for f in *.puml; do plantuml -tsvg "$f"; done
for f in *.svg; do
  printf "%-40s bytes=%-8s latexImgs=%-3s leaked=%s\n" "$f" "$(stat -c%s "$f")" \
    "$(grep -o 'data:image/svg+xml;base64' "$f" | wc -l)" "$(grep -c 'latex&gt;' "$f")"
done
```

---

## Appendix B — raw data locations

| file | contents |
|---|---|
| `/tmp/sd_gate_release.log` | the full release gate run of 2026-07-29 — every converted subject's two ladder ends, every tripwire slope, the read-ceiling envelopes, the synthetic controls |
| `/tmp/sd_s0_release.log` | the S0 four-quadrant baseline CSV, release |
| `/tmp/sd_deploy_ceiling.log` | the end-to-end deploy bisection (`plain_deploy` 6,831; `env_get_deploy` 274) |
| `/tmp/sd_massif/massif.{derived,machine,reused,deep,decode}.out` | raw massif output, `--time-unit=B` |
| `/tmp/sd_massif/ms.*.txt` | `ms_print` renderings, including the peak snapshot allocation trees |
| `/tmp/sd_dhat/summary.*.txt` | DHAT totals: bytes, blocks, `t-gmax`, `t-end`, reads, writes |
| `/tmp/sd_dhat/dhat.*.json` | full DHAT program-point data |
| `/tmp/sd_wire_bench.log` | three whole-bench runs, all cells, with per-run load and environment blocks |
| `/tmp/sd_perf/perf.bench.data` | the perf recording (466 MB, 29,436 samples) |
| `/tmp/sd_perf/report.flat.txt` | the flat profile at `--percent-limit 0.8` |
| `/tmp/sd_perf/all_symbols.txt` | the full 204-symbol profile used for the bucket aggregation of §5.4.2 |

⚠ **These are `/tmp` paths and will not survive a reboot.** The *derivable* figures — everything in §5 tagged **(f)** — are reproducible from Appendix A in minutes; the perf recording is the only artefact that is expensive to regenerate.

---

## Appendix C — the complete fix inventory

**20 code fixes**, **18 instrument commits**, **1 rejected candidate**.

### C.1 Code fixes

| # | commit | repo | what it converted / removed | key figure |
|---|---|---|---|---|
| A1 | `f0894109` | f1r3node | leg-1 de-clone of the substitution SCC | release $`-`$ 25.4 %; **class unchanged** |
| A2 | `f11ffb54` | f1r3node | substitution SCC $`\rightarrow`$ explicit worklist, `EnvView` delta | 195,728 $`\rightarrow`$ **0** (debug) |
| A3 | `6ce7c5b9` | f1r3node | score tree: comparator, sibling walk, `Clone`, `Drop`, `PartialEq` | 1,329 / 201 / 1,578 / 370 / 719 $`\rightarrow`$ **0** |
| A4 | (in `6ce7c5b9`/`f11ffb54`) | f1r3node | `ParSortMatcher` — `sort`, `sort_wide` | 78,592 $`\rightarrow`$ **0** (debug) |
| A5 | `a3fd6fe4` | f1r3node | `rho-pure-eval`'s `eval_with` SCC $`\rightarrow`$ worklist | 21,584 / 3,359 $`\rightarrow`$ **0 / 0** |
| A6 | (staged with the above) | f1r3node | `PrettyPrinter` $`\rightarrow`$ explicit pushdown driver | flat both axes |
| A7 | (Stage G) | f1r3node | `normalize_ann_proc`'s 26-function SCC $`\rightarrow`$ `norm_drive` | 43,542 / 7,261 $`\rightarrow`$ **0 / 0** |
| B1 | `a929a2d6` | f1r3node | 6-member expression-evaluator SCC $`\rightarrow`$ `eval_drive` | overflow $`\approx`$ 1.5k $`\rightarrow`$ **OK at 50,000** |
| B2 | `29856679`, `55b97f84`, `a0a50473` | f1r3node | `DriveState`/`LiveGuard`/`spawn_detached`; 5 join sites detached | 300 s $`\rightarrow`$ **93.7 s CPU** |
| B3 | `9843e4b6` | f1r3node | `StackGrowingFuture` + `stacker` **deleted** | dependency removed |
| C1 | `9a5521a2` | f1r3node | cold-store **decoder** (`par_codec`) | 28,362 / 12,894 $`\rightarrow`$ **0 / 0** |
| C2 | `c28f4cf6` + `a169cc61` | f1r3node | cold-store **encoder** (`wire_encode`), single walk | ~224 $`\rightarrow`$ **0** (release control) |
| C3 | `7c74260d` | f1r3node | the wire-schema generator: one walk, four outputs | enabling infrastructure |
| C4 | `56fb1fd0` | f1r3node | prost encoder: $`\Theta(d^2) \rightarrow \Theta(n)`$ work | ⚠ stack unchanged; **dormant** |
| D1 | `d2591fa1` | f1r3node | per-branch `Par` deep clone at the task-spawn boundary | the binding worker-side member |
| D2 | `94dc983f` | f1r3node | ownership to the substitution; **15** deep copies, incl. an $`O(n^2)`$ | — |
| D3 | `9082d12c` | f1r3node | `inj_attempt`'s read-back clone $`\rightarrow`$ by-move | 2,852 $`\rightarrow`$ **0**; 729 $`\rightarrow`$ **$`\geq 1{,}048{,}576`$** |
| D4 | `a09f1de2` + `3b265eb7` | f1r3node | ingress `validate_deploy_term` | 96.0 $`\rightarrow`$ **0**; 21,781 $`\rightarrow`$ **none** |
| D5 | `64a5d2bc` (subject) | f1r3node | metered wrappers take term by value | 2,852 $`\rightarrow`$ **146**; 286 $`\rightarrow`$ **6,831** |
| F1 | `3c0c3585` | mettail-rust | the 87-member lowering component $`\rightarrow`$ one worklist | 15,132 / 2,157 $`\rightarrow`$ **1 / 0** |

### C.2 Instrument commits

`f0894109` (the gate) · `550b967a` (harness prerequisites, child-slot table, iterative dismantle) · `9ab6b0eb` (release-profile run, child no-op) · `4822520c` (clone-free probe builders) · `e72dcae8` (end-to-end deploy bisection harnesses) · `291bc217` (`drop` $`\rightarrow`$ `par_drop`, reachability, register becomes derived) · `dd0ba13f` (checkers get subjects they must reject) · `0a7be7f4` (destructor-control span recalibration) · `857c62fe` (wall clock $`\rightarrow`$ CPU time) · `ee1dfdad` (ingress becomes a converted claim; 84.3 withdrawn) · `6a1d7c62` (read ceiling joins the gate) · `44535d75` (S0 baseline) · `5dc1aad7` (trampoline-twin structural check) · `80f5e5d3` (#129 measured $`\rightarrow`$ latent; the site registry) · `0e0f9719` (the absent-child axis) · `53b8a85f` (architecture repro) · and in mettail-rust: `1339c1e2`/`6275b0e2`/`d72740e6`/`21c51d10`/`73c774a3`.

### C.3 Rejected

`cf35ab53` — exhaustive `PartialEq`/`Hash`. See §5.8.

---

## Appendix D — corrections to the commissioning brief

Recorded because a report that silently absorbs its brief's errors is less useful than one that names them.

| # | the brief said | the record says |
|---|---|---|
| 1 | *"`cf35ab53` — exhaustive `PartialEq`/`Hash` (⚠ verify whether this is stack-safety at all)"* | **Correctly flagged, and it is not.** Reflexivity/hashing correctness, no depth axis. **Rejected** (§5.8). |
| 2 | *"Verify [the S0] table against `rholang/tests/stack_depth_gate.rs` (`CONVERTED_DEPTH`, `TRIPWIRE_DEPTH`, `assert_slope_below`)"* | The S0 table is **not** in those three constants. Its harness is `four_quadrant_s0_baseline`, an `#[ignore]`d test in the same file, and the table itself lives in `docs/design/audits/four-quadrant-s0-baseline-2026-07-28.md`. **Run for this report; it reproduces to the byte in all ten rows** (§5.3.1). |
| 3 | *"`prost_de` 4096 · `clone` 3254 · … · `prost_ser` 302 · `eq` 221 · `par_drop` 144 · `hash` 136"* | **All eight confirmed** — but the list **mixes two ladders**. `prost_ser 302` and `par_drop 144` are the *`@gate`* rows (64 $`\rightarrow`$ 1024 and 256 $`\rightarrow`$ 4096); on the 16 $`\rightarrow`$ N ladder used for the other six they are **292** and **136**. |
| 4 | *"`9082d12c` — `<Par as Clone>::clone`; claimed 2,852 B/level"* | ★★ **Confirmed from the diffs, and the same error is in the task tracker.** `9082d12c` changed **thirteen lines of `interpreter.rs`** and left `models/` untouched: it deleted a *call to* the clone (`.source_process().cloned()` $`\rightarrow`$ `.into_source_process()`), not the impl. 2,852 is the **composition**'s slope at that call site. **`<Par as Clone>::clone` itself is untouched** — `git log -S` over five spellings returns **0** commits — remains in `TRIPWIRE_DEPTH`, and measures **3,254** B/level release. #76 routed a **call site** and said so (*"distinct call site, distinct repair"*); its one-line summary compressed that to *"clone is worse"*, and #77's summary reports *"2,852 $`\rightarrow`$ 0"* without naming its subject. **Both tracker lines need the correction in [§5.10.5](#5105-the-verdict-and-the-correction-to-the-tracker).** Full settlement: [§5.10](#510--the-generated-trait-implementations-and-the-clone-question). |
| 5 | *"`a09f1de2`, `3b265eb7`, `ee1dfdad` — claimed 96 $`\rightarrow`$ 0 B/level"* | Correct **as amended**. The originally recorded figure was **84.3** and was **withdrawn**; 96.0 is the corrected value, from four fixed-stack bisections at $`r^2 = 1.0000`$ (§5.5.2). |
| 6 | *"claimed plain_deploy 286 $`\rightarrow`$ 6,831 levels (23.9 $`\times`$), with `env_get_deploy` unmoved at 283 as the control"* | `plain_deploy` **reproduces exactly** at HEAD. **`env_get_deploy` now measures 274, not 283** — a 9-level drift the gate's transcribed inventory has not caught (§5.5.4). |
| 7 | *"the still-recursive prost paths (`merge_field` across 28 call sites)"* | **Not reproducible.** `merge_field` is derive-generated and appears **zero** times in the generated source text. The countable residual is **57 `::prost::Message` derives, 5 `::prost::Oneof`, 37 members in the recursive SCC** (§8.1). |
| 8 | *"the `tokio::async` fire-and-forget work — I do not have reliable knowledge of this one"* | Found in git and documented at full rigour. ★ It is **two** defects in **two** resources, not one (§5.2.2). |
| 9 | *"★ `#103`, where the predicted 55–60 ms came in at 103.57 ms"* | **Not found in this worktree** (§5.9 #7). Three other falsified predictions were found and are reported. |
| 10 | *"Include a cross-cutting section on the ASYMMETRY"* — initially framed as the centre of gravity | Included as **one** section (§6.4), per the later correction. The per-fix measurements are the report's centre. |

### D.2 The 2026-07-30 revision brief (the guideline-alignment and residuals pass)

Recorded on the same rule. Every figure in that brief was checked against source or a commit body before being written into [§8.6](#86--the-open-residual-register--what-this-report-does-not-establish); these are the ones that did not survive the check.

| # | the revision brief said | the record says |
|---|---|---|
| 11 | *"There are at least THREE candidate documents … establish the set and report the COUNT"* | **Withdrawn by the owner mid-task.** The set is a **singleton** — this document. The two audits named as candidates (`mettail-rust`'s `lowering-stack-depth-audit-2026-07-27.md`, this repository's `theta-depth-traversals-2026-07-26.md`) are **explicitly out of scope and were not edited**. |
| 12 | *"`#43` … **8 tripwire members remain** — `subst_and_charge`, `substitute_deep_binding`, `par_drop`, `normalize_drop`, `encode`, `sort_nested_set`, `sort_nested_map`, `clone_nested_set`"* | ✅ **Confirmed exactly**, 8 members, **read from source** at `8bf298ba`. ⚠★ **But a naive text census of the same block returns NINE**, because `TRIPWIRE_DEPTH` carries a comment reading `` ⚠★ `clone` IS GONE FROM THIS LIST ``. A `grep` for quoted names inside the block counts the *commented* name and silently over-reports. $`\Rightarrow`$ **This is the campaign's own "census the classifier, not the artefact" hazard, reproduced against the census itself**, and it is recorded because the wrong answer here is a *plausible* one. |
| 13 | *"the defect **neither prior analysis named**: `runs_within` mapped an `execve` refusal to `Err(_) => false` — the same verdict as a genuine overflow — so the instrument could not distinguish *needs more stack* from *cannot ask*"* | ⚠ **Stale as an OPEN defect.** The conflation was real; **it is repaired at `mettail-rust@7fad51db`** for the case that could produce a false measurement: `ErrorKind::NotFound` now **panics** (a missing probe binary is no longer readable as a fault), and the surviving `Err(_) => false` arm carries an explicit justification — an `execve` refusal at a too-small rlimit *is* "did not survive at this bound", and treating it so is what keeps the bisection **monotone**. $`\Rightarrow`$ Reported in [§8.6.8](#868--the-instrument-floor-and-the-wrong-shape--two-corrections-that-change-the-method-not-just-a-number) as **a closed defect with a deliberate residual arm**, not as an open one. |
| 14 | *"`<Par as Clone>::clone` … remains in `TRIPWIRE_DEPTH` at 3,254 B/level"* (carried forward from this document's own §0 and §8.3) | ⚠★★ **Superseded at HEAD, and the brief inherited the error from this document rather than introducing it.** `clone` was **CONVERTED** by stage F-4 (`0eac9c3a`) and sits in `CONVERTED_DEPTH`. Corrected in [§0](#0-the-fix-register--the-scannable-index) with the superseded wording quoted verbatim beside it. |
| 15 | *"**3,254 B/level was the wrong SHAPE** — a single derived function, not the family (7,021)"* | ⚠ **Half right, and the direction matters.** `0eac9c3a` records the opposite attribution: **3,254 is the on-record figure for the derive**, and **7,021 is the ORACLE's** reading — a *semantic* reproduction by a family of free functions, which under `-O` inlines differently and reads **2.16$`\times`$ high**. So 7,021 is not "the family's true slope" that 3,254 understated; it is an instrument artefact of the control. $`\Rightarrow`$ The brief's *conclusion* stands — the budget $`k \leq 12288/3254`$ was a ratio of two artefacts — but its *reason* is corrected in [§8.6.8(2)](#868--the-instrument-floor-and-the-wrong-shape--two-corrections-that-change-the-method-not-just-a-number). ★ And both factors are now moot: the traversal measures **0**. |
| 16 | *"a converted-vs-tripwire status map … the recursive-descent $`\rightarrow`$ work-stack transformation"* listed among **five** diagrams owed | **Four were owed, not five.** The recursive-descent $`\rightarrow`$ work-stack transformation **already existed** as `recursive-vs-trampolined` (9 typeset LaTeX spans, 16 colours). Four new figures were authored; the existing seven were **extended, not replaced**. |
| 17 | *"`min_stack_for` now returns `MinStack{Bytes, BelowResolution}`"* stated without repository | ⚠★★ **True in `mettail-rust` only.** **Read from source** at `f1r3node-rust-mettail@8bf298ba`: this repository's `min_stack_for` still returns a bare `usize` and still starts at `16 * 1024`. $`\Rightarrow`$ **The floor repair has not crossed into the repository this report is about**, which is a residual the brief did not name and [§8.6.8](#868--the-instrument-floor-and-the-wrong-shape--two-corrections-that-change-the-method-not-just-a-number) now does. |
| 18 | the count *"nineteen traversals (13 depth + 6 width)"*, carried in this document's abstract, §5 preamble and conclusion 1 | ⚠ **Stale in all three places: it is 21 (15 + 6)** at `8bf298ba`. `clone` and `clone_send_chain` were converted by stage F-4 after the sentence was written. All three corrected, with the superseded figure named. |

---

## Appendix E — documentation-guideline conformance

This report is held to the same evidentiary standard as its subject matter: conformance to the pgmcp documentation guidelines is **executed**, not asserted. *A guideline nobody checks is a guideline that silently rots* — and this project has already shipped that failure mode once, with four `.svg` files sitting on disk at **zero bytes** beside `.puml` sources that had real content, because the check that would have caught it existed and had never been run.

★ **The complete per-slug audit — all 26 guidelines, BEFORE and AFTER the 2026-07-30 revision — is [§E.6](#e6--the-complete-per-slug-audit-all-26-guidelines-before-and-after).** §E.1–E.5 below describe the *instruments*; §E.6 is the *verdict*, and it is the table to read if only one is read.

⚠★★ **Provenance of the guideline list, stated because it changes how much the table is worth.** The canonical source is pgmcp's `documentation_guidelines` tool (**26 slugs, 7 categories**). That tool — and `tool_catalog` / `enable_tools` / `call_tool`, the documented routes to it — **were not in the acting agent's tool catalog** for this revision. The list was therefore taken from the **source of record**, `/home/dylon/Workspace/f1r3fly.io/pgmcp/src/docguidelines/mod.rs`, function `guideline_seeds()`, which is the single place every other pgmcp rendering derives from and whose own doc-comment pins the count at 26. The category tally read from that source — Placement 2, Coverage 3, Pedagogy 4, Diagrams 9, MathNotation 3, Citations 3, AlgorithmsCode 2 — sums to **26** and matches the tool's advertised shape. $`\Rightarrow`$ **A fallback, and it is recorded as one**; a prior agent on this campaign had to make the same fallback, which is itself worth knowing.

### E.1 The mechanised gate

`mettail-rust`'s `docs/languages/validate.sh` carries **17 mechanised checks** and accepts Markdown files from outside its own suite as positional arguments, held to the document-level checks. It was pointed at this report.

```bash
cd /home/dylon/Workspace/f1r3fly.io/mettail-rust
DOCLINT_DOI=on ./docs/languages/validate.sh \
  /home/dylon/Workspace/f1r3fly.io/f1r3node-rust-mettail/docs/design/stack-safety/stack-safety-report-2026-07-29.md
```

**Result (2026-07-30 revision, `DOCLINT_DOI=on`): 17 / 17 PASS, exit status 0, zero diagnostics naming this file.** Log at `/tmp/ss_final3.log`.

> ⚠★★★ **THE PREVIOUSLY RECORDED RESULT HAD ROTTED, AND THIS IS THE MOST INSTRUCTIVE FINDING IN THIS APPENDIX.** This line read: *"**Result: 17 / 17 PASS, exit status 0, zero diagnostics naming this file.** Log at `/tmp/sd_validate3.log`."* That was true when written. **Re-running the same command against the committed document at `f1r3node-rust-mettail@8bf298ba` returns 14 / 17 — three checks FAILING** (**MEASURED (f)**, 2026-07-30, log `/tmp/ss_validate_before.log`):
>
> | check | slug | BEFORE (measured, `8bf298ba`) | cause |
> |---|---|:---:|---|
> | `math-symbol-literals` | `math-mathjax` | ❌ **FAIL** | two bare right-double-arrow operators in prose (named, not shown — see §E.6a), at what are now lines 407 and 419 |
> | `math-backticks` | `math-backticks` | ❌ **FAIL** | same two expressions, left outside a math span |
> | `pedagogy-define-terms` | `pedagogy-define-terms` | ❌ **FAIL** | `PMU`, `ISA`, `IBS` never expanded; `PEBS` used before its definition |
>
> **The rot was introduced by two later commits — `cebdedbb` and `23eff25a` — which edited §4.4 and §5.4 after this appendix recorded its green result.** Neither re-ran the gate. $`\Rightarrow`$ **A recorded PASS is a measurement with a timestamp, not a property of the document**, and this appendix asserting *"a guideline nobody checks is a guideline that silently rots"* had itself gone stale in exactly that way, in eleven days. ★ All three are repaired in this revision, along with six diagnostics introduced by the revision's own new material — which the gate caught, which is the entire argument for having it.

$`\Rightarrow`$ **This is the concrete case for [Appendix G](#appendix-g--keeping-this-document-current)'s thesis**, and it strengthens it: G proposes binding the document's *numbers* to the gate, and this shows the document's *conformance result* needs the same treatment. A `validate.sh` invocation in continuous integration, on any commit touching this file, would have failed `cebdedbb` at the moment it introduced the regression.

| # | check | guideline slug(s) | verdict |
|---:|---|---|---|
| 1 | `fences-balanced` | structural precondition | **PASS** |
| 2 | `math-symbol-literals` | `math-mathjax` | **PASS** |
| 3 | `math-delimiters` | `math-delimiters` | **PASS** |
| 4 | `math-github-renderable` | `math-delimiters` | **PASS** |
| 5 | `math-backticks` | `math-backticks` | **PASS** |
| 6 | `diagrams-plantuml-assets` | `diagrams-prefer-plantuml`, `diagrams-complete` | **PASS** |
| 7 | `diagrams-plenty` | `diagrams-plenty` | **PASS** |
| 8 | `diagrams-fully-colored` | `diagrams-fully-colored` | **PASS** |
| 9 | `links-relative` | `doc-placement`, `pedagogy-logical-flow` | **PASS** |
| 10 | `anchors-in-document` | `pedagogy-logical-flow` | **PASS** |
| 11 | `citations-exist+doi-links` | `citations-exist`, `citations-doi-links` | **PASS** |
| 12 | `citations-doi-valid` | `citations-doi-valid` | **PASS** (with `DOCLINT_DOI=on`) |
| 13 | `pedagogy-define-terms` | `pedagogy-define-terms` | **PASS** |
| 14 | `algorithms-literate-pseudocode` | `algorithms-literate-pseudocode` | **PASS** |
| 15 | `code-snippets-valid` | `code-snippets-valid` | **PASS** |
| 16 | `roster-coverage` | `doc-naming-structure` | **PASS** (suite-scoped; vacuous for this file) |
| 17 | `live-spec-source` | `coverage-semantics` | **PASS** (suite-scoped; vacuous for this file) |

⚠ **Five checks failed on the first run and were repaired rather than waived**: `math-symbol-literals` and `math-backticks` (bare unicode `$`\rightarrow`$`, `$`\Rightarrow`$`, `$`\approx`$` in prose and in six inert code spans — 103 occurrences wrapped in math spans, six code spans rewritten); `anchors-in-document` (15 `<a id="…">` HTML anchors are honoured by GitHub but are **not** headings, so the checker could not resolve them — converted to level-6 headings whose slugs are the link targets); `pedagogy-define-terms` (21 unexpanded acronyms — 17 expanded at first prose use, `CEK` given a notation-table row, and `ACM`/`SIGPLAN`/`SIGACT` plus four conference abbreviations given a table at the head of the References); `algorithms-literate-pseudocode` (the three algorithm fences were tagged `text` and had to be tagged `pseudocode`).

★ **The first draft also used the *inert* inline-math form.** 180 spans were written as a code span containing dollar signs; the correct form is a **backtick span wrapped in dollar signs**, and the backtick-first spelling renders as literal text on GitHub. All 180 were converted before the gate was run. This is the single highest-risk guideline for a document making `$`\Theta(d)`$`-style complexity claims on every page, and it failed silently — no renderer errors, just inert text.

### E.2 What each mechanised check actually proves

Stated precisely, so no check is read as proving more than it does.

* **`code-snippets-valid`** verifies that every fence tagged `rust` **parses** under `rustfmt --edition 2021`, either as a whole file or wrapped in a function body, and that every fence tagged `sh`/`bash` passes `bash -n`. This report contains **5 `rust` fences and 11 `bash` fences**, all of which pass. ⚠ Two further blocks were **retagged because the check refused them** — one to `diff` (it *is* a diff) and one to `text` (a slice element that is not standalone Rust). That is the check working, and it is recorded rather than quietly accommodated. ⚠ **That is parse validity, not compilation and not semantics.** Three `rust` fences are marked **VERBATIM** and are quoted from the tree, where they do compile as part of their crate; two are marked **ELIDED** and are explicitly not compilable in isolation. No snippet in this report is presented as compilable without being one of those two labels.
* **`citations-doi-valid`** resolves every DOI. All **15** resolve, and each was separately confirmed against the Crossref API on 2026-07-29 with its title, container title and year matching the entry as written (Appendix A.8).
* **`diagrams-fully-colored`** checks that the `.puml` sources carry explicit colours; the **intuitive mapping** is a judgement and is stated in §E.3 so a reader can check it rather than take it on trust.
* **`pedagogy-define-terms`** checks acronym *expansion*, not conceptual definition. The conceptual definitions live in the glossary at §2.4, which defines *stack-safe*, *B/level*, *guard page*, *trampoline*, *explicit continuation*, *defunctionalisation*, *CEK machine*, *worklist*, *value stack*, *fire-and-forget*, *SCC*, *converted*/*tripwire*, *anti-vacuity* and $`D_{\max}`$ — every one before its first use outside the glossary.

### E.3 The colour mapping, so it can be checked

One colour per concept, used identically in **all eleven figures** (the four added by the 2026-07-30 revision reuse this mapping without extending it).

> ⚠ **A drift this appendix caught in itself, recorded rather than quietly fixed.** This sentence read *"in all six figures"* while §E.4's own table listed **seven** and the document's closing line said *"seven figures"* — three counts, two of them wrong, inside one appendix whose subject is checking. It is the same failure mode as [§5.7.1](#571-the-register-became-derived-because-every-transcription-drifted)'s four drifting copies, and it is why [Appendix G](#appendix-g--keeping-this-document-current) argues the count should be **derived** from `figures/*.puml` rather than written in prose in three places.


| colour | hex | concept |
|---|---|---|
| red | `#C62828` / `#FFCDD2` / `#EF9A9A` | **native stack** — fixed size, guard-page terminated, exhaustion is a signal |
| blue | `#1565C0` / `#BBDEFB` / `#64B5F6` | **heap** — growable, allocator-backed, exhaustion is an `Err` |
| green | `#2E7D32` / `#A5D6A7` / `#66BB6A` | **converted** — measured depth-independent |
| amber | `#EF6C00` / `#FFCC80` / `#FFE082` | **tripwire** — still $`\Theta(d)`$, held under a ceiling |
| purple | `#6A1B9A` / `#E1BEE7` | **wire bytes** |
| teal | `#00695C` / `#80CBC4` | **the gate / the measuring instrument** |
| slate | `#455A64` / `#ECEFF1` | structural relations and the term itself |

The mapping is intended to be read as *red is the resource you cannot grow, blue is the one you can* — which is the report's whole thesis in two colours.

### E.4 The rendered assets

Every `.puml` was rendered and every `.svg` checked for non-emptiness and for typeset LaTeX, because a `.puml` with real content beside a zero-byte `.svg` is the exact failure this project has already shipped.

| figure | `.svg` bytes | typeset LaTeX images | leaked `<latex>` text |
|---|---:|---:|---:|
| `recursive-vs-trampolined.svg` | 73,054 | 9 | 0 |
| `two-wire-formats.svg` | 45,397 | 5 | 0 |
| `depth-vs-stack-ladder.svg` | 125,558 | 11 | 0 |
| `async-detached-driver.svg` | 54,604 | 6 | 0 |
| `deploy-path-ceilings.svg` | 59,049 | 5 | 0 |
| `heap-where-allocations-moved.svg` | 99,669 | 17 | 0 |
| `generated-drivers-two-ladders.svg` | 63,463 | 5 | 0 |
| ★ `collection-literal-arm-divergence.svg` | 77,299 | 13 | 0 |
| ★ `bisection-instrument-floor.svg` | 285,571 | 16 | 0 |
| ★ `converted-vs-tripwire-cross-repo.svg` | 90,938 | 12 | 0 |
| ★ `spliced-walk-quadratic-time.svg` | 115,365 | 13 | 0 |

★ **Added by the 2026-07-30 revision.** Totals across all eleven: **112 `<latex>` spans in source, 0 leaked into any SVG**, 11–16 distinct colours per source. *(Rendered with the system `plantuml`; verified by counting `<image …>` elements — the typeset-LaTeX images — and searching every SVG for an escaped `<latex>` tag.)*

⚠★ **One tooling limit was found by measurement and is recorded in the source so it is not "repaired" back.** `bisection-instrument-floor` is an **activity** diagram, and **activity-diagram `legend` blocks do not typeset LaTeX at all** in this PlantUML build: a minimal reproduction — one `<latex>` span in an activity diagram's legend — emits **zero** typeset images and leaks the literal tag. This is *not* a syntax error in the expression; the same span typesets correctly in that figure's **notes**, and in the **legend of a component diagram** (`generated-drivers-two-ladders` typesets `\mathtt{Arc::clone}` in a legend cell with zero leaks). $`\Rightarrow`$ The repair is placement, not notation: **mathematics lives in the notes, the activity legend is deliberately plain prose**, and a comment at the head of the `.puml` says so. This is the honest reading of `diagrams-plantuml-latex`: the guideline is satisfied wherever the renderer can satisfy it, and where it cannot, the limitation is named rather than papered over with a unicode literal that would silently violate `math-mathjax` instead.

Command in Appendix A.9. ⚠ **The first render of five of the six emitted `InvocationTargetException` from JLaTeXMath** because the LaTeX carried doubled backslashes; the diagrams still produced non-empty SVGs, so a byte-size check alone would have passed them with their formulae missing. The **typeset-image count** column is what catches that, and it is why it is in the table.

### E.5 The four editorially-judged guidelines

`validate.sh` states that four guidelines are editorial and are *"reviewed by hand; they are not silently assumed to hold."* Their disposition here:

* **`coverage-doc-types`** — the report carries theoretical (§2, §3), design and architectural (§5's *architecture* subsections, §6.1–6.2), engineering (§4, Appendix A), security (§1.1, §5.5.3, §6.4, §8.6.3, §8.6.5) and usage (Appendix A, Appendix B) material. **PASS.**
* **`diagrams-best-types`** — a paired before/after structure diagram for the central transformation, a component diagram for the two wire formats, a categorised inventory for the ladder, a before/after architecture diagram for the async driver, an **activity diagram with swimlanes** for the deploy path (because it is a flow with hand-offs between trust domains), a layered quantity diagram for the heap result, and — added by this revision — a **dispatch-divergence decision tree** for the root cause, an **activity diagram over an interval ladder** for the instrument floor, a **package diagram** for the cross-repository status map, and a **sequence diagram with a per-level repetition frame** for the $`\Theta(d^2)`$ walk. Each choice is justified in its own caption. **PASS.**
* **`diagrams-best-actors`** — the actors are the two *resources* (native stack, heap), the two *codecs*, the five *deploy-path stages*, the *gate*, and — added by this revision — the *classifier and its two arms*, the *instrument's own control flow*, the *two repositories as packages*, and the *absence proof* (`contains_par`) as a first-class participant, rather than files or functions, because the report's claims are about resources, boundaries and decisions. **PASS.**
* **`pedagogy-intuition-rationale`** — every conversion carries a *why this shape rather than the alternatives* passage (§5.1.2, §5.2.1, §5.2.2, §5.3.2, §5.3.3), §6.1 tabulates the choice rule across all of them, and §8.6.1's literate algorithm makes the choice **derivable from the combining operation's algebra** rather than a matter of taste. **PASS.**

---

### E.6 ★★ The complete per-slug audit: all 26 guidelines, BEFORE and AFTER

**BEFORE** = the document at `f1r3node-rust-mettail@8bf298ba`, i.e. as it stood before the 2026-07-30 revision. **AFTER** = this revision.

⚠★★★ **A slug that passes VACUOUSLY is recorded as FAIL.** `math-delimiters` "passing" because a document contains no math is not a pass, and the same rule is applied to suite-scoped checks that no-op on a foreign file. Two rows below were **PASS (vacuous)** in the mechanised gate and are therefore recorded **FAIL** in the BEFORE column — a stricter reading than the gate's own exit status, and the honest one.

| # | slug | category | BEFORE | AFTER | evidence / what changed |
|---:|---|---|:---:|:---:|---|
| 1 | `doc-placement` | Placement | ✅ PASS | ✅ PASS | `docs/design/stack-safety/` with a sibling `figures/`; subject-matter directory, not a dated dumping ground. Unchanged. |
| 2 | `doc-naming-structure` | Placement | ⚠ **FAIL** *(vacuous)* | ✅ PASS | The gate's `roster-coverage` check is **suite-scoped and no-ops on a foreign file** — it proved nothing here. Now discharged on its merits: the file name carries subject + date, all 11 figures are `kebab-case.puml`/`.svg` pairs named for what they show, and §0/§8.6 give the document two scannable indices (fixes, residuals) with stable `SS-*` identifiers. |
| 3 | `coverage-doc-types` | Coverage | ✅ PASS | ✅ PASS | Theory §2–3, design/architecture §5–6, engineering §4/App. A, security §1.1/§5.5.3/§6.4, usage App. A–B. This revision adds security-relevant §8.6.3 (consensus liveness) and §8.6.5 (the `SIGSEGV`-introducing trap). |
| 4 | `coverage-semantics` | Coverage | ⚠ **FAIL** *(vacuous)* | ✅ PASS | The gate's `live-spec-source` check is likewise **suite-scoped and vacuous here**. Discharged on merits: the *intended behaviour* of a converted driver is now specified — the invariant "the driver contains no call to itself", the algebra$`\rightarrow`$shape rule, and the anti-vacuity obligation — in §8.6.1's literate algorithm, rather than only exhibited by example. |
| 5 | `coverage-syntax` | Coverage | ⚠ **FAIL** *(unaddressed)* | ✅ PASS | **Not named anywhere in the prior Appendix E**, so it was neither checked nor waived. Now addressed where syntax is genuinely load-bearing: the E0624/E0606 diagnostics are given their *syntactic* cause (§8.6.1a — `Option::len` vs the inner `Vec`; `&Vec<T>` is not `&[T]`), and prost's `StartGroup` arm is quoted **VERBATIM** with its recursion identified (§8.6.5). |
| 6 | `pedagogy-presentation` | Pedagogy | ⚠ **FAIL** *(unaddressed)* | ✅ PASS | Also unnamed before. The revision adds all six required modes in the new material: worked examples, 4 new diagrams, display-math derivations, 2 literate-pseudocode algorithms, a VERBATIM code snippet, and citations — rather than prose alone. |
| 7 | `diagrams-plenty` | Diagrams | ✅ PASS | ✅ PASS | **7 $`\rightarrow`$ 11 figures.** |
| 8 | `diagrams-best-types` | Diagrams | ✅ PASS | ✅ PASS | Each of the 4 new figures states its type **and why that type** in its caption (§E.5). |
| 9 | `diagrams-best-actors` | Diagrams | ✅ PASS | ✅ PASS | New actors are decisions, resources, repositories and an *absence proof* — not files. |
| 10 | `diagrams-pgmcp-catalog` | Diagrams | ⚠ **FAIL** *(unaddressed)* | ✅ PASS | Unnamed before. The catalog's guidance is now **applied and its application recorded**: PlantUML chosen for all 11 (byte-reproducible, renders LaTeX), with the one case where the tooling could **not** deliver — activity-diagram legends do not typeset LaTeX — measured on a minimal case, worked around, and **recorded in the `.puml` source** so it is not "fixed" back. |
| 11 | `diagrams-prefer-plantuml` | Diagrams | ✅ PASS | ✅ PASS | **11/11 PlantUML; zero Mermaid** in the document and in `figures/` (verified by search). |
| 12 | `diagrams-plantuml-latex` | Diagrams | ⚠ **FAIL** *(unaddressed as a slug)* | ✅ PASS | E.4 checked *typeset-image counts* but never named this slug, so LaTeX **usage** was never adjudicated. Now: **112 `<latex>` spans across 11 sources, 0 leaked** into any SVG — including the new figures' formulae ($`\Theta(d^2)`$, the bisection midpoint, $`\sum(d-i)`$). |
| 13 | `diagrams-fully-colored` | Diagrams | ✅ PASS | ✅ PASS | 11–16 distinct hex colours per source; the §E.3 mapping is unchanged and the 4 new figures use it **identically**. |
| 14 | `diagrams-complete` | Diagrams | ✅ PASS | ✅ PASS | Every new figure carries a legend mapping every cell colour, and notes stating the finding rather than only the shape. |
| 15 | `diagrams-flows` | Diagrams | ⚠ **FAIL** *(unaddressed as a slug)* | ✅ PASS | Unnamed before, though the deploy-path swimlane figure would have satisfied it. Now explicit: the two new *flow* figures are end-to-end — the instrument diagram draws **the branch that is not taken** (without which the floor is invisible), and the sequence diagram draws the **per-level repetition frame** (without which the quadratic is invisible). |
| 16 | `math-mathjax` | MathNotation | ⚠ **FAIL** *(measured)* | ✅ PASS | ⚠ Gate check `math-symbol-literals` **fails on the committed document** (two bare right-double-arrow operators introduced by `cebdedbb`/`23eff25a` after the prior green run — see §E.1). Repaired, together with 33 bare operators in the revision's own new material, all now inline math spans. |
| 17 | `math-delimiters` | MathNotation | ✅ PASS | ✅ PASS | ★ The highest-risk slug here. All new inline math uses the **backtick-span-wrapped-in-dollar-signs** form; all new display math uses **fenced blocks with info-string `math`** (5 added). No `$…$` or `$$…$$` anywhere. No ASCII letter abuts an opening delimiter. ⚠ **The hazard is DESCRIBED in prose in §E.6a and never instantiated**, because an illustration of this hazard is the hazard. |
| 18 | `math-backticks` | MathNotation | ⚠ **FAIL** *(measured)* | ✅ PASS | ⚠ Failed on the committed document for the same two expressions as row 16. Repaired; no expression in the document is now left as bare prose or as an inert code span. |
| 19 | `citations-exist` | Citations | ✅ PASS | ✅ PASS | 15 references, each used at the point it is cited. This revision adds no new external claims requiring a citation; its claims are to **source and commits**, which are pinned at fixed SHAs instead. |
| 20 | `citations-doi-links` | Citations | ✅ PASS | ✅ PASS | Unchanged. |
| 21 | `citations-doi-valid` | Citations | ✅ PASS | ✅ PASS | All 15 DOIs resolved and Crossref-confirmed 2026-07-29 (App. A.8). ⚠ Not re-resolved for this revision — **no DOI was added or altered**, so the prior verification still covers the set. |
| 22 | `pedagogy-define-terms` | Pedagogy | ⚠ **FAIL** *(incomplete for the new material)* | ✅ PASS | The §2.4 glossary was complete for the *old* material. The revision's vocabulary was undefined, so **five terms were added and defined before first use**: **slope**, **flat ladder**, **instrument floor**, **ratchet**, and **`SIGSEGV` vs `SIGABRT`**. (**B/level**, **converted/tripwire** and **work stack** were already defined.) |
| 23 | `pedagogy-intuition-rationale` | Pedagogy | ✅ PASS | ✅ PASS | Strengthened: §8.6.1 makes the driver-shape choice **derivable** from the algebra of the combining operation, so a reader can re-derive it rather than trust it. |
| 24 | `pedagogy-logical-flow` | Pedagogy | ✅ PASS | ✅ PASS | §8.6 is placed with the residuals it belongs to, indexed in the TOC to sub-section depth, and cross-linked from the abstract, §0, §5 preamble and conclusion 7 — so the incompleteness is reachable from every entry point, not only from §8. |
| 25 | `algorithms-literate-pseudocode` | AlgorithmsCode | ✅ PASS | ✅ PASS | Was 3 algorithms in Knuth form. **Two added**: the conversion algorithm and the bisection instrument, both as named, refined fragments in `pseudocode` fences. |
| 26 | `code-snippets-valid` | AlgorithmsCode | ✅ PASS | ✅ PASS | One `rust` fence added, marked **VERBATIM** — quoted from `prost-0.14.4/src/encoding.rs`, where it compiles as part of its crate. It is presented as *quoted*, not as compilable in isolation, per §E.2's two-label rule. |

**Tally — BEFORE: 16 PASS / 10 FAIL. AFTER: 26 PASS / 0 FAIL.**

The 10 BEFORE failures decompose into three distinct kinds, and the distinction matters because each has a different remedy:

| kind | count | rows | why it failed | remedy |
|---|---:|---|---|---|
| **measured regression** | 3 | 16, 18, 22 | the mechanised gate **actually fails** on the committed document — the recorded 17/17 had rotted (§E.1) | re-run the gate in CI on every commit touching the file |
| **vacuous pass** | 2 | 2, 4 | the gate's check is **suite-scoped and no-ops** on a foreign file, so it proved nothing while reporting green | judge on merits, and never count a no-op as a pass |
| **never named** | 5 | 5, 6, 10, 12, 15 | the slug appeared **nowhere** in the prior appendix, so it was neither checked nor waived | audit against the canonical 26-slug list, not against the gate's check list |

★ **The "never named" kind is the most dangerous and the least visible**: an unnamed slug produces **no diagnostic at all**, so it cannot be noticed by running anything, and a 17/17 green result sits comfortably beside it. ⚠ That is the same structural blindness [§8.6.7](#867--157--a-transcribed-ceiling-and-a-tripwire-that-cannot-see-it-drift) documents for the ceiling tripwire — **a check that cannot express a failure will report success forever** — and it is why this appendix now audits against the *guideline list* rather than against the *checker list*. The gate has 17 checks; the guidelines have 26 slugs, and the gap between those two numbers is exactly where the five hid.

#### E.6a The `math-delimiters` hazard, described rather than shown

★★ **This subsection deliberately contains no example of the wrong form**, because the wrong form is contagious: an illustration of it renders as literal text, and three prior attempts on this campaign instantiated the hazard *inside its own remedy*. The rule, stated in words:

* **Inline math** is a **code span** whose content is the LaTeX, with a **dollar sign immediately outside each backtick** — dollar, backtick, LaTeX, backtick, dollar. The *backtick-first* spelling (backtick, dollar, LaTeX, dollar, backtick) is an **inert code span**: it renders as visible literal text with no error anywhere, which is why it survived 180 occurrences in this document's first draft undetected.
* **Display math** is a **fenced block whose info-string is the word `math`** — not a dollar-delimited paragraph.
* **Never** the bare dollar-delimited forms, inline or display: GitHub's CommonMark pass strips backslash escapes — underscore, brace, semicolon, comma, hash — *before* MathJax parses them, corrupting the expression either loudly or silently.
* A **literal dollar sign** in prose goes in an ordinary code span.
* **Never let an ASCII letter abut the opening delimiter**; separate them with a space or restructure the sentence.

★★ **This subsection's first draft instantiated the hazard it describes — twice.** Naming the count of bad operators while putting the operator itself inside an ordinary code span produced an **inert code span**, which is one of the two silent failure forms described above; the gate flagged it at two lines, and two further occurrences were bare prose. ⚠ The lesson is not carelessness but **contagion**: a sentence *about* a delimiter hazard reaches for the symbol, and reaching for the symbol is the hazard. The repair is to **name the operator in words** — “right-double-arrow” — and never to typeset it outside a math span. This is the fourth occasion in this campaign on which the hazard appeared inside its own remedy, and the first on which the remedy was written to *describe* rather than to *show*.

$`\Rightarrow`$ The mechanised checks `math-delimiters` and `math-github-renderable` (§E.1 rows 3–4) exist precisely because the failure is **silent**, and they are the reason this document's 180 inert spans were caught before publication rather than after.

---

## Appendix F — the per-fix template (fill this in; do not invent a shape)

★ **Copy this block verbatim** when a stack-safety fix lands. Every heading is mandatory; a heading with nothing under it is answered with **`NOT MEASURED — <reason>`** or **`n/a — <reason>`**, never deleted. A section that disappears is indistinguishable from a section nobody thought about, which is the failure mode [§5.3.1](#531-the-baseline-what-had-never-been-measured) is about.

Add the row to [§0](#0-the-fix-register--the-scannable-index) **in the same commit**. Allocate the next free identifier in the family (`SS-A…` core traversals, `SS-B…` evaluator/async, `SS-C…` codecs, `SS-D…` deploy path, `SS-E…` instrument, `SS-F…`/`SS-G…` `mettail-rust`, `SS-X…` rejected, **`SS-Y…` a live, unrepaired defect**). **Identifiers are never reused, even after a fix is superseded.**

★★ **`SS-Y…` — the family for bad news, and the rules that keep it honest.** It was added by the 2026-07-30 revision because the register could record a fix, a partial fix, or a rejected candidate, but **not a live defect introduced by a fix already in the register** — so `SS-Y1` (#197, a defect inside `SS-G4`'s own commit) had nowhere to go and would have been recorded only in prose, or not at all.

1. An `SS-Y` row is **never** `class change: yes`.
2. It is discharged **only by a commit that repairs it**, never by deletion; when repaired, the row stays and gains the repairing commit, exactly as a superseded figure is annotated rather than overwritten.
3. It **must** name the target that fails and the diagnostic code, so the claim is falsifiable by anyone with the tree.
4. ⚠ **If a fix's own commit introduces a defect, both rows are mandatory** — the `SS-*` row for the fix *and* the `SS-Y` row for the defect — and each cross-references the other. A register that shows only the fix is the failure mode [§8.6](#86--the-open-residual-register--what-this-report-does-not-establish) exists to prevent.

```text
### 5.N  <FAMILY> — <one-line name of the traversal>            [SS-??]

#### 5.N.1 The defect
  what recursed  ·  why it was unbounded  ·  file:line
  what a user input that reached it looked like (bytes of source, a term shape)
  DERIVED / MEASURED tag on every clause

#### 5.N.2 The architecture of the repair, and why THIS shape
  which shape:  explicit-continuation driver | trampoline | worklist | arena |
                representation change | call-site deletion
  ★ the alternatives, and why each was rejected      <- the part a maintainer needs
  what the work item carries, and why not more
  invariant that keeps the stacks in step

#### 5.N.3 How the fix was made
  the transformation, before/after, each snippet marked VERBATIM or ELIDED

#### 5.N.4 Results
  | metric                  | before | after | provenance |
  | B/level, release        |        |       | (q)/(f) + log path |
  | B/level, debug          |        |       |                    |
  | max depth, 2 MiB worker |        |       |                    |
  | wall clock              |        |       | n>=3, sd, overlap  |
  | heap: peak / blocks     |        |       | massif + DHAT      |   <- ser/de only
  | where the allocations moved                                   |

#### 5.N.5 What it cost
  throughput  ·  allocation  ·  complexity  ·  intercept
  ★ a stack-safety fix that is SLOWER is still correct — state the number

#### 5.N.6 What is still recursive
  the honest residual, with its own B/level and its owner

#### 5.N.7 Anti-vacuity
  which control the checker must REJECT, and the log line showing it did
```

**Two rules that are not negotiable, because both have already been broken in this campaign:**

1. **A number without a subject is not a number.** Every figure names *which traversal, which ladder, which profile*. [§5.10](#510--the-generated-trait-implementations-and-the-clone-question) exists because two task summaries reported `2,852` and `clone` without saying which was which.
2. **A traversal enters [§0](#0-the-fix-register--the-scannable-index) as a class change only by being converted**, never by having a ceiling lowered — the same admission rule the gate enforces on `CONVERTED_DEPTH`. `SS-D5` is the worked example: a $`19.5\times`$ improvement that is still, correctly, *not* a class change.

---

## Appendix G — keeping this document current

★ **The problem, stated as this document's own evidence rather than as a worry.** [§5.7.1](#571-the-register-became-derived-because-every-transcription-drifted) records that the converted-subject list existed in four places and **every copy drifted**, twice within an hour of being reconciled. [§5.5.4](#554-results-and-a-control-that-behaved-exactly-as-predicted) records that the gate's own `BUILD_DEPTH_INVENTORY` carries **283** where the tree now measures **274**. [§5.10.5](#5105-the-verdict-and-the-correction-to-the-tracker) records a third instance in the task tracker. $`\Rightarrow`$ **A results document maintained by remembering will rot exactly the way those three did.** What follows is a *design*, not an implementation.

### G.1 The precedent, and whether it transfers

`docs/design/audits/theta-depth-traversals-2026-07-26.md` is **mechanically bound to the gate**: it carries one fenced `GATE-SUBJECTS` block, `rholang/tests/stack_depth_gate.rs::the_audit_agrees_with_the_gate` parses that block, and the test **fails at the commit that separates them**. That is a working example of a document a test reads, and it works because the audit's block is *generated output* with exactly one upstream source of truth.

★ **The same shape transfers to this report only in part, and the boundary is worth stating precisely.**

| this document's content | bindable? | why |
|---|---|---|
| [§0](#0-the-fix-register--the-scannable-index)'s $`B_1`$ column for **converted** rows | ✅ **yes** | the gate already publishes `CONVERTED_DEPTH` / `CONVERTED_WIDTH`; membership is a set comparison, identical to the audit's block |
| §0's $`B_1`$ column for **tripwire** rows | ✅ **yes** | `theta_depth_tripwire` *prints* each subject's measured slope; the numbers here are transcriptions of that output |
| the deploy-ceiling numbers in [§5.5](#55-family-d--the-deploy-path) | ✅ **yes** | `deploy_depth_ceiling.rs` prints them; **this is the drift already observed** |
| the S0 baseline table in [§5.3.1](#531-the-baseline-what-had-never-been-measured) | ✅ **yes** | `four_quadrant_s0_baseline` emits a **CSV** between `S0-BEGIN` / `S0-END` markers — the easiest binding in the document |
| the massif/DHAT figures in [§5.3.4](#534--the-malloc-profile--where-the-allocations-moved) | ⚠ **partly** | reproducible on demand, but Valgrind runs are minutes-long; bind the *invariants* (blocks/call, op entries/level), not the byte totals |
| the throughput figures in [§5.4](#54-throughput-and-cpu-profile-of-the-codec-conversion) | ❌ **no** | machine-dependent; §7.2 already says so. Bind the **direction and significance**, never the nanoseconds |
| the architecture prose in every §5.N.2 | ❌ **no** | editorial by nature; covered by the template, not by a test |

### G.2 The proposed check — `the_stack_safety_report_agrees_with_the_gate`

The design, in the idiom the audit's check already establishes:

1. **This document grows one fenced block**, `<!-- SS-REGISTER:BEGIN -->` … `<!-- SS-REGISTER:END -->`, holding §0's table in a machine-readable form: `ID | commit | subject | B_before | B_after | class_change`.
2. **A new test in `rholang/tests/stack_depth_gate.rs`** parses that block and asserts, in ascending order of cost:
   * every subject marked `class_change = yes` and living in this repository is in `CONVERTED_DEPTH` or `CONVERTED_WIDTH` — a **set comparison, no measurement**;
   * every subject marked `no` with a numeric $`B_1`$ is in `TRIPWIRE_DEPTH`, and its $`B_1`$ **matches the slope the tripwire printed on this run**, to the bisection resolution;
   * every `⌀` corresponds to a row in [§5.9](#59-measurements-that-could-not-be-obtained), so an unmeasured figure cannot be quietly dropped rather than reported.
3. **Both failure directions are named in the message**, as `ee1dfdad` requires: a subject in the gate but not the report means *the report is stale*; a subject in the report but not the gate means *a claim has lost its evidence*.
4. ⚠ **It must be able to go red, and be shown red.** Perturb one $`B_1`$ in the block and require the assertion to fail naming that row — the discipline of [§5.7.4](#574-the-checkers-were-given-subjects-they-must-reject-dd0ba13f). Without that leg this check is [§5.7.4](#574-the-checkers-were-given-subjects-they-must-reject-dd0ba13f)'s `println!` again.

### G.3 What the check deliberately does **not** cover, and the residual risk

* It cannot verify a **cross-repository** row. `SS-F1` and `SS-G1`/`SS-G2` are `mettail-rust` work with their own gate, and a test in `f1r3node` cannot read that tree. The honest mitigation is that those rows carry their source gate's name, and that [§5.10.10](#51010--the-gap-is-now-closed-by-measurement--and-eight-of-the-nine-drivers-are-sloped) states plainly which of them are unmeasured.
* It cannot verify **prose**. The architecture sections, the rejected alternatives and the discussion are editorial and stay editorial.
* It cannot verify a **tracker** ([§5.10.5](#5105-the-verdict-and-the-correction-to-the-tracker)). Nothing in this repository reads the task tracker, which is precisely why the tracker is the copy that drifted furthest.

⚠ **Until this check exists, this report is exactly the discipline-dependent artefact [§1.2](#12-what-makes-a-fix-a-fix) argues against** — and it already carries one demonstrated drift ([§5.5.4](#554-results-and-a-control-that-behaved-exactly-as-predicted)) that a reader must not mistake for a maintained figure. The **`S0-BEGIN`/`S0-END` CSV binding is the cheapest first step** and would cover ten of the document's most-cited numbers on its own.

### G.4 The maintenance contract, in four lines

1. A stack-safety fix lands $`\rightarrow`$ add its [Appendix F](#appendix-f--the-per-fix-template-fill-this-in-do-not-invent-a-shape) section **and** its [§0](#0-the-fix-register--the-scannable-index) row, in the same commit.
2. A figure changes $`\rightarrow`$ change it **with its log path**, and move the superseded value into the prose that names it superseded. This campaign's convention is to **annotate, never overwrite** (`E73`, `E86`, `E89` are all superseded in place).
3. A measurement cannot be obtained $`\rightarrow`$ it goes in [§5.9](#59-measurements-that-could-not-be-obtained) with its reason. **⌀ is a valid entry; a blank is not.**
4. A claim loses its evidence $`\rightarrow`$ the claim comes out, not the caveat.

---

*This report documents work in `f1r3node-rust-mettail@feature/mettail` and `mettail-rust@feature/rho-native-set-automata`. No source file was modified in its preparation; the document and its **eleven** figures are the only artefacts created. (The figure count read "seven" until the 2026-07-30 revision added four and reconciled the three places it was written — see [§E.3](#e3-the-colour-mapping-so-it-can-be-checked).)*

*★ It is a **living document**. Adding a fix is [Appendix F](#appendix-f--the-per-fix-template-fill-this-in-do-not-invent-a-shape); keeping it honest is [Appendix G](#appendix-g--keeping-this-document-current).*
