---
title: Rholang name garbage collection — Isabelle mechanization plan
status: draft
author: claude-session
date: 2026-05-01
related-docs:
  - docs/discoveries/rholang-gc-design.md
---

# Rholang name garbage collection — Isabelle mechanization plan

This plan tracks the formal mechanization of Rholang name garbage
collection. The semantic model and theorem statements are fixed in
`docs/discoveries/rholang-gc-design.md`; this document only schedules the
work and lists what each phase delivers.

## Phase 0 — Skeleton (this branch, `feat/rholang-gc-formal`)

**Status: in_progress.**

Deliverables:

- `formal/isabelle/RholangGC/ROOT` — Isabelle session declaration.
- `formal/isabelle/RholangGC/Atoms.thy` — countably-infinite atom type;
  `pub` set.
- `formal/isabelle/RholangGC/Names.thy` — `name` datatype with
  `GPrivate / GDeployId / GDeployerId / GSysAuthToken / GUri / Quote /
  Bundle`; `atoms_of`, `forgeable_by`.
- `formal/isabelle/RholangGC/Syntax.thy` — full `par` AST (sends,
  receives — linear/persistent/peek, news, matches with guards, ifs,
  bundles, exprs as opaque values, connectives, `EMatchExpr`).
- `formal/isabelle/RholangGC/Patterns.thy` — pattern AST and the
  `matches` oracle.
- `formal/isabelle/RholangGC/RSpace.thy` — the configuration `(σ, P)`
  with multiset of datums and waiting continuations.
- `formal/isabelle/RholangGC/Reduction.thy` — labelled small-step
  reduction; the `COMM(c)` observable.
- `formal/isabelle/RholangGC/FreeNames.thy` — `FN`, `BN`, `atoms_in_P`,
  escape relation.
- `formal/isabelle/RholangGC/Garbage.thy` — `is_garbage`, `gc0`, `gc1`.
- `formal/isabelle/RholangGC/SoundnessGC0.thy` — theorem statement,
  proof = `sorry`.
- `formal/isabelle/RholangGC/NonTriviality.thy` — theorem statement,
  proof = `sorry`.
- `formal/isabelle/RholangGC/SoundnessGC1.thy` — theorem statement,
  proof = `sorry`.
- `formal/isabelle/RholangGC/Adequacy.thy` — file:line table of
  correspondences; no theorems.

Acceptance:

- All `.thy` files parse against Isabelle/HOL + Nominal2 (assuming a
  local install; CI integration is not part of P0).
- Each `sorry` corresponds to a theorem stated in the design doc.
- The skeleton compiles in the sense that `theory ... imports ... begin
  ... end` is well-formed; we are not asserting that proofs check.

No Rust code changes in this phase.

## Phase 1 — Discharge the proofs

Discharge each `sorry` in dependency order:

1. `NonTriviality.thy` — finite `atoms_of P + bn_new P + pub` ⇒ infinite
   complement. Should be straightforward set-theory. Done first because
   it does not depend on the reduction relation.
2. `SoundnessGC0.thy` — by induction on the reduction sequence. The key
   invariant: any atom appearing in a `COMM(c)` step's channel `c` was
   either in `atoms_of P_0`, in `pub`, or freshly allocated by `new` in
   `P_0`. This requires a "fresh-atom-introduction is the only source of
   new private atoms" lemma.
3. `SoundnessGC1.thy` — strengthen the invariant with escape and
   one-sided reasoning. The bundle-aware refinement requires lemmas
   about how `Bundle(cap, n)` filters which COMM rules can fire.

Each proof should cite the corresponding Rust source in a comment as
extra documentation.

Acceptance:

- All `sorry`s removed.
- `isabelle build -d <session-dir> RholangGC` succeeds locally.
- A short note in `docs/CompletedTasks.md` records the proofs as
  reviewable.

## Phase 2 — Differential testing

Drive a small corpus of Rholang programs (the examples from the design
doc, plus randomly-generated terms) through:

- the Isabelle reduction relation (executable extracted via
  `code_pred` / `value`); and
- the Rust interpreter (`rholang::interpreter`).

Compare the sequences of COMM events on each channel. Any divergence is
a bug in the Isabelle semantics or an under-specified rule in the design
doc; fix on the Isabelle side.

Acceptance:

- A test harness under `formal/diff/` driving both engines.
- A short report in `docs/discoveries/` listing the corpus and any
  divergences resolved.

## Phase 3 — Runtime integration

Implement GC₁ as a static analysis in the Rust workspace, likely in a
new `rholang-gc` crate consumed by `rspace++` to elide datums and
waiting continuations whose channels are reported garbage. The
correctness of the analysis is delegated to the Phase-1 proof; the Rust
implementation is exercised by Phase-2 differential testing.

Acceptance:

- New crate `rholang-gc` with `gc1(par: &Par) -> NameSet` and unit
  tests.
- Optional integration in `rspace++` behind a feature flag.
- Doc note explaining how to enable it.

This phase is deferred until Phases 1 and 2 are complete.

## Risks

- **Nominal2 install friction.** Mitigation: pin the AFP version in
  `ROOT` and document the Isabelle setup in a follow-up note when we
  start Phase 1.
- **Pattern-matching abstraction is too coarse.** Mitigation: refine the
  oracle in a later sub-phase if Phase 2 reveals false negatives that
  matter for runtime use.
- **Bundle algebra subtleties.** Bundle composition rules are stated in
  the Rholang spec; cross-check with `models` on first use.
