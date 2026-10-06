# Ownership transfer and funding consent

## Requirement and current status

The user approved arbitrary finite ownership-transfer histories on September 10, 2026.
The lifetime transfer count must not have an arbitrary fixed limit.
Each operation must still satisfy its authorization, resource, funding, and representation constraints.
A configured simultaneous signer limit does not define a lifetime transfer limit.

The [signed phlo contract](signed-phlo-contract-proposal.md) records the selected maximum-price meaning and the remaining economic decisions.
This document records the first abstract proofs for that contract.
It does not report completed phlo-aware ownership transfer in the node.

The [lollipop translation](../../../../rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/desugar.rs) moves continuation authority to the destination signature.
The existing [chain property](../../../../rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/sugar_properties.rs) compares multi-stage sugar with explicit authority layers.
Neither result alone establishes monetary consent, reservation backing, or arbitrarily long native execution.

## Contract requirements

An ownership transfer must identify the rights that move and the rights that remain.
Transferred rights can acquire new owners and new authorized terms.
Retained rights keep their restrictions.
An ownership transfer must not silently reset consumed allowances or erase reserved obligations.
New funding can extend an allowance only through an authorized funding operation.

Every required owner must permit the applicable price.
For compatible ceilings, the effective ceiling is their minimum.
The allocator must not average ceilings or discard a required owner's consent.

`phloLimit` must participate in the signed funding terms and the sufficiency proof.
The approved two-layer design separates deployment limits from persistent funding allowances.
Deployment limits bound individual executions, while persistent allowances bound cumulative authorized draws.
The native contract must still specify continuation activation, located-region attribution, and encumbered partial-transfer routing.
The [allowance proof record](persistent-funding-allowance.md) covers the initial quantitative model.

An estimate alone does not establish a sufficient reservation.
The prover needs a sound conservative bound or valid state-dependent evidence, together with enforced spending limits.
The proof must cover the transfer operation's cost and the continuation's applicable funding obligations.

## Abstract model

[`FundingPriceConsent.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingPriceConsent.v) operates on arbitrary finite lists of natural-number ceilings.
It rejects the empty list instead of interpreting missing consent as an unlimited price.
The list assumes compatible asset and resource units.

[`FundingConsentHistory.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingConsentHistory.v) separates two kinds of state:

| State | Contents | Update rule |
| --- | --- | --- |
| Live funding right | Current owner/ceiling pairs, opaque signed limit terms, asset, schedule, and authorization generation. | An authorized transfer replaces one identified right and advances its generation. |
| Reservation evidence | Captured right, generation, terms, root, and price. | Reservation creation captures the current right. Transfer does not rewrite the captured record. |
| Settlement status | Whether an identified reservation has settled. | Settlement changes the flag once without changing the captured terms. |

`reserve_right` requires a matching authorization generation, a previously unused reservation identity, a funding-validity input, and sufficient price consent.
`transfer_right` requires a present right, authorization input, and matching expected generation.
`settle_reservation` reads captured evidence, not the current owner's terms.

The model treats `phloLimit` as opaque signed data.
It does not choose a lifetime, deployment, or surface interpretation for that data.
Authorization generations use unbounded natural numbers in this mathematical model.
That choice does not approve an unbounded native integer or a particular production nonce scheme.

## Proven obligations

| Theorem | Established result |
| --- | --- |
| `required_price_ceiling_checks_every_consent` | Comparing the price with the minimum is equivalent to checking every required ceiling. |
| `required_price_ceiling_is_permutation_invariant` | Reordering consents does not change the effective ceiling. |
| `adding_required_consent_cannot_raise_ceiling` | Adding a required owner cannot relax the shared price ceiling. |
| `duplicate_consent_does_not_change_ceiling` | Repeating the same consent does not change the effective ceiling. |
| `transfer_preserves_other_rights` | A transfer does not modify a different right key. |
| `transfer_rejects_previous_authorization` | A completed transfer prevents immediate reuse of its previous generation. |
| `arbitrary_authorized_term_sequence` | Every finite replacement-term list has a corresponding authorized command sequence in the abstract model. |
| `no_fixed_transfer_count` | The transition has no fixed numerical limit on the number of successive authorized transfers. |
| `arbitrary_transfer_history_preserves_reservations` | Every successful finite transfer history preserves existing reservation records and settlement flags. |
| `arbitrary_transfer_history_preserves_settlement_evidence` | Later transfers do not change the evidence selected for an existing reservation. |
| `reservation_captures_current_consent` | New reservation evidence captures the current right and satisfies every captured price ceiling. |
| `settled_reservation_cannot_settle_twice` | One sequential model state rejects a second settlement of the same reservation. |

The arbitrary-term theorem preserves the exact replacement list, not just repeated writes of one owner configuration.
It also proves that the final generation equals the initial generation plus the list length.
The existence theorem supplies successful external authorization inputs.
It does not assert that arbitrary real-world transfers are authorized or sufficiently funded.

The proof files include executable counterexamples expressed as Rocq examples.
One example shows that a maximum ceiling can violate a required lower ceiling.
Another shows that returning to the original owners does not restore the old authorization generation.
The third distinguishes captured reservation terms from an unsafe lookup of the current owner's replacement terms.
These examples identify unsafe model alternatives, not newly reproduced native node bugs.

## Evidence and limits

Both modules compiled on September 10, 2026, with a two-gibibyte memory limit and no swap.
The printed theorem assumptions are closed under the global context.
The modules are registered in `_CoqProject` and the proof-assumption gate.
Independent `coqchk` verification passed for both final proof modules under the same memory and swap limits.
The complete repository proof gate and native lifecycle tests were not rerun for this abstract proof slice.

The following obligations remain outside these proofs:

- Authentication of signatures, capabilities, and supplied causal roots.
- Proof validity, monetary solvency, reservation backing, and exact resource bounds.
- Retained restrictions within a partially transferred joint right.
- Interpretation and consumption of `phloLimit`, including authorized replenishment.
- Ownership changes interleaved with reservation creation and settlement across independent validators.
- Production replay, conflict resolution, garbage collection, and machine-integer behavior.

The model preserves the supplied root but does not prove that it identifies the correct causal state.
It stores reservation records without defining production retention or reclamation.
Its settlement flag is not a proof that competing validator branches cannot settle the same obligation.
Its transfer-history theorem covers transfer-only histories around captured reservations, not every mixed lifecycle history.

## Required model and regression extensions

| Requirement | Next verification boundary |
| --- | --- |
| Arbitrary transfer histories | Inductive proofs over mixed transfer, reserve, execute, settle, abort, and authorized funding operations. |
| Concurrent owners | TLA+ instances with independent validators, competing transfers, shared custody, and disjoint rights. |
| No stale consent | Negative controls for current-owner substitution, reservation deletion, old-generation reuse, and retained-right modification. |
| No allowance reset | Explicit quantity scope and residual allowance, with owner-return and repeated-transfer properties. |
| Native correspondence | Generated ownership histories through the actual signed envelope, funding planner, vault settlement, and cold replay. |
| Safe concurrency | Loom tests for actual shared Rust synchronization paths and native tests for RSpace publication and branch conflicts. |
| Long-lived execution | Bounded per-operation work and storage, with no production limit copied from finite model-checking bounds. |

Generated tests must vary transfer count separately from simultaneous owner count.
They must include owner-set changes, repeated owners, partial transfers, and returns to previous owner sets.
Live consent metadata must not grow solely to retain superseded owners after every transfer.
Historical audit evidence and live authorization state have separate retention requirements.
They must preserve independent oracles for monetary balances, resource allowances, captured consent, and replayed effects.
Implementation remains incomplete until these native and formal requirements pass.

## Concurrent local publication model

[`FundingConsentPublication.tla`](../../../../formal/tlaplus/cost_accounted_rho/FundingConsentPublication.tla) extends the consent contract with separate prepare, commit, and abort actions.
Workers can prepare operations concurrently and commit them in different orders.
Each prepared operation captures its inputs before another worker can change published state.

The model describes one local publication domain.
It does not describe independent validators sharing one mutable ledger.
Per-right generation checks are local authorization checks, not a requirement for globally equal validator state roots.
Atomic publication is an abstract operation whose native implementation still needs a correspondence proof.

The checked configuration uses three workers, two right keys, two reservation identities, and one prepared operation per worker.
Owner terms contain either two or three owners, with different price ceilings, opaque limit terms, and schedule identifiers.
Prices are 1 or 3 in compatible abstract units.
The initial owner ceilings are 2 and 4. Replacement-owner ceilings are 4, 6, and 8.
These are test values, not production economic parameters.

The operation bound permits at most three committed operations in one explored history.
It does not replace the arbitrary-history Rocq proofs or impose a production transfer limit.

### Publication requirements

| Operation | Required behavior |
| --- | --- |
| Prepare | Capture provisional inputs without changing published rights, reservations, or settlement state. |
| Transfer | Check the captured right generation and change only the selected right. Preserve concurrently published reservations and settlement flags. |
| Reserve | Check current authorization, unused reservation identity, and every required price ceiling. Publish the captured consent record. |
| Settle | Check the reservation identity and unsettled status. Use captured consent without substituting current owners. |
| Abort | Discard only provisional state. Do not restore a snapshot over another worker's committed effects. |

A reservation committed before a transfer retains its original consent.
A locally stale reservation prepared before a transfer cannot commit afterward under an obsolete authorization generation.
That local rule does not decide cross-validator branch eligibility or the transfer of encumbered funds.

### Invariants and negative controls

| Invariant | Unsafe alternative that the gate must reject |
| --- | --- |
| `CapturedReservationsRemainImmutable` | Transfer restores an old reservation map, or another reservation overwrites an issued identity. |
| `SettlementUsesCapturedConsent` | Settlement substitutes the current owner's terms. |
| `AtMostOneSettlement` | Two prepared workers both commit settlement for the same reservation. |
| `SettledFlagsMatchReceipts` | Transfer restores an old settlement flag after another worker settles. |
| `TransfersUseFreshAuthorization` | Transfer accepts an obsolete generation. |
| `ReservationsUseRequiredConsent` | Reservation accepts the highest ceiling instead of satisfying every required ceiling. |
| `RightChangesStayInScope` | Transfer changes a different right key. |
| `AbortPreservesCommittedState` | Abort restores an old shared-state snapshot. |
| `IndependentReservationRemainsEnabled` | An unrelated publication invalidates a locally eligible reservation through a global freshness guard. |

The final invariant checks immediate enabledness, not eventual progress or general operation commutativity.
Settlement uses a receipt count as well as a Boolean flag.
Two commits that both write `true` must not evade duplicate-settlement detection.

Two additional configurations require reachability witnesses.
One requires ownership to change away and return while a reservation still holds the original generation.
Its specialized preparation history forces different intermediate owner terms.
The other requires settlement to use captured consent after an ownership change, with different live owner terms at settlement.
These configurations intentionally refute statements that deny the required traces.
They do not narrow the safe configuration's transition relation.

### Model review and evidence boundary

The first focused gate detected three ineffective negative controls.
Boolean assignment precedence turned their intended failure records into action guards.
Those guards prevented the unsafe transitions instead of exposing invariant violations.
The plan agent independently identified the same error.
The assignments now parenthesize their complete Boolean expressions.

The final focused gate passed on September 10, 2026.
It passed one safe configuration with nine invariants, ten mutation controls, and two required reachability witnesses.
The gate reported 13 passed and zero failed.
Each checker used a one-gibibyte Java heap, two checker threads, and a three-gibibyte memory ceiling with no swap.
The outer command also had a four-gibibyte memory ceiling and a two-CPU quota.
All model state files used the repository's disk-backed `target/` directory, not `/tmp`.
This finding concerns the model itself. It does not demonstrate a corresponding native node defect.

The model does not verify cryptographic authorization, funding-proof validity, monetary backing, or resource-limit interpretation.
It also does not execute the Rocq functions or prove native checkpoint atomicity.
An independent-validator model must still cover immutable causal roots, whole-effect rejection, compatible branch merge, and delivery-order independence.
Native tests must establish those behaviors through the existing Casper selection rules without introducing global serialization.
