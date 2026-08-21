# D3 — Replace Phlo with Tokens (approved design)

**Status:** Historical D3 design, implemented and refined by DR-31, DR-32, and
DR-47. The removal of singular-phlo escrow/price and the one-unit calculus COMM
projection remain binding. Protocol 4 additionally charges canonical RSpace
introduction, payload-transfer, and trace bytes from the same fixed RevVault
reservation. Statements below that call unmatched I/O “zero cost” refer only to
the calculus execution projection, not the protocol-4 total. OD-1's unbounded
execution decision is superseded: every production user execution and replay
now runs with finite authority-derived capacity.

## Resolved open decisions (user-approved)
- **OD-1, superseded by DR-31:** removing client-selected `phlo_limit` was correct, but replacing it with an unbounded budget was not. Capacity is now derived from authenticated authority supply after the fee. Exhaustion rejects and cannot certify; accepted execution completes under the same finite cap.
- **OD-2:** D3 + D4.1 land as **ONE atomic commit** (the only build-green unit; the escrow data model and its precharge/refund consumer are circularly coupled).
- **OD-3, refined:** `BillableKind::Comm` denotes a successful atomic RSpace match. Send/receive/new/match/if reducer entries are diagnostic only. The distinction is internal, not on the wire.
- **OD-4:** **delete `phlo_share` outright** (no spec construct maps to it; per-component Split/Join spend is task #12's `effective_supply` closure, not a share field). Do NOT zero-and-reserve.

## D.1 — Remove the singular-phlo escrow
Delete `DeployData.phlo_limit`/`phlo_price`; all escrow arithmetic (`checked_total_phlo_charge[_value]`, `refund_amount_for_token_cost[_value]`, `total_phlo_charge`, `validate_phlo`, casper_message.rs:1116-1197); `Cosigner.phlo_share`, `Cosigned::total_phlo_share`, the `phlo_limit` param of every `Cosigned::from_*`, the share-related `CosignedError` variants; `ProcessedDeploy.primary_phlo_share` + `try_refund_amount`. **Wire (DR-6 fresh-genesis, no back-compat):** remove `DeployDataProto.phloPrice(7)`/`phloLimit(8)`/`primary_phlo_share(15)`, `CompoundSigner.phloShare(4)`, `SigAtom.phloShare(4)`; add `reserved` for the retired tags (precedent: CasperMessage.proto:291). **Nothing replaces them** — the per-signature token supply `Σ⟦s⟧` (balance datum on `from_sig(s)`, DR-13) IS the funding; the gate is the cost authority (Def 19, §7.6). KEEP the cosigner list/sort/dedup/per-signer verification/threshold-algebra (DR-10) — orthogonal to escrow.

## D.2 — Demote `costs.rs` per-op gas to DIAGNOSTIC (DR-9)
The calculus execution component is the count of successful atomic `Comm`
events, each worth one. The RSpace observer records one event after a complete
binary or join match has been selected and before any corresponding state
mutation. `Reduction`/`Primitive`/`Substitution` events remain diagnostic and
contribute zero. An unmatched introduction consumes zero COMM execution units;
a join consumes one regardless of arity. DR-47's protocol-4 refinement makes
`total_cost()` the sum of that execution component and canonical introduction,
payload-transfer, and trace bytes, without depending on which participant
arrived last.

## D.3 — Flip the consensus count to per-COMM (CENTERPIECE; the D1→D3 handoff)
The three protocol layers are related but deliberately not conflated:

- **Runtime:** `RSpace` and `ReplayRSpace` invoke the same pre-mutation observers
  for canonical introductions and the atomic match boundary. A COMM reserves one
  execution unit plus its payload and trace bytes using a stable identity
  derived from the event's consume/produce hashes and multiplicity. An
  introduction reserves its encoded footprint using its own stable semantic
  identity. Scheduler-local source paths and redex indexes are excluded.
- **Structural gate:** `delta_sigma::demand()` counts potential send/receive introductions in the closed, non-persistent fragment. It is a conservative reservation, not an exact runtime trace. Persistent I/O and unresolved dequotation are unprovable structurally. Production does not rely on this pass for exact cost.
- **State-bound proof and settlement:** proposal evaluates the canonical
  candidate sequence from the authenticated merged root under finite
  authority-derived capacity. It binds exact realized physical authority,
  quantitative bytes, total cost, and adjacent roots into evidence. Committed
  execution and replay reproduce that evidence, then settlement burns the exact
  physical and byte draws.

This refinement removes the false `Δ_s == consumed` assumption. For example,
the §7.4 desugared term has eight potential introductions and four successful
matches. The four matches contribute four calculus execution units, while every
actually introduced endpoint also contributes its canonical encoded bytes.

## D.4 — D3+D4.1 atomic (OD-2) + authority-derived runtime capacity (DR-31)
`phlo_limit`/`price`/`share` were removed together with escrow fan-out. The retained runtime budget is protocol-derived rather than client-selected: `state_bound_execution_caps` computes effective authority minus the deterministic fee; the single committed user execution and constrained replay install that exact finite capacity. `ProcessedDeploy.cost` is the canonical weighted `total_cost()` comprising COMM execution units and quantitative bytes. The inner soft checkpoint still rolls back failed user effects, while capacity exhaustion prevents a deployment from producing admission evidence.

## D.5 — remove the legacy minimum-price admission rule
The legacy `Validate::phlo_price`, dispatcher hook, `DeployData::validate_phlo`, and submission checks are removed. The retained source guard records the boundary at `validate.rs:1495-1500`. Keep `min_phlo_price` as an ingress economic configuration field. It is not an input to the proof-bearing authority reservation: a configurable scalar cannot turn an unknown lower bound into a certified finite upper bound.

## D.6 — Migrate the ~372 refs (per-category)
Proto/models (`CasperMessage.proto`, `DeployServiceCommon.proto`, `casper_message.rs` 91): remove fields, reserve tags, regenerate prost, and delete escrow arithmetic. Crypto/signed (`signed.rs` 39): delete `phlo_share`, constructor limits, share sums, and share errors. Node API/gRPC/CLI remove client-selected phlo fields; any structural estimate is explicitly a conservative potential-interaction bound, never exact realized cost. Casper runtime/replay/validation use state-bound evidence. Fuzz and property tests target supply bounds, exact atomic-COMM settlement, replay equality, and no-underflow instead of escrow arithmetic.

## D.7 — Spec-fidelity verdict
D3 realizes DR-9 and the publication's token cost model (§3.6 Rules 1–5 one token per atomic COMM; §4.6/Remark 11 per-signature pools; §7 funding judgment). “Phlogiston” survives as the name of the renewable resource, realized as tokens. The runtime observer, state-bound proof, replay checks, and compound apportionment together complete the consensus-cost path; structural analysis remains a conservative proof for its stated fragment.

## D.8 — Formal / test (LOCAL-ONLY)
Rocq proves the one-unit COMM projection, canonical byte product sum, rejection
atomicity, replay equality, and exact settlement. TLA+ explores
producer/consumer permutations, unmatched introduction bytes, joins,
insufficient reservations, rejection rollback, top-up interleavings, and replay
completion; unsafe controls must refute every omitted or late charge. Rust tests
assert unmatched introductions consume no COMM unit but do consume bytes,
trigger-side symmetry, complete N-way join payload accounting, observer
rejection atomicity, persistent retry idempotence, and exact state-bound
play/replay settlement.

## Sequencing
- **Commit 1 (historical D3 core + D4.1):** OD-3 BillableKind split + reconcile per-COMM (D.2/D.3a); demand per-COMM (D.3b); remove `phlo_limit/price/share`, escrow, price validation, and precharge/refund fan-out.
- **DR-31 refinement:** replace the historical unbounded budget with authenticated authority-derived capacity and state-bound dependent evidence across a single proposal play and constrained replay.
- **Commit 2:** fuzz/kani retarget.
- **Commit 3:** formal (Rocq/TLA+/Sage).
- **Commit 4:** test sweep (re-pin + new consensus-cost tests).

## Critical files
`rspace++/src/rspace/{rspace,replay_rspace,rspace_interface}.rs` (atomic observer boundary), `rspace++/src/rspace/trace/event.rs` (stable COMM identity), `rholang/.../rho_runtime.rs` (authority observer), `accounting/mod.rs` (reconciliation/total cost), `accounting/delta_sigma.rs` (conservative structural proof), `casper/.../rholang/{runtime.rs,replay_runtime.rs}` (state-bound execution/replay), `casper/.../acceptance.rs` (proof and exact apportionment), and `costacc/close_block_deploy.rs` (settlement).
