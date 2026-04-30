---
title: Rholang `where` clauses on receives, `where` guards on match cases, and match-as-boolean-expression
status: draft
author: claude-session
date: 2026-04-29
related-docs:
  - docs/rholang/rholangtut.md
  - docs/rholang-language-analysis.md
---

# Rholang language extension: `where` clauses and match-as-bool-expr

## 1. Goals

Three related extensions to the Rholang surface syntax and semantics:

### 1.1 Guarded receives

Today:

```
for (
  ptrns_{1,1} <- x_{1,1} & ... & ptrns_{1,n_1} <- x_{1,n_1};
  ...;
  ptrns_{k,1} <- x_{k,1} & ... & ptrns_{k,n_k} <- x_{k,n_k}
) { P }
```

Extended:

```
for (
  ptrns_{1,1} <- x_{1,1} & ... & ptrns_{1,n_1} <- x_{1,n_1} where cond_1;
  ...;
  ptrns_{k,1} <- x_{k,1} & ... & ptrns_{k,n_k} <- x_{k,n_k} where cond_k
) { P }
```

`cond_j` is a boolean process that may reference any variable bound by
`ptrns_{i,m}` with `i ≤ j`. The receive only commits when every spatial pattern
matches AND every `cond_j` evaluates to `true`.

### 1.2 Guarded match cases (with fall-through)

Today:

```
match e {
  pat_1 => P_1
  ...
  pat_n => P_n
}
```

Extended:

```
match e {
  pat_1 where cond_1 => P_1
  ...
  pat_n where cond_n => P_n
}
```

If `pat_i` matches `e` and `cond_i` evaluates to `true`, the case fires.
If `pat_i` matches but `cond_i` is `false`, fall through to case `i+1`.

### 1.3 Match as a boolean expression

A `match` block in which every right-hand side is a boolean expression is
itself a boolean expression and may appear in any expression context (e.g.,
inside `if`, inside another `where`, as an argument to a boolean operator).

## 2. Touchpoints

### 2.1 External parser repo (`rholang-rs`, sibling at `/Users/stay/greg/f1r3fly/rholang-rs`)

The parser is pinned by `rev = "d25f953a"` in
`rholang/Cargo.toml` and `rspace++/Cargo.toml`. We must fork/branch
`rholang-rs`, land grammar + AST changes there, then bump the pin in this
workspace.

| File | Change |
|------|--------|
| `rholang-tree-sitter/grammar.js` | Add `where` token; extend `case` and `receipt` rules. |
| `rholang-tree-sitter/src/...` (regenerated) | Generated parser tables. |
| `rholang-parser/src/ast.rs` | Add `guard: Option<AnnProc<'ast>>` to `Case`; introduce a `Receipt` wrapper carrying its binds and an optional guard. |
| `rholang-parser/src/parser/...` | Map new tree-sitter nodes into the AST. |
| `rholang-parser/tests/...` | Round-trip tests for new syntax. |
| `rholang-jetbrains-plugin/...` | Highlighting/keyword update if the plugin enumerates keywords. |

### 2.2 This workspace (`f1r3node-rust`)

| File | Change |
|------|--------|
| `models/src/main/protobuf/RhoTypes.proto` (`Receive`, `MatchCase`, `Expr`) | Extend message schema; regenerate via `models/build.rs`. |
| `rholang/src/rust/interpreter/compiler/normalizer/processes/p_input_normalizer.rs` | Normalize receive `where` clauses; conjoin per-receipt guards into one `Receive.condition`; perform syntactic sublanguage check. |
| `rholang/src/rust/interpreter/compiler/normalizer/processes/p_match_normalizer.rs` | Normalize per-case guards; classify match-as-expr; emit `EMatchExpr` when applicable. |
| `rholang/src/rust/interpreter/compiler/normalizer/processes/p_if_normalizer.rs` | Apply the same sublanguage check to `if` conditions for parity (see §3.8). |
| `rholang/src/rust/interpreter/compiler/receive_binds_sort_matcher.rs` | No change — `pre_sort_binds` still permutes binds; the merged `Receive.condition` references binds by their post-sort indices. |
| `rholang/src/rust/interpreter/reduce.rs` (`eval_match`, receive plumbing) | Match-case guard evaluation + fall-through; `EMatchExpr` evaluation. Receive guards do **not** run here — they run inside the rspace matcher. |
| **NEW shared crate `rho-pure-eval`** | Pure-functional evaluator for the guard sublanguage. Depended on by both `rholang` (for `if` and match-case guards) and `rspace++` (for receive guards inside the matcher). See §3.9. |
| `rspace++/src/rspace/match.rs` and the rholang spatial matcher (`rholang/src/rust/interpreter/matcher/spatial_matcher.rs`) | Extend the matching API to take an optional guard predicate. After spatial bindings are produced, evaluate the guard via `rho-pure-eval`; if false, treat as no match (don't consume). |
| `rspace_plus_plus_rhotypes` | Type bridge updates if the guard predicate uses types defined here. |
| `rholang/src/rust/interpreter/compiler/normalize.rs` | New helper `is_pure_boolean_expr_par(&Par) -> bool` for the syntactic sublanguage check. |
| Tests (see §6) | Normalizer tests, `reduce_spec.rs` integration tests, and rspace-level matcher tests for guard evaluation. |

## 3. Design

### 3.1 Surface syntax

#### Receive guards

Tree-sitter changes to `rholang-tree-sitter/grammar.js`:

```js
// before
receipts: $ => semiSep1($.receipt),
receipt: $ => conc1(choice($.linear_bind, $.repeated_bind, $.peek_bind)),

// after
receipts: $ => semiSep1($.receipt),
receipt: $ => seq(
    field('binds', conc1(choice($.linear_bind, $.repeated_bind, $.peek_bind))),
    optional(seq('where', field('guard', $._proc)))
),
```

`where` becomes a contextual keyword (not reserved at top level) — same approach
as `matches`. Confirm precedence so `_proc` after `where` is lexed up to the
next `;` or `)`. We may need a higher-precedence non-terminal for the guard
expression to avoid greedily eating the closing `)`.

#### Match guards

```js
// before
case: $ => seq(field('pattern', $._proc), '=>', field('proc', $._proc)),

// after
case: $ => seq(
    field('pattern', $._proc),
    optional(seq('where', field('guard', $._proc))),
    '=>',
    field('proc', $._proc)
),
```

#### AST (`rholang-parser/src/ast.rs`)

Introduce a wrapper for receipts so we can carry the guard alongside the bind
list (today the parser hands the normalizer
`SmallVec<[SmallVec<[Bind; 1]>; 1]>` — a 2-D vec of binds). Replace the inner
SmallVec with:

```rust
pub struct Receipt<'ast> {
    pub binds: SmallVec<[Bind<'ast>; 1]>,
    pub guard: Option<AnnProc<'ast>>,
}
```

Extend `Case`:

```rust
pub struct Case<'ast> {
    pub pattern: AnnProc<'ast>,
    pub guard: Option<AnnProc<'ast>>,
    pub proc: AnnProc<'ast>,
}
```

These are additive; `guard: None` reproduces today's behaviour.

### 3.2 IR (`models/src/main/protobuf/RhoTypes.proto`)

Receive: each `Receive` is one receipt's worth of binds (because `;` desugars
to nested `for`s — see §3.3). It carries one optional guard.

```protobuf
message Receive {
  repeated ReceiveBind binds = 1;
  Par body = 2;
  bool persistent = 3;
  bool peek = 4;
  int32 bindCount = 5;
  bytes locallyFree = 6;
  bool connective_used = 7;
  Par condition = 8;            // NEW. Empty Par = no guard. Evaluated under this receive's bindings.
}
```

MatchCase:

```protobuf
message MatchCase {
  Par pattern = 1;
  Par source = 2;
  int32 freeCount = 3;
  Par guard = 4;                // NEW. Empty Par = unguarded.
}
```

New `Expr` oneof variant for match-as-bool-expr:

```protobuf
message EMatchExpr {
  Par target = 1;
  repeated MatchCase cases = 2; // each case.source must reduce to a Par with a single boolean Expr
  bytes locallyFree = 3;
  bool connective_used = 4;
}

message Expr {
  oneof expr_instance {
    ...
    EMatchExpr e_match_expr = NN;
  }
}
```

New top-level Par variant for first-class `if`:

```protobuf
message If {
  Par condition = 1;
  Par if_true = 2;
  Par if_false = 3;          // empty Par (= Nil) when no else clause
  bytes locallyFree = 4;
  bool connective_used = 5;
}

message Par {
  ...
  repeated If conditionals = NN;  // field name picked to avoid Rust `if` keyword
}
```

Reducing an `If` evaluates the condition; on `true` runs `if_true`, on
`false` runs `if_false`, otherwise raises a runtime error. See §3.10.

Schema choice notes:
- One `Par condition` per `Receive` is enough because each receipt is its own
  `Receive` after `;`-nesting desugaring. `&`-joined binds inside a single
  receipt share the same guard (and `pre_sort_binds` may reorder them — the
  guard references variables by post-sort De Bruijn level).
- `EMatchExpr` is structurally similar to `Match` but lives inside `Expr`, so
  it's available wherever expressions are. The runtime treats it as a value.
- The guard sublanguage (§3.8) is enforced syntactically at normalize time,
  so the IR `condition` / `guard` Pars are guaranteed to be in the
  pure-expression subset by construction. The rspace matcher and
  `rho-pure-eval` may rely on this invariant.

### 3.3 Normalizer: `;` desugars to nested `for`, plus per-receipt guards

**Language change.** `for (R_1; R_2; …; R_k) { P }` desugars to
`for (R_1) { for (R_2) { … for (R_k) { P } … } }`. Each receipt is its own
sequential `for`. `&`-joined binds inside a single receipt remain atomic.
The current Rust normalizer (`p_input_normalizer.rs:216-219`) flattens both
`;` and `&` into one atomic `Receive` — that is replaced by this desugaring.
This is a breaking change for any program that relied on `;` for atomic join
across mixed bind kinds; see §7.4.

In `p_input_normalizer.rs::normalize_p_input`:

1. **Desugar `;` to nested `for`s before normalization.** When `receipts.len() > 1`,
   peel off the first receipt `R_1` and re-build the AST as
   `for (R_1) { for (R_2; …; R_k) { P } }`, then recurse on the new outer
   `for`. The recursion bottoms out at single-receipt `for`s.
2. **Single-receipt path** (the only path the rest of the normalizer ever
   sees): one receipt's worth of `&`-joined binds, plus an optional guard
   from the AST. Normalize as today through `pre_sort_binds` and free-map
   merge.
3. **New step**, after the binds are normalized and free maps merged, before
   normalizing the body:
   - If the receipt has a guard `cond`:
     - Normalize `cond` against the merged free map of this receipt's binds.
     - Run `is_pure_boolean_expr_par(cond_par)` (§3.8); on failure error
       `InterpreterError::NonPureGuard`.
     - Variable indices in `cond_par` automatically reference the post-sort
       merged-map levels because we normalize after `pre_sort_binds`.
   - Otherwise, set `Receive.condition = Par::default()`.
4. Continue with body normalization as today (line 424+).

The `i ≤ j` scoping rule is automatic: under nested `for`s, `cond_j` lives
inside the j-th `for`, so it has lexical access to bindings from receipts
1..j and no access to receipts j+1..k. No special check is needed.

#### Edge cases

- `cond` contains a forbidden Par variant (send/receive/new/bundle/…) →
  `InterpreterError::NonPureGuard` from `is_pure_boolean_expr_par` (§3.8).
- `cond` references a free variable not in scope → existing "unbound" error.
- Variable shadowing across nested receipts — the existing free-map merge
  shadowing detection covers this; no new error type needed.

### 3.4 Normalizer: match guards & match-as-expr

In `p_match_normalizer.rs::normalize_p_match`:

1. Lift each `Case` to `(pattern, guard, body)` (was `(pattern, body)`).
2. After normalizing the pattern (today, line 41–50), build the case
   environment by absorbing the pattern's free map (line 52–54).
3. **New step**: if `guard.is_some()`, normalize it under `case_env`. Run
   `is_pure_boolean_expr_par(guard_par)` (§3.8); on failure error
   `InterpreterError::NonPureGuard`.
4. Continue normalizing the body under `case_env`.
5. Store the normalized guard on the new `MatchCase.guard` field.

For match-as-expr classification:

- After all cases have been normalized, compute
  `all_bodies_pure_bool = cases.iter().all(|c| is_pure_boolean_expr_par(&c.source).is_ok())`.
- If `all_bodies_pure_bool`, emit `Par { exprs: [Expr::EMatchExpr(...)] }`
  instead of `Par { matches: [Match] }`. We always emit `EMatchExpr` when the
  predicate holds — `EMatchExpr` is a strict refinement of `Match` for
  pure-bool cases and is usable in either process or expression context.
- This avoids threading an "expression context" flag through the normalizer.

### 3.5 Runtime: Option 3 — guards inside the rspace matcher

We chose **Option 3**: receive guards are part of pattern matching. A receive
commits iff its spatial patterns match AND its guard evaluates to `true` under
the resulting bindings. Guard failure is indistinguishable from pattern
non-match: no consumption, no observable side effects, the continuation stays
installed. This preserves rspace's atomicity contract and keeps the interpreter
out of the commit path (important for casper replay determinism).

#### Matcher API change (`rspace++` and rholang spatial matcher)

Today the matcher takes `(patterns, data) -> Option<bindings>`. Extend to:

```rust
// Conceptual signature; final API to be designed in implementation
fn match_with_guard(
    patterns: &[Pattern],
    data: &[Datum],
    guard: Option<&GuardPredicate>,
) -> Option<Bindings>;
```

Internally:
1. Run today's spatial match → candidate bindings.
2. If `guard.is_some()`, evaluate it via `rho-pure-eval` (§3.9) using the
   candidate bindings as the environment.
3. Return `Some(bindings)` only if the guard returned `true`. Otherwise return
   `None` — rspace treats it identically to a structural mismatch.

Two implementation notes for rspace:
- The matcher is called both at produce-time (looking for a waiting
  continuation that accepts the new datum) and at consume-time (looking for
  data that satisfies a new continuation). Guard evaluation must run on both
  paths.
- Where multiple candidate datums exist, the matcher iterates them; under
  guards it must keep iterating past guard-fail candidates rather than
  concluding "no match." Confirm the existing iteration loop does not
  short-circuit on first spatial match.

#### Receive evaluation in `reduce.rs`

Almost no change. The interpreter installs the continuation as today; the
`Receive.condition` is passed through to rspace as part of the continuation
descriptor, packaged for evaluation by the matcher. When the matcher commits,
the interpreter receives a `(continuation, bindings)` pair already known to
satisfy the guard, and runs the body unchanged.

#### Match guard evaluation (`reduce.rs::eval_match`)

Today (line 1297–1356), `eval_match` iterates cases, calls
`SpatialMatcherContext::spatial_match_result`, and on success binds vars and
runs the case body. Change:

```rust
for case in &mat.cases {
    match spatial_match(target, &case.pattern) {
        Some(bindings) => {
            let case_env = env.extend(bindings);
            if !case.guard.is_default() {
                // Match-case guards may run via rho-pure-eval too, since the
                // guard is in the same sublanguage as receive guards.
                let g = rho_pure_eval::eval_bool(&case.guard, &case_env)?;
                if !g { continue; }   // fall through
            }
            return self.eval_par(&case.source, case_env).await;
        }
        None => continue,
    }
}
// no case matched -> existing "match exhausted" semantics
```

#### `EMatchExpr` evaluation

Add a new arm in expression evaluation. Mirrors `eval_match` but the chosen
case's `source` is evaluated as an expression and the resulting bool Par is
returned as the value of the EMatchExpr. If no case matches, semantics: return
an error (`InterpreterError::NonExhaustiveMatchExpr`) — match-as-expr is
strict. Because `EMatchExpr` is in the guard sublanguage (§3.8), it can also
appear as a sub-term of a receive guard, in which case it is evaluated
recursively by `rho-pure-eval` inside the matcher.

### 3.6 Sort, hashing, canonical form

`receive_binds_sort_matcher::pre_sort_binds` produces a deterministic ordering
used for canonical form (block hashing). The merged-conjunction approach keeps
sort-friendliness: the binds are sorted as before, and the conjunction lives
on the `Receive.condition` field which is hashed atomically. Variable indices
inside `condition` use the post-sort merged-map indexing, so they're already
canonical.

For `MatchCase.guard`: cases in `match` are *not* reordered (case order is
significant under fall-through), so guard hashing is straightforward.

For `EMatchExpr`: same as `Match` — case order significant.

### 3.7 Bind-type semantics for guards

The three Rholang receive flavours interact with guards uniformly because
guards live inside the matcher and a guard-failure is treated as a no-match:

| Bind | Symbol | Guard-true (matched) | Guard-false (not matched) |
|------|--------|----------------------|---------------------------|
| Linear | `<-` | Datum consumed; body fires | Datum **not** consumed; continuation stays installed; matcher tries another datum or blocks |
| Peek | `<<-` | Datum stays (peek does not consume); body fires once per arrival | Datum stays; body does not fire |
| Repeated | `<=` | Datum consumed; body fires; continuation stays installed for future matches | Datum **not** consumed; continuation stays; guard re-evaluated on next candidate |

The guard's purity (§3.8) makes these semantics deterministic: re-evaluating
the same `(patterns, datum, bindings)` produces the same result every time, so
"try another datum after guard-fail" doesn't loop on the same datum.

For peek + guard, note that the matcher is responsible for knowing the
continuation is a peek and not consuming on guard-fail — but it would not
have consumed on guard-true either, so this is a no-op in practice.

For repeated + guard, take care that the continuation is *not* fired multiple
times for the same datum just because the guard is being re-evaluated. The
existing rspace bookkeeping for `<=` already prevents redundant fires; the
guard simply gates whether a fire happens at all.

### 3.8 No syntactic sublanguage; runtime bool check only

Guards (`Receive.condition`, `MatchCase.guard`, `EMatchExpr.cases[i].guard`)
and `if` conditions accept **any process** syntactically. There is no
normalize-time forbidden-list and no `is_pure_boolean_expr_par` helper.
This was the original symmetric tightening proposal; it turned out to be
redundant.

The reason: the only Rholang constructs that can cause observable side
effects are `Send`, `Receive`, `New`, `Bundle`, and `Contract`. The
evaluators we use never fire any of these on a guard / condition Par:

- `if` and match-case guards in process context use the existing
  `eval_expr` (`reduce.rs:6856-6875`), which walks `par.exprs` only.
  Sends/news/receives sitting in `par.sends` / `par.news` / `par.receives`
  are left in place as inert data and dropped when no case matches.
- Receive guards in rspace context use `rho-pure-eval` (§3.9), which is
  pure by construction. Same Expr-only walk; nothing in the guard ever
  executes as a process.

So a program like `for (x <- c where { y!(5); true }) { … }` is well-formed
at normalize time, but the `y!(5)` Send is never sent — the guard's Par
contains both a Send and the Expr `true`; pure-eval extracts the Expr and
ignores the rest; matcher reads `true` and the guard passes. The Send is
abandoned. Likewise `if (y!(5)) { P } else { Q }` today silently no-ops
(see Phase 2 for the change to a synchronous error).

Runtime behaviour for non-bool result:

- **Receive guards** (rspace context, via `rho-pure-eval`): non-bool →
  matcher treats as guard-false (no consumption). Preserves rspace's
  "matching is total" contract; an ill-typed guard is semantically
  equivalent to a non-matching pattern.
- **Match-case guards** (interpreter context, via `eval_expr`): non-bool →
  fall-through to the next case. Same shape as receive-guard behaviour:
  fail closed, try the next thing.
- **`if` conditions** (interpreter context, via the new `eval_if`):
  non-bool → synchronous `InterpreterError::IfConditionTypeError`. See
  §3.10. The asymmetry vs match-case is deliberate: `if` has only two
  intended outcomes (true / false / Q), so a non-bool is a programmer
  error worth reporting; match has explicit fall-through, so non-bool can
  be threaded through it.

The asymmetry between receive guards (non-bool = match-fail) and `if`
(non-bool = error) is also deliberate. Aborting from inside the rspace
matcher would tear down a deploy based on a state of the tuple space the
user didn't directly cause; treating it as no-match keeps the deploy alive
and keeps replay deterministic.

### 3.9 New crate `rho-pure-eval`

Lives at `/Users/stay/greg/f1r3fly/f1r3node-rust/rho-pure-eval/`. Pure
function from `(Par, Env) -> Result<Par, EvalError>` that mirrors the
existing `Reduce::eval_expr` (`reduce.rs:6856`) — walks `par.exprs`,
evaluates each `Expr` (arithmetic, comparisons, boolean ops, methods,
`matches`, `EMatchExpr` recursively), and assembles the resulting Par
with the rest of the input Par's fields (sends/news/receives/etc.) carried
through as inert data. Critical properties:

- **No I/O, no rspace, no continuation creation.** A literal pure function.
- **Deterministic.** Same input → same output, byte-identical, on every
  node. This is what makes Option 3 safe under casper replay.
- **No async.** Synchronous, no `tokio` dependency, so it can be called from
  inside the rspace matcher's tight loops without runtime contamination.
- Returns the resulting `Par`. Callers extract the bool (`Par with one
  GBool Expr`) or treat anything else as guard-false.

Dependencies:
- Depends only on `models` (for `Par`, `Expr`, etc.) and `shared`.
- Does **not** depend on `rholang` or `rspace++`.

Consumers:
- `rholang` calls it for match-case guards and `EMatchExpr` (`if`
  conditions also use it via the new `eval_if`).
- `rspace++` calls it for receive-guard evaluation inside the matcher.

Refactor: extract the existing `eval_expr` (and the per-expression
evaluators it calls — `EAddBody`, `EAndBody`, `EEqBody`, method dispatch,
etc.) from `reduce.rs` into the new crate. `reduce.rs` keeps `eval_send`,
`eval_receive`, `eval_new`, `eval_match`, `eval_if`, etc. (anything that
fires processes), and delegates pure-Expr evaluation to `rho-pure-eval`.

### 3.10 First-class `if` IR construct with synchronous type error

Today `p_if_normalizer.rs` desugars `if (cond) { P } else { Q }` to a
`Match { true => P; false => Q }` IR node. Non-bool `cond` silently no-ops.
We replace this with a first-class `If` IR node and a synchronous runtime
error for non-bool conditions.

**Surface syntax** is unchanged. The grammar (`rholang-tree-sitter/grammar.js:75`)
already treats `if` as its own production. The AST already has its own
`Proc::IfElse` (or equivalent) node. So **no parser changes are needed**.

**IR (§3.2):** new `If { condition, if_true, if_false, locally_free,
connective_used }` proto message, and a new `repeated If conditionals` field
on `Par`. (`conditionals` chosen as the field name because `if` and `ifs`
are awkward / reserved in generated Rust code.)

**Normalizer:** `p_if_normalizer.rs::normalize_p_if` (lines 67-91 today)
stops constructing a `Match` and instead constructs an `If` directly:

```rust
let desugared_if = If {
    condition: Some(target_result.par.clone()),
    if_true:   Some(true_case_body.par.clone()),
    if_false:  Some(false_case_body.par.clone()),  // Par::default() if no else
    locally_free: union(...),
    connective_used: ...,
};
let updated_par = input.par.prepend_if(desugared_if);
```

No syntactic check on the condition (see §3.8). Any Par is accepted; the
runtime check enforces bool-ness at the eval site below.

**Reducer:** new arm in `reduce.rs` (or wherever `eval_match` lives):

```rust
async fn eval_if(&self, conditional: &If, env: &Env<Par>, rand: ...) -> Result<(), InterpreterError> {
    let cond_par = rho_pure_eval::eval(&conditional.condition, env)?;
    match extract_bool(&cond_par) {
        Some(true)  => self.eval_par(&conditional.if_true,  env, rand).await,
        Some(false) => self.eval_par(&conditional.if_false, env, rand).await,
        None        => Err(InterpreterError::IfConditionTypeError {
            actual: cond_par,
        }),
    }
}
```

`rho_pure_eval::eval` evaluates the condition's `Expr` slots without firing
any sends/news/receives that may also be present in the Par (mirroring
today's `eval_expr` semantics). `extract_bool` returns `Some(b)` iff the
resulting Par has exactly one entry — a `GBool(b)` Expr — and nothing else.
Anything else (Par with sends or news still inert, Par with a non-bool
Expr, Par with a still-unevaluated EVar) returns `None`, yielding the type
error.

**New error variant:** `InterpreterError::IfConditionTypeError { actual:
Par }`. Synchronous — raised at the point of evaluation. Propagates through
the existing error path at `interpreter.rs:228+` (the same path
`MethodNotDefined` uses). Consumed cost is reported correctly.

**No `EAbort`, no `UserAbortError` involvement, no registry lookup, no
`Match` desugaring.** The error is raised at the eval site of the `If`,
not concurrently via a send to a system process — strictly synchronous.

**Backwards-compat:**
- Existing `if (5) { … }` style programs now error at runtime instead of
  silently no-op'ing. Validate against the corpus.
- Existing `if (true) { P } else { Q }` programs evaluate identically *at
  the language level* but produce a different IR (`If` instead of `Match`).
  Block hashes for any block whose deploys contain `if` will change. This
  is **consensus-affecting**; see §7.3.

## 4. Semantics summary

### Receive

`for (R_1; …; R_k) { P }` desugars to `for (R_1) { for (R_2) { … for (R_k) { P } … } }`.
Each `R_i` is one `&`-joined receipt-with-guard. A single `for(R)` reduces iff
all spatial patterns in `R` match atomically AND `R`'s guard (if any) evaluates
to `true` under the resulting bindings. Guard-false is treated identically to
spatial mismatch: no consumption, no abort. The continuation stays installed.

### Match (process)

```
match e { p_1 where g_1 => P_1; ...; p_n where g_n => P_n }
```

Try cases in order; fire the first `i` such that `p_i` matches `e` and `g_i`
holds. If none, match is silently dropped (existing semantics for non-matching
match).

### Match-as-bool-expr

Same as match-process, but the chosen case's RHS produces a value used in
expression context. Non-exhaustive match-as-expr is a runtime error.

## 5. Examples

```rho
// Receive guards
for (
  @{"buy", price} <- @market & @{"limit", limit} <- @orderBook
    where price <= limit
) { @"trades"!(price) }

// Match guard with fallthrough
match age {
  n where n < 13 => @"category"!("child")
  n where n < 20 => @"category"!("teen")
  _              => @"category"!("adult")
}

// Match-as-bool-expr
for (@x <- @input where match x {
       i where i % 2 == 0 => i > 0
       _                  => false
     }) { @"out"!(x) }
```

## 6. Test plan

Normalizer tests (`p_input_normalizer.rs`, `p_match_normalizer.rs`,
`p_if_normalizer.rs`):

1. Receive without guard → `Receive.condition` is default (regression).
2. Single-receipt guard referencing a bind var → IR has correct condition Par.
3. Multi-receipt receive (`R_1; R_2`) → outer/inner `Receive` IR nodes
   correctly nested; each carries its own guard.
4. Multi-receipt guard `cond_2` references vars from `R_1` and `R_2` → ok
   (lexical scope via nesting); `cond_1` referencing `R_2`'s vars → unbound
   variable error (no special check needed).
5. Guard contains a Send/New/Receive — accepted at normalize, treated as
   inert at runtime; guard reads as guard-false (matcher fails atomically).
6. Match without guard → `MatchCase.guard` default (regression).
7. Match with guard, all-bool RHS → emits `EMatchExpr`.
8. Match with guard, mixed RHS → emits `Match` with guards.
9. `If` IR shape: condition / if_true / if_false; else-less form has
   `if_false = Par::default()`.
10. (slot retained for stable numbering)

Integration tests (`reduce_spec.rs` and a new `where_spec.rs`):

11. Receive guard true → body fires; messages consumed.
12. Receive guard false → no body fires; messages remain in tuple space.
    Can be verified by sending another, satisfying message and observing
    eventual fire.
13. Match guard fall-through hits second case.
14. Match-as-expr inside a receive guard.
15. Concurrency: another continuation is allowed to consume messages while a
    guard-failed continuation waits.
16. `;`-nested receive: outer `for` consumes from outer channel, then inner
    `for` waits on inner channel — verify that messages on inner channel
    arriving *before* the outer fires are not consumed by the outer.
17. `if (5) { P }` → `AbortError` raised at runtime, deploy fails with
    "if condition was not a boolean" or equivalent message.
18. Backward-compat sweep: run all `.rho` files under `examples/` and
    `rholang/tests/`. None should break under the new `;`-nesting +
    `if`-abort semantics; if any do, they were relying on the old (atomic
    `;` / silent-noop `if`) semantics, and need fixing or the plan needs
    revisiting.

Parser tests (in `rholang-rs`): round-trip new syntax; conflicting precedence;
guard-only-no-where (must fail); `where` as ordinary identifier outside of
receive/match (must succeed — keyword is contextual).

## 7. Risks and open questions

1. **rspace matcher API change.** Extending the matcher to accept and evaluate
   a guard predicate is the largest piece of new code. The matcher today does
   not depend on any expression evaluator. The `rho-pure-eval` crate (§3.9)
   makes this clean, but we should validate that the matcher's iteration loop
   correctly tries multiple candidate datums on guard-fail (rather than
   short-circuiting after first spatial match).

2. **`;` semantics — already aligned in Phase 1.** Investigation during
   Phase 1 (committed at `1b94fe9`) found the Rust normalizer already
   desugars `;` to nested `for`s at `p_input_normalizer.rs:48-59`; the
   plan's earlier description of "atomic flatten" was based on a stale
   exploration. Phase 1 aligned docs (`rholangtut.md`,
   `08-channels-and-concurrency.md`) and rewrote `tut-philosophers.rho`
   to use `&` (the only example that would deadlock under nested
   semantics). Other corpus programs are pattern-equivalent under both
   semantics. No further mitigation needed.

3. **`if` becomes a first-class IR node — consensus-affecting.** Today
   `if` desugars to `Match`; under §3.10 it produces a new `If` Par
   variant. Block hashes for any block whose deploys contain `if` will
   differ before vs after. Implications:
     - This is **not** a soft-fork-compatible change. Coordinated upgrade
       required: every validator must run the new code at the same block
       height.
     - `casper`'s replay (`replay_runtime.rs`, `replay_rspace.rs`) must
       agree on the new IR shape; old replay against new blocks (or vice
       versa) will diverge.
     - The IR migration story for stored state: tuple space contents that
       contain `Match`-encoded `if`s from before the change won't be
       rewritten. Stored continuations remain valid (still `Match` shape
       in the IR) — only newly-deployed code uses the new `If` shape.
     - Corpus sweep: existing `if (5) { … }` style programs now error at
       runtime instead of silently no-op'ing. Fix in corpus or accept.
     - Land as an isolated commit so the IR change is bisectable.

4. **Parser pin coordination.** `rholang-rs` rev pin must be bumped after the
   external repo lands. Develop on a feature branch in `rholang-rs`, pin to
   the branch SHA in this workspace during dev, then squash-merge in
   `rholang-rs` and pin to the final SHA.

5. **Hashing canonical form.** The new proto fields (`Receive.condition`,
   `MatchCase.guard`, `EMatchExpr`) must be deterministically hashed — verify
   the hashing code in `models` or `casper` traverses the new fields:
     - Search for structural-equals/hash trait impls on `Receive`,
       `MatchCase`, `Expr` and confirm prost-derived vs hand-written.
     - Add new fields to any hand-written impls.

6. **Backwards compatibility.** Existing `.rho` programs do not use `where`,
   and the new proto fields are optional; default-empty values reproduce
   today's IR exactly. Existing block hashes therefore stand. Confirm by
   round-tripping the existing test corpus and diffing block hashes.

7. **`where` as a keyword.** It collides with no current Rholang keyword, but
   shadow-checking against any existing `.rho` corpus is a prereq. Make it a
   contextual keyword (only inside `for(...)` or `match { ... }`).

8. **Keyword choice.** The historical Rholang Mercury BNFC grammar
   (`rholang/src/main/bnfc/rholang_mercury.cf:120-122`) had a commented-out
   `LinearCond ::= [LinearBind] "if" Proc` — i.e., the original intent was to
   use `if` for receive guards, not `where`. We are using `where` per the
   user's spec; flagging the historical precedent in case it's worth
   reconsidering. ML/Haskell tradition is `where`; SQL/some imperative
   traditions use `where` too. `if` would be more concise but slightly more
   ambiguous since `if` already has a process-level meaning.

9. **Sort/permutation in receive guard variable indices.** Within a single
   receipt, `pre_sort_binds` may reorder `&`-joined binds for canonical
   hashing. The guard is normalized after sort and references variables by
   post-sort De Bruijn level, so this is automatic — but watch for off-by-one
   when wildcard-only binds contribute zero levels. Add a focused test that
   exercises a sort-reordering `&`-join with a guard mentioning every bound
   variable.

10. **Rho-pure-eval extraction scope.** Pulling the existing pure-expression
    evaluator out of `reduce.rs` is a refactor; we need to be careful about
    methods that today call into reduce-internal helpers. Methods are
    functional but their *implementations* may close over reduce-side state
    (e.g., random source for arithmetic? — almost certainly not, but verify).

11. **`If` field name and proto numbering.** `if` is a Rust keyword and
    `ifs` is awkward; this plan uses `conditionals`. Confirm that doesn't
    clash with anything else, and pick a stable proto field number (the
    largest unused number on `Par` is the convention).

## 8. Implementation phasing

Each phase ends in green tests and is mergeable independently.

- **Phase 0** (this doc): plan reviewed and accepted.
- **Phase 1 — `;` semantics docs alignment.** ✅ **Done** (commit `1b94fe9`).
  Discovered the normalizer already desugars `;` to nested `for`s at
  `p_input_normalizer.rs:48-59`. Aligned docs (`rholangtut.md`,
  `08-channels-and-concurrency.md`) and rewrote
  `tut-philosophers.rho` to use `&`.
- **Phase 2 — `if` as a first-class IR construct (language + consensus
  change).**
    1. Add `If` proto message and `Par.conditionals` field; regenerate
       prost types.
    2. Update `p_if_normalizer.rs` to emit `If` instead of `Match`.
       (No syntactic check on the condition — see §3.8.)
    3. Add `eval_if` reducer arm and
       `InterpreterError::IfConditionTypeError`.
    4. Update existing `p_if_normalizer.rs` tests (which currently assert
       the `Match` desugaring shape).
    5. Verify casper replay paths (`replay_runtime.rs`, `replay_rspace.rs`)
       handle `If` nodes.
    6. Corpus sweep — confirm no `.rho` programs rely on silent-no-op
       semantics for non-bool `if` conditions. Isolated commit.
- **Phase 3 — `rholang-rs` external**:
    1. Grammar + AST + parser tests for receive `where`, match `where`.
    2. Tag a feature-branch SHA.
- **Phase 4 — `rho-pure-eval` crate**:
    1. New crate skeleton with `eval(par, env) -> Par` API.
    2. Extract `eval_expr` and per-Expr evaluators from `reduce.rs`.
    3. Replace `reduce.rs` call sites with delegation (including
       `eval_if`'s condition evaluation).
    4. Round-trip tests against the existing rholang test corpus.
- **Phase 5 — proto + normalizer for guards**:
    1. Bump `rholang-parser` pin to Phase 3 SHA.
    2. Add `Receive.condition`, `MatchCase.guard`, `EMatchExpr` to the
       proto.
    3. Normalizer changes for match guards and receive guards (no
       syntactic checks; runtime handles bool-ness).
    4. Tests 1–9.
- **Phase 6 — match-as-expr runtime**:
    1. `eval_match` fall-through (non-bool guard → next case).
    2. `EMatchExpr` evaluation (delegates to `rho-pure-eval`).
    3. Tests 7, 13, fall-through runtime tests.
- **Phase 7 — receive guard in rspace matcher**:
    1. Extend rspace matcher API to take optional guard predicate.
    2. Wire `rho-pure-eval` into the matcher.
    3. Bind-type behaviour tests for `<-`, `<<-`, `<=` (§3.7).
    4. Tests 10–14.
- **Phase 8 — docs, tutorial updates, examples in `examples/`**.

## 9. Out of scope

- Refinement types or static exhaustiveness for match-as-expr.
- Allowing `where` on a single bind (rather than per-receipt). The user spec
  says per-receipt; we honour that. (Adding bind-level `where` later would
  reduce to per-receipt by AND'ing into the receipt-level guard, but there's
  no demand yet.)
- Optimisations for repeated-guard receives (e.g., compiling guards into rspace
  pattern predicates). The first cut evaluates guards in the rholang
  interpreter only.
