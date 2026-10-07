# Persistent funding allowance

## Decision and implementation status

The user approved the two-layer funding plan on September 10, 2026, after plan-agent review.
Deployment `phloLimit` bounds one signed execution scope.
A persistent allowance bounds cumulative authorized draws across executions and ownership transfers.
Neither limit replaces signature-specific resource sufficiency.

This document records the first quantitative proof slice, not completed native integration.
The [signed contract](signed-phlo-contract-proposal.md) defines the surrounding consent requirements and unresolved economic choices.
The [ownership record](ownership-transfer-consent.md) documents the separate consent-history proofs.
No production runtime or Casper behavior changed for this proof slice.

## Stocks, flows, and identity

An allowance has one stable accounting identity.
Let $`Q`$ be its issued authorization, $`A`$ its available authorization, $`R`$ its reserved authorization, and $`C`$ its consumed authorization.
These quantities use one compatible unit within each equation.

```math
A + R + C = Q.
```

Released authorization returns to availability.
It is a flow, not an additional stock in the equation.
Wallet balance is backing, not permission to increase $`Q`$.

| Operation | Required condition | Allowance effect |
| --- | --- | --- |
| Reserve $`x`$ | Current authorization and $`x \le A`$. | Decrease $`A`$ by $`x`$. Increase $`R`$ by $`x`$. |
| Settle reservation $`x`$ with debit $`d`$ | The reservation remains open and $`d \le x`$. | Remove $`x`$ from $`R`$. Add $`d`$ to $`C`$. Return $`x-d`$ to $`A`$. |
| Release reservation | The reservation remains open and no charge is authorized for this release. | Remove $`x`$ from $`R`$ and return $`x`$ to $`A`$. |
| Transfer authorization | The transfer presents current authorization. | Advance authorization version without changing $`Q,A,R,C`$. |
| Expand allowance | An explicit amendment presents current authorization. | Increase $`Q`$ and $`A`$ by the amendment amount. Advance authorization version. |
| Top up wallet | The wallet operation supplies its own authorization. | Change wallet backing outside this model. Leave the allowance unchanged. |

For example, an allowance starts with 1,000 units.
A deployment reserves 100 units and consumes 40 units.
Settlement returns 60 units, leaving 960 available and 40 consumed.
Ownership transfer and a 500-unit wallet top-up leave that allowance unchanged.

The model's zero-debit `Abort` means release of a reservation without a billable effect.
It does not select zero charges for every failed deployment.
Failure fees and billable failed work remain separate policy decisions.

## Executable Rocq model

[`PersistentFundingAllowance.v`](../../../../formal/rocq/cost_accounted_rho/theories/PersistentFundingAllowance.v) implements pure, partial transitions over natural numbers.
`allowance_step` returns `None` for rejected operations without returning a modified state.
`allowance_history` stops at the first rejected operation.
The generated-history tests also continue from the unchanged input after rejection.

Each reservation occupies a distinct list position.
Closing a reservation retains an empty position, so later reservations do not reuse its identity.
This list is a mathematical representation, not the proposed production storage layout.
The native design still needs bounded live storage and replay-safe identity retention.

| Proof or check | Obligation |
| --- | --- |
| `arbitrary_mixed_history_conserves` | Every successful finite mixed history preserves the allowance equation. |
| `arbitrary_mixed_history_never_resets_consumption` | No successful history reduces consumed authorization. |
| `settlement_cannot_repeat` | An immediate second close of the same reservation fails. |
| `transfer_preserves_allowance_stocks` | Transfer preserves all stocks and reservation entries while advancing authorization. |
| `top_up_does_not_expand_allowance` | Wallet top-up leaves the entire allowance state unchanged. |
| `generated_zero_allowance_histories` | Check every length-four history from 19 commands with initial allowance zero. |
| `generated_funded_allowance_histories` | Check the same command histories with initial allowance two. |

Each generated suite checks 130,321 command sequences, including rejected operations and zero amounts.
The generated properties check conservation, nondecreasing consumption, explicit issuance changes, and authorization-version changes.
Separate examples cover stale authorization, competing reservations, refunds, and repeated closure after another reservation.
These are executable formal-model regressions, not native Rust property tests.

The unbounded history proofs do not impose the generated tests' four-operation bound on production behavior.
Authorization inputs remain abstract booleans.
This module alone does not prove signatures, resource bounds, price consent, or physical backing.

## Consent and quantity composition

[`ConsentedFundingAllowance.v`](../../../../formal/rocq/cost_accounted_rho/theories/ConsentedFundingAllowance.v) combines the quantity transitions with owner price ceilings and captured reservation terms.
It reuses the minimum-ceiling function, consent record types, and executable allowance transitions.
Each captured record includes its grant, authorization version, terms, causal root, price, and reserved amount.
An invariant pairs each live quantity slot with the correct captured record and grant.

| Theorem | Established result |
| --- | --- |
| `reservation_checks_price_and_allowance` | Admission checks every required ceiling, available allowance, current version, and the external authorization/funding input. |
| `consented_arbitrary_history_preserves_validity` | Arbitrary successful mixed histories preserve conservation and quantity/capture correspondence. |
| `consented_arbitrary_history_preserves_captured_consent` | Existing captured records remain unchanged across arbitrary successful mixed histories. |
| `settlement_matches_captured_amount` | Debit cannot exceed the captured reservation. The exact unused amount returns to availability. |
| `consented_settlement_cannot_repeat` | A second close of the same reservation fails. |
| `arbitrary_history_retains_price_authorization` | Every retained capture keeps its original valid price authorization throughout arbitrary successful mixed histories. |
| `captured_price_satisfies_every_required_owner` | Captured price authorization satisfies every ceiling in its required owner list. |

Two generated suites each check 50,625 length-four command sequences over 15 commands.
They start from zero and two allowance units, respectively.
The checked observations include conservation, paired quantities and captures, grant identity, and captured price validity.
Example tests transfer from three owners to four owners, then settle under the original captured consent.
Other examples reject a higher price, insufficient allowance, stale owner-return authorization, and a new draw above the replacement owners' ceiling.

These proofs accept arbitrary finite owner lists and histories.
The generated suites and TLA+ configurations use finite examples, not production owner-count limits.
The composition is not a refinement proof for every transition of `FundingConsentHistory`.
`consented_history_refines_quantity_history` projects every admitted consented history to the existing quantitative allowance transitions.
The projection preserves actual settlement debits, rather than replacing settlement with an abstract balance update.
It supports unbounded-history proofs for nondecreasing consumption, stable grant identity, and issuance changes limited to explicit expansions.

The [persistent activation composition](operator-authority-proof-boundaries.md#persistent-activation-and-recorded-settlement) adds located cell consumption and recorded firing charges.
It requires closure to settle recorded work and proves that reusable permission cannot recreate cells.
That composition uses an explicit valuation interface. It is not completed native persistent-grant integration.

The remaining abstract inputs include authorization, physical funding sufficiency, resource proofs, compatible units, and causal-root validity.
Signed limits remain opaque terms in this slice.
The model does not yet establish partial-transfer liability routing or cross-validator replay correspondence.

## Shared physical backing and refunds

Different grants can reference the same physical purse.
Each grant retains its own authorization constraints, but those grants cannot each reserve the full physical balance independently.
The check must aggregate their physical draws before accepting their combined effects.
It must not remove logical authority requirements when it combines physical custody entries.

[`CanonicalCustodyAliasing.v`](../../../../formal/rocq/cost_accounted_rho/theories/CanonicalCustodyAliasing.v) now covers successive reservation batches and original-custody refunds.
Its proofs quantify over arbitrary finite lane lists and a supplied lane-to-purse mapping.
The mapping remains fixed for each reservation and refund calculation.

| Theorem | Obligation |
| --- | --- |
| `physical_reservation_batches_use_remaining_capacity` | Admitting combined batches equals checking the second batch against capacity remaining after the first admitted batch. |
| `physical_refund_matches_original_custody` | Per-purse refunds plus actual draws equal per-purse reservations, including aliases. |
| `physical_actual_cannot_exceed_reservation` | Component-bounded actual draws cannot exceed the aggregate reservation for a purse. |
| `physical_reserve_then_refund_conserves` | Remaining balance, refunds, and actual draws reconstruct each original purse balance. |

[`RotatingMonetaryAllocation.v`](../../../../formal/rocq/cost_accounted_rho/theories/RotatingMonetaryAllocation.v) now covers allocation against reserved capacities.
`reservation_bounded_settlement_preserves_original_caps` proves that this second allocation cannot exceed the original capacities.
`allocation_from_reserved_caps_has_exact_refunds` proves that refunds plus actual debits equal the reservation total.
These results do not infer componentwise monotonicity when unrelated input capacities change.

Native regression coverage uses the existing allocator and custody operations:

| Test | Covered requirement |
| --- | --- |
| `monetary_reservations_cannot_reuse_shared_physical_capacity` | Two six-unit plans cannot both consume a shared ten-unit purse. Aggregate rejection leaves the input balance unchanged. |
| `reservation_bounded_settlement_preserves_caps_and_refunds` | Generated full-width quantities preserve per-purse exposure and exact total refunds. |
| `reservation_bounded_settlement_matches_independent_oracle` | Generated bounded quantities match the independent allocation oracle for reservation and actual settlement. |
| `monetary_reservation_refunds_preserve_original_custody` | Generated custody aliases preserve physical refund attribution and original balances. |

The three property tests each run 256 generated cases.
All 35 tests in the native monetary-allocation module passed in 5.13 seconds after a 25.88-second incremental build.
Strict Clippy passed for the Rholang library and test targets in 9.56 seconds.
These tests exercise production primitives, not a completed persistent-grant transaction path.
The shared-purse regression did not uncover a new failure in those primitives.

The proofs and tests assume compatible units for each reservation, debit, refund, and backing comparison.
They do not approve a REV-to-phlo tariff or collapse signature-indexed fuel into one scalar balance.
Physical conversion, authenticated funding evidence, and integration with resource sufficiency remain separate obligations.

## Concurrent TLA+ model

[`PersistentFundingAllowance.tla`](../../../../formal/tlaplus/cost_accounted_rho/PersistentFundingAllowance.tla) separates preparation from publication.
Workers can prepare against the same allowance state before either worker commits.
Publication checks current available authorization and authorization version.
Settlement uses its identified reservation even after authorization changes.
Discarding a provisional operation must not restore a stale shared snapshot.

The checked configuration has three workers, two grants, two reservation identities, and two initial units per grant.
Each worker prepares at most one operation.
Both grants can receive one modeled wallet top-up.
Expansion adds one unit and requires the captured authorization version.
Worker permutation symmetry reduces equivalent schedules.
The model also includes two owner configurations with three and four required ceilings.
Their ceilings are `[3, 5, 8]` and `[1, 2, 3, 4]`.
Candidate prices are 1, 2, and 4 in one compatible unit.
Transfer changes the current owner configuration but preserves existing reservations.
The extended configuration also considers all four mappings from two grants to two physical purses.
This includes shared custody and separate custody.
Each physical purse starts with two backing units.
Wallet top-ups increase physical backing without expanding grant authorization.
Reservation publication checks both available grant authorization and current physical backing.

| Configuration | Expected result |
| --- | --- |
| Safe configuration | Grant and wallet conservation, no overdraw, receipt agreement, explicit expansion, single closure, required price consent, and captured settlement consent hold. |
| `StaleCapacityUnsafe` | A stale capacity check violates `NoOverdraw`. |
| `ResetConsumedUnsafe` | Transfer resets consumed authorization and violates `ConsumptionMatchesReceipts`. |
| `RepeatedSettlementUnsafe` | Competing close operations violate `AtMostOneClose`. |
| `LostRefundUnsafe` | Dropping the unused reservation violates `Conservation`. |
| `TopUpExpansionUnsafe` | A wallet top-up increases authorization and violates `NoImplicitExpansion`. |
| `AbortSnapshotUnsafe` | Discard restores old availability and violates `Conservation`. |
| `TransferredSettlementReachable` | Settlement after transfer refutes the deliberately false unreachability claim. |
| `HighestCeilingUnsafe` | Accepting any owner's ceiling violates `ReservedPriceAuthorized`. |
| `CurrentOwnerUnsafe` | Settlement uses replacement owner terms and violates `SettlementUsesCapturedConsent`. |
| `ConsentOnlyUnsafe` | Publishing a reservation without the quantity debit violates `Conservation`. |
| `QuantityOnlyUnsafe` | Debiting quantity without the reservation record violates `Conservation`. |
| `StaleWalletUnsafe` | A captured wallet capacity permits competing draws and violates `NoWalletOverdraw`. |
| `IgnoreCustodyUnsafe` | Checking grant capacity without physical backing violates `NoWalletOverdraw`. |

The physical-backing safe check and all thirteen expected refutations passed on September 10, 2026.
Twelve controls exercise unsafe variants. One establishes reachability in the safe model.
Initial validation rejected an inconsistent idle-state representation and incomplete next-state assignments. Those model errors were corrected before these results.
These checks did not reveal a new native Casper bug.

The model represents one local publication domain, not a global ledger shared synchronously by validators.
Atomic publication is an assumption that native integration must establish.
The check does not establish distributed liveness, unbounded concurrency, or equivalence between Rocq and TLA+ transitions.

## Verification commands and resource limits

Both Rocq modules compiled, including their generated regressions, with `MemoryMax=2G`, `MemorySwapMax=0`, and `CPUQuota=200%`.
Independent kernel checking passed for both modules under the same limits.
The seven registered composition theorems report closed assumptions under the global context.
The extended custody and allocation modules also compiled and passed independent kernel checking under a two-gibibyte limit with no swap.
The TLA+ gate used a four-gibibyte outer limit, a one-gibibyte Java heap, two workers, and no swap.
Each checker had a three-gibibyte limit and a 180-second timeout.
Checker state directories were on disk under `target/verification/claims-audit-20260909/`, not the `/tmp` memory filesystem.

Run the allowance checks through the registered gate:

```sh
systemd-run --user --scope -p MemoryMax=4G -p MemorySwapMax=0 \
  -p CPUQuota=200% env TLC_HEAP=1g TLC_RSS=3G TLC_WORKERS=2 \
  TLC_WALL_TIMEOUT=180s \
  TLC_METADIR_ROOT="$PWD/target/verification/allowance-tlc" \
  bash scripts/check-cost-accounted-rho-tla-invariants.sh \
  --filter PersistentFundingAllowance
```

The native allocator tests used a six-gibibyte limit and no swap.
Clippy used a three-gibibyte limit and no swap while the four-gibibyte model-checking scope ran independently.
Rust temporary files used an explicit disk-backed `TMPDIR` under the verification directory.
The complete repository proof gate and full native test suites were not rerun for this isolated slice.

Checked source identities:

| Source | SHA-256 |
| --- | --- |
| `PersistentFundingAllowance.v` | `80ab02889df83ac2e8eac29698cd6175c86e2d6aff7af6a0d0b907e2567fe578` |
| `ConsentedFundingAllowance.v` | `0d6bdad2cbdc7fec2be3313226482c871bf7d4344101fe23567bf5113e1f1314` |
| `CanonicalCustodyAliasing.v` | `9b61a047bc5c2b6fdcfd47dd338bd3fa62a7bfabe931d20cbb4ddcc33b020d72` |
| `RotatingMonetaryAllocation.v` | `5a74cc7ca0e49085dba4f837dfb6cb978f332240f5c4aa9a8b173e316c35f852` |
| `PersistentFundingAllowance.tla` | `4ef97187e0ea7728e30f968e05a9bee7f5aee7adc7cba50deafd59361119c36e` |
| Native allocation tests | `8f068852c3b349c8aba8ac979b3f88b6c7cbd3ee0d58fe8305cc29eae1458926` |
| Native custody tests | `f74762ba22326f3e2de26efd5613d14d169d42b22a17d6f64646dc8df0636a2f` |

## Remaining integration sequence

| Stage | Required result | Completion evidence |
| --- | --- | --- |
| Signed contract | Bind deployment scope, grant identity, units, owner consent, schedule, and maximum exposure. | Canonical encoding and stale-term rejection tests. |
| Model composition | Extend the checked quantity/consent composition to signature-specific resource proofs, physical backing, and signed deployment limits. | Rocq composition proofs and concurrent counterexample controls. |
| Rights partition | Divide available authorization without cloning it. Preserve restrictions and refund provenance. | Arbitrary split, join, transfer, and owner-return properties. |
| Causal replay | Model independent validator roots and accepted whole effects. | Equal accepted effects produce equal allowance state, regardless of arrival order. |
| Pure native transitions | Use checked arithmetic and preserve the model's accounting invariants. | Generated Rust properties, overflow tests, and explicit refinement evidence. |
| Runtime publication | Publish reservations, settlement, and refunds atomically with their retained effects. | Loom tests for actual synchronization boundaries and signed native regression tests. |
| Lifecycle integration | Exercise deposits, repeated draws, transfers, top-ups, refunds, and cold replay. | Multi-owner integration tests with original-custody refund assertions. |

No stage may introduce a global execution lock or a new Casper voting, finality, or branch-selection rule.
Disjoint rights must remain independently usable.
Shared custody and competing grants must expose their actual dependencies.

The remaining economic gates are resource-to-money mapping, units, failure charges, conversion rules, and encumbered partial-transfer routing.
The first paper's `sec:data-dependent` requires conservative overcharge-and-refund for its covered semantics.
Its requirement does not independently specify every monetary or persistent-allowance policy.
The [signed contract's specification section](signed-phlo-contract-proposal.md#specification-and-implementation-boundaries) identifies the inspected paper sources.
