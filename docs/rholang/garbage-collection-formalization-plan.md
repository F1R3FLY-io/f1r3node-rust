# Scope-Trapped GC — Formalization Plan and Status

Tracks the formalization of the H1 safety argument for the scope-trapped
unforgeable garbage collector shipped in PR #984. H1 is the predicate
`is_scope_trapped` in `rholang/src/rust/interpreter/garbage.rs`; the
prose proof lives in `docs/rholang/garbage-collection.md`.

The prose proof had a hole (missed Rule B — firing a parked
`Receive.body`); the current Rust implementation closed the hole
syntactically by rejecting any mention of `x` inside a `Receive.body`.
Formalization replaces "prose + hope" with machine-checkable evidence.

Two-phase strategy. **TLA+ first** to validate the strict H1 against
operational semantics on bounded instances (catches the entire
"forgot-a-case" bug class with a concrete counterexample trace),
~1–2 engineer-days. **Rocq second** to deliver a universal,
axiom-free proof following the `formal/rocq/rspace_guards/` template
(~2 engineer-weeks). Rust ships the implementation; the formal ladder
raises confidence until the sweep's default can be flipped from
`F1R3FLY_SCOPE_TRAPPED_GC=1` opt-in to always-on.

## Status legend

- [ ] not started
- [~] in progress
- [x] done
- [!] blocked / failed

Each item's "Artifact" column names the file (or files) once it exists.

## Phase 1 — TLA+ (bounded safety)

Target: `formal/tlaplus/scope_trapped_gc/`.

| # | Item | Status | Artifact |
|---|---|---|---|
| 1.1 | Area scaffold (directory + README with model↔code table + configs table) | [x] | `formal/tlaplus/scope_trapped_gc/README.md` |
| 1.2 | Rholang-abstracted syntax (terms, variables, `locally_free`) | [x] | `formal/tlaplus/scope_trapped_gc/ScopeTrappedGC.tla` |
| 1.3 | `IsScopeTrapped(x, P)` matching Rust's `is_scope_trapped` | [x] | (same file) |
| 1.4 | Operational semantics — parking, COMM, body-firing, new-alloc | [x] | (same file) |
| 1.5 | Invariants + main theorem (`Inv_N_NeverInLiveProgram`, `Inv_N_NeverInParkedData`, `Inv_N_NeverInParkedBodyOnForgeable`) | [x] | (same file) |
| 1.6 | Post-fix config `MC_ScopeTrappedGC.{tla,cfg}` (bounded, must stay clean) | [x] | `.../MC_ScopeTrappedGC.{tla,cfg}` |
| 1.7 | Pre-fix control: allow `x` in `Send.data` | [x] | `.../MC_ScopeTrappedGC_payload_leak_pre_fix.{tla,cfg}` |
| 1.8 | Pre-fix control: allow `x` in `Receive.body` | [x] | `.../MC_ScopeTrappedGC_body_mention_pre_fix.{tla,cfg}` |
| 1.9 | ~Pre-fix control for the `receive-body-on-x` subtler hole~ *Dropped: the strict body-mention rejection in 1.8 fully subsumes the holed-prose subtlety; a recursive classifier that already rejects body-on-x-with-nested-receive-on-f leaves no residual defect class to witness. Noted in the design doc.* | [x] | n/a |
| 1.10 | Register post-fix + 2 pre-fix configs in `scripts/ci/check-tla-invariants.sh` | [x] | `scripts/ci/check-tla-invariants.sh` |
| 1.11 | README updated with model↔code table and configurations table | [x] | `.../README.md` |
| 1.12 | Run TLC against all three configs — post-fix clean, each pre-fix violates exactly its named invariant | [x] | `MC_ScopeTrappedGC` 835 distinct states, no error; `_payload_leak_pre_fix` violates `Inv_N_NeverInParkedData`; `_body_mention_pre_fix` violates `Inv_N_NeverInParkedBodyOnForgeable`. |

### Model knobs

- `H1_RejectsPayload` — when `TRUE`, the classifier rejects any `x` in
  `Send.data`. Post-fix: `TRUE`.
- `H1_RejectsReceiveBody` — when `TRUE`, the classifier rejects any
  `x` inside a `Receive.body` (at any depth, on any channel). Post-fix:
  `TRUE`. Covers the reduction rule (firing a parked receive) that the
  original prose proof overlooked.

Each knob gates a specific pre-fix regression config.

## Phase 2 — Rocq (universal proof, axiom-free)

Target: `formal/rocq/scope_trapped_gc/` (template: `formal/rocq/rspace_guards/`).

| # | Item | Status | Artifact |
|---|---|---|---|
| 2.1 | Claim document with traceability table and seam premises | [ ] | `docs/claims/rholang-scope-trapped-gc.md` |
| 2.2 | Rocq module scaffold (`_CoqProject`, `theories/`) | [ ] | `formal/rocq/scope_trapped_gc/` |
| 2.3 | Rholang syntax as inductive types (de Bruijn) | [ ] | `theories/ScopeTrapped.v` |
| 2.4 | Capture-avoiding substitution + `locally_free` lemma | [ ] | `theories/ScopeTrapped.v` |
| 2.5 | `strictly_scope_trapped` predicate | [ ] | `theories/ScopeTrapped.v` |
| 2.6 | Operational semantics (reduction + rspace state + firing) | [ ] | `theories/ScopeTrapped.v` |
| 2.7 | Invariant definition | [ ] | `theories/ScopeTrapped.v` |
| 2.8 | Main theorem (invariant preservation under `→*`) | [ ] | `theories/ScopeTrapped.v` |
| 2.9 | Sweep soundness corollary (deleting N-keyed entries preserves observable behavior) | [ ] | `theories/ScopeTrapped.v` |
| 2.10 | Capstone `MainTheorem.v` with seam-premise `Section Variable`s surfaced | [ ] | `theories/MainTheorem.v` |
| 2.11 | Zero `Admitted`, no custom `Axiom` / `Parameter` | [ ] | (build gate) |
| 2.12 | Add to Rocq CI (whatever the project's current Rocq-build job is) | [ ] | `.github/workflows/*.yml` |

### Seam premises (to be enumerated in 2.1)

- **Fresh-name uniqueness.** `Blake2b512Random::next` yields pairwise
  distinct 32-byte ids modulo cryptographic collision resistance. Rust
  enforces by construction in `eval_new`
  (`rholang/src/rust/interpreter/reduce.rs:1381`).
- **`locally_free` cache correctness.** For every `Par` produced by the
  normalizer, the cached bitset reports every free-variable occurrence.
  Rust enforces via the normalizer's cache invariants.
- **Receive-bind-order determinism.** Shared with
  `rspace_guards` (CLAIM-RSPACE-001 premise C3).

## Open questions tracked here

- Should the TLA+ model abstract `Expr` entirely or reflect `EVarBody`
  specifically? **Resolved**: abstract, with a single "value position"
  bucket that stands in for every non-channel occurrence site. The
  distinction doesn't matter for H1 — any non-channel mention is unsafe.
- Nominal vs. de Bruijn for the Rocq formalization? **Resolved**: de
  Bruijn, matching the Rust interpreter and the `locally_free` bitset
  encoding. Nominal would be closer to the surface Rholang, but the
  substitution-preservation lemmas are much heavier.
- Multi-binder `new x, y in P` in the Rocq proof: prove the single-binder
  case first; extend to multi-binder via a straightforward induction on
  binder count. Tracked as follow-up to 2.8.

## Flip-the-default gate

The sweep's `F1R3FLY_SCOPE_TRAPPED_GC=1` guard comes off when **all** of:
1. Phase 1 post-fix config is in CI and passes.
2. All three Phase 1 pre-fix configs are registered negative controls
   that exhibit the specific invariant they target.
3. Phase 2 main theorem is `Admitted`-free and gated in CI.
4. A separate soak-against-genesis + block-production run (the original
   casper regression trigger) passes with the sweep on.

Item 4 is **not** part of this formalization plan; it's the integration
work that unblocks the default flip. Formalization gives us confidence
that H1 is correct; the soak gives us confidence that no Rust-side side
channel breaks when the sweep runs.
