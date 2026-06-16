# Funding-Slots Producer — Design (per-lane attribution under P8-balanced settlement)

**Status:** RESOLVED — the producer is **not needed for settlement** (Greg,
2026-06-16). This doc is retained for the reasoning + the diagnostic-only option.

> ## ★ RESOLUTION (Greg, 2026-06-16) — supersedes §6's deferred conflict
>
> **"The tensor operation of multi-sig was always intended to distribute the cost
> as equally as possible over the co-signers."** This is authoritative and settles
> the Rule-1-vs-P8 conflict §6 flagged: the compound `s₁∘s₂` tensor **always**
> distributes cost BALANCED across cosigners (= P8 = the existing
> `DefaultApportionment`), as the *original intent* — not a compromise.
>
> **Consequence:** the per-located-stack / per-signer SETTLEMENT (the Rule-1
> reading) is **not** the intended semantics, so the region-attribution PRODUCER
> below (A / A-prime Stage-II / B1 / B2) is **unnecessary for settlement**. The
> existing balanced apportionment (`compute_settlement_debits` +
> `DefaultApportionment`, verified by `default_apportionment_conserves_and_never_overdraws`)
> IS the intended multi-sig cost model. A per-signer attribution would *contradict*
> balanced settlement (it implies `sᵢ` pays for its region; in truth all cosigners
> pay equally). **No producer is built.** The W1 s₀ collapse (single-sig → the lone
> envelope lane) + balanced multi-sig apportionment already realize Greg's intent.
>
> The mechanisms below stand only as the (now optional, low-value) DIAGNOSTIC
> per-region *structural* visibility — they show which signature *decorates* a
> region, NOT who pays (balanced pays). B2's consensus-Par change is never needed:
> with balanced settlement the per-lane split is never a consensus quantity.

This documented the producer that would make the W1 Phase-3 per-lane attribution
infrastructure *active* — now moot for settlement per the resolution above; the
Phase-3 channel-match infra remains dormant diagnostic forward-infra.

## 0. Decisions already taken (frame the whole design)

1. **Settlement stays P8-balanced.** Multi-sig cost is split equally across
   cosigners (`∘` commutative — the ratified P8), realized by the existing
   `compute_settlement_debits` + `DefaultApportionment`. Funding slots does **NOT**
   change which `Σ⟦sᵢ⟧` pool settles — the located-stack per-signature draw of
   Rule 1 is **deferred** until Greg reconciles Rule 1 vs P8 in the papers (they
   currently conflict: Rule 1 §838 charges a signed redex to *its* signature's
   stack; P8 charges multi-sig balanced).
2. **Consequence — the per-lane split is DIAGNOSTIC, not consensus.** Since
   settlement is balanced, the per-lane attribution feeds only *visibility*
   (`per_lane_demand`-style reports: "which signature funds which COMMs"), never
   the supply-pool debits. Consensus cost (`consumed_units`, the `Σ` balances, the
   post-state) is **unchanged** by any option below.

## 1. The corrected cost model (why this is not the gate translation)

The normative cost is **Rule 1** (§838): a signed COMM
`{recv(y,x,T) | send(x,U)}_s | s:S → T[*U/y] | S` consumes **one token, charged
to `s`** — there is no separate fuel-gate COMM. The fuel-gated translation
(`{% P %}[s] ⟿ for(t<-Σ⟦s⟧){*t|P}`, §2419) is a *demonstrative pure-rho encoding*
that adds gate+deposit COMMs and therefore **over-counts** relative to Rule 1. W1
correctly avoids that translation. So the faithful producer is **region
attribution** — charge P's *own* COMMs to lane `s` — which is **total-cost-neutral**
(same `consumed_units`; only the per-lane split changes). It is NOT "emit gates,"
and it does NOT double-meter.

The obstacle (real, not double-metering): recognition is *gate-free*, so the
normalized `Par` of `{% P %}[s]` is byte-identical to unsigned `P` — the per-term
`s` is erased. Anything that wants per-lane attribution must recover the
`{% P %}[s]` boundary somewhere. The three mechanisms differ in *where*.

## 2. Mechanism A — static AST-region demand (RECOMMENDED)

**Idea.** Compute the per-lane split as a STATIC analysis over the *parsed AST*
(where the `{% P %}[s]` regions are still intact), before normalization erases
them. Walk the `AnnProc` carrying a region stack; on entering a `Proc::SignedTerm
{ proc, sig }` push `lane = signature_to_native_sig(sig).lane_hash()`, on each
`Send`/`Receive` (the COMM-driving nodes — same discipline as `delta_sigma::
demand_par`) attribute one token to the current top-of-stack lane (the deploy
envelope when the stack is empty), and on a `Bind::Signed` clause push that
clause's lane for its rendezvous COMM.

This is the AST-side dual of `delta_sigma::demand_by_sig` (which runs on the *Par*
via channel-match): `demand_by_region` runs on the *AST* via region tracking, so
it attributes `{% P %}[s]`'s data-channel COMMs to `s` — exactly what channel-match
cannot do post-normalization.

- **Surface:** a new `demand_by_region(ast: &AnnProc, env_key, parser) ->
  BTreeMap<SigKey, DemandEntry>` in (or beside) the compiler; exposed on the
  compiler output as a diagnostic. Reuses `signature_to_native_sig` + the
  `demand_par` node discipline.
- **Blast radius:** additive. One new analysis fn + one diagnostic field on the
  compiler/evaluate result. NO change to `normalize_*`, the reducer, the metering,
  or the `Par`.
- **Consensus / replay impact:** ZERO. The `Par`, `consumed_units`, supply pools,
  `cost_trace_digest`, and replay are untouched (the analysis is a read-only side
  report). Byte-identity preserved.
- **Fidelity:** it is the STATIC per-lane demand (what each signature *would* fund
  per the program text). It does not reflect runtime parking/contention — but
  under recognition-only there is no in-program parking anyway (MAJOR-4), and the
  static demand is exactly the `Δ_s` the paper's linear proof uses (§589 "the
  static analysis checks token availability per located surface"). So for a
  diagnostic it is the *right* quantity, and it agrees COMM-for-COMM with the total
  `demand` when summed over lanes.
- **Test plan:** (i) `demand_by_region` over the demo + the GATE-1 fixtures: sum
  over lanes == `demand` total; each `{% P %}[s]` region's COMMs land on lane `s`;
  (ii) a multi-signature program: per-lane counts match the hand-computed split;
  (iii) the s₀ single-sig case collapses to `{envelope: demand}` (every region's
  `s` resolves to the lone envelope signer — BLOCKER-1).

**Why recommended:** it delivers the per-lane visibility (the whole point under
balanced settlement) with zero consensus surface, zero reducer/Par change, and it
reuses the Phase-2/3 resolution machinery. It is the spec-minimal complete answer
to "funding slots" *given the P8-balanced decision*.

## 3. Mechanism B1 — non-Par runtime side-table

**Idea.** Thread a `region → lane` map from `recognize_signed_term/join` to the
reducer; the reducer attributes each *runtime* COMM to its region's lane (a
runtime-accurate per-lane count), feeding `note_channel_lane`/`per_lane_demand`.

- **The hard part:** the key. Recognition runs in the normalizer; the reducer
  assigns `source_path`/`redex_id` at *runtime* (per fork, after substitution and
  spawning), so the normalizer cannot pre-key the table by a runtime identity, and
  the AST→runtime COMM mapping is not 1:1 (a `{% P %}[s]` under a persistent
  receive fires many times). A stable key would itself have to be carried in the
  `Par` — collapsing B1 into B2.
- **Blast radius:** the reducer's metering-context flow (a region-context stack
  pushed/popped as evaluation enters/leaves a recognized region) + a normalizer→
  install side channel. Touches `reduce.rs` eval paths and `metering.rs`.
- **Consensus / replay impact:** the per-lane counts are diagnostic (settlement
  is balanced), so consensus is unchanged — BUT the reducer-context plumbing is on
  the hot path and must preserve the `cost_trace_digest` (no charge reordering),
  re-running the Phase-3 byte-identity gate.
- **Fidelity:** runtime-accurate (reflects which regions actually fired). Higher
  than A, but unnecessary for a diagnostic under balanced settlement.
- **Verdict:** more plumbing + hot-path risk for fidelity that balanced settlement
  does not consume. Only worth it if a runtime-accurate per-lane report is later
  required *and* B2's Par change is unacceptable.

## 4. Mechanism B2 — Par-level region marker

**Idea.** Recognition wraps P in a lane-tagged `Par` node (a new proto construct,
or a reuse of an annotation slot) the reducer reads to set the region lane.

- **Blast radius:** a `RhoTypes.proto` change + every consumer of the `Par` shape
  (normalizer, reducer, RSpace, serialization, the Rocq/TLA+ models).
- **Consensus / replay impact:** **consensus-visible** — the `Par` is hashed, so
  every cost-accounted program's `Par`/post-state/`deploy_id` byte-identity shifts.
  A deliberate byte-identity break requiring full replay + cross-prover
  re-validation, even though *settlement* is unchanged.
- **Fidelity:** cleanest semantics (the region is first-class), runtime-accurate.
- **Verdict:** **overkill for a diagnostic.** A consensus-visible Par change to
  carry information that, under balanced settlement, never reaches consensus. Only
  justified if/when located-stack per-signature settlement (Rule 1) is adopted —
  i.e., after Greg reconciles Rule 1 vs P8 — at which point the per-lane split DOES
  become consensus and a first-class region is warranted.

## 5. Recommendation

Adopt **Mechanism A (static AST-region demand)**. It is the complete, faithful,
zero-consensus realization of funding-slots visibility under the P8-balanced
decision, reusing existing machinery and preserving byte-identity. Defer B1/B2
until (and unless) the Rule-1-vs-P8 reconciliation makes the per-lane split a
consensus quantity — at which point B2 (first-class region) becomes the right
basis, scoped as its own consensus-versioned change with replay + cross-prover
re-validation.

| Mechanism | Per-lane fidelity | Reducer change | `Par`/consensus change | Replay re-validation | Recommended |
|---|---|---|---|---|---|
| **A — static AST-region** | static `Δ_s` (diagnostic-correct) | none | none | none | ✅ now |
| B1 — runtime side-table | runtime-accurate | hot-path context stack | none (diagnostic) | Phase-3 byte-identity gate | only if runtime accuracy needed |
| B2 — Par region marker | runtime-accurate, first-class | yes | **yes (byte-identity break)** | full | only with Rule-1 settlement |

## 6. Open dependency (not this design's to resolve)

The located-stack per-signature **settlement** (Rule 1) vs **P8-balanced** conflict
is a genuine paper contradiction (a Greg P-question). This design assumes
P8-balanced (the ratified decision) and delivers per-lane *visibility* only. If
Greg later rules Rule-1 per-signature settlement governs multi-sig, the per-lane
split becomes consensus and B2 + a settlement change (replacing balanced draws
with per-located-stack draws) becomes the follow-on workstream — re-validated for
replay determinism, conservation, and the cross-prover arsenal.
