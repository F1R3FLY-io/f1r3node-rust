# Multi-wallet funding findings and repair evidence

## Status and evidence boundary

This matrix maps funding findings to implementation requirements and verification evidence.
The evidence date is September 10, 2026.
This document does not certify implementation completion or authorize a new economic policy.

The [funding audit](multi-wallet-funding-path-audit.md) supplies source locations, source hashes, formal bounds, and historical regression results.
Its reviewed SHA-256 is `c440285cf62d2e1c0a674f2ce3abc38e6722633136dcff556236a1fbec03d2a9`.
Audit acceptance does not establish that the audited implementation satisfies the replacement contract.

The base commit is `9e58cf211885b2cdb319e030efa2af0491d71471`.
The audit also inspected uncommitted changes.
Its source hashes, rather than the base commit alone, identify those changes.
No new compilation, model checking, or integration run supports this matrix.

## Interpretation

A **logical authority** identifies required authorization occurrences.
A **custody key** identifies a physical balance, including its custody role.
A **monetary obligation** specifies the total asset amount to fund.
These concepts are distinct even when one wallet supplies all three.

Each row specifies evidence required to establish the corresponding implementation claim.
An independent reviewer must assess that evidence.

The following disposition terms apply:

- **Replace** requires implementation work under the approved replacement contract.
- **Integrate** requires connection between existing components and the production path.
- **Verify** requires evidence for existing behavior, including negative cases.
- **Decide** requires an explicit policy decision before implementation.
- **Limited exclusion** rejects an overbroad claim but does not certify every related path.

## Original findings

The identifiers NPA-1 through NPA-9 refer to the historical findings in the reviewed audit.
The current disposition accounts for subsequent source changes.

| Finding | Current disposition and reason | Required acceptance evidence |
| --- | --- | --- |
| NPA-1: Binary structural helpers | Limited exclusion of a universal two-wallet runtime limit. The inspected production path uses state-bound admission. Replace or retire obsolete helpers after caller and compatibility checks. | Inventory wrappers, tests, exports, and production callers. Exercise authenticated leaf funding from one payer through the configured cap. Reject zero and cap-plus-one. Preserve logical multiplicity. |
| NPA-2: Authority consumption versus monetary sharing | Replace the monetary interpretation, not the paper's complete authority requirement. Restricted feasibility alone does not implement the approved optimizer. | Prove backing conservation under paper-authorized regrouping. Compare complete native assignments with an independent small-state minimax oracle. Vary owner count independently from authority count. |
| NPA-3: Multiplied deployment fee | Integrate and verify the current one-total-unit fee repair. The historical per-signer fee is not the current source rule. | Query every payer and recipient through execution and replay. Derive the fee from the independent obligation. Include selected threshold members, failure, resource debits, and repeated cursor transitions. |
| NPA-4: Byte charges | Integrate monetary pricing behind exact resource measurements. Do not replace authority checks with an aggregate balance sum. | Vary compute, introduction bytes, transferred bytes, and trace bytes independently. Check units, backing, limits, replay inputs, and boundary arithmetic. Gate storage implementation on the selected liability contract. |
| NPA-5: Narrow proofs | Verify the replacement contract and its production correspondence. Binary or scalar proofs remain valid only within their stated premises. | Map each theorem and invariant to production symbols and named tests. Record assumptions, model bounds, generator ranges, and expected-failing controls. |
| NPA-6: List settlement is not share calculation | Integrate the approved solver with existing list settlement. An exactly-two-entry property does not establish a two-entry implementation limit. | Generate list arity, aliases, roles, permutations, and each failure position. Check aggregate physical capacity, checked lowering, original refund custody, and atomic publication. |
| NPA-7: Carrier exchange is not priced conversion | Limited exclusion of an implemented asset-conversion claim. Decide and implement any required conversion contract separately from opaque carrier transport. | Authenticate source withdrawal, asset identity, destination, rate schedule, consent, and any slippage bound. Reject unauthorized routes. Preserve refund asset and custody. |
| NPA-8: Installation is not rent | Limited exclusion of a complete storage-lifetime tariff. Decide the specified liability before adding monetary rules. | Map installation, retention, release, and repeated firing to specification clauses. Distinguish implemented charges from an unselected rent policy. Do not invent a tariff to complete this row. |
| NPA-9: Conservation cannot select the correct fee | Verify fee-selection correspondence before composing existing atomic-settlement results. Conservation of an excessive supplied fee is insufficient. | Mutate the supplied fee while preserving transfer conservation. Require rejection against the independent obligation. Check the same constraint in native execution and replay. |

## Source-refresh and review findings

These rows cover findings that the original nine identifiers do not fully express.
Each row specifies an additional implementation or verification obligation.

| Identifier | Finding and disposition | Required acceptance evidence |
| --- | --- | --- |
| FM-10 | Integrate restricted funding. The public feasibility helper has no external production caller in the inspected Rust-source search. | Demonstrate the actual admission-to-settlement call chain. Prove minimax over the complete feasible set, not a supplied candidate subset. Retain eligibility and source-to-obligation assignments. |
| FM-11 | Use the approved minimax objective instead of max-min. | Restricted funding uses lexicographic minimax of descending contributions. Capped water filling applies only after all-to-all certification and a refinement proof. Establish algorithm-specific work bounds. |
| FM-12 | Decide restricted equal-cost ties and cursor rules before integration. Existing all-to-all rotation does not select these policies automatically. | Record approved labeled-assignment ties, equal-total acquisition alternatives, and zero-new-funding behavior. Decide unequal-total acquisition alternatives separately before fixed-total minimax. Prove canonical results without deploy-controlled order or entropy. |
| FM-13 | Restore authenticated price and limit controls. Reserved protobuf fields and helper budgets do not supply signed client consent. | Trace signed encoding, verification, dispatch, admission, metering, settlement, and replay. Enforce every required payer's price ceiling. Test version boundaries and unauthorized field changes. |
| FM-14 | Verify prepaid funding and ownership histories without a second charge or sponsor reimbursement. | Match compatible typed prepaid rights before calculating new funding. Preserve acquisition backing, captured consent, consumed allowances, and current ownership through arbitrary finite histories. |
| FM-15 | Verify shared physical capacity independently from logical occurrences. | Aggregate aliases across resources and scopes. Interleave top-ups, reservations, cancellation, and settlement. Compare permitted outcomes with a reference ledger. Use production state types where feasible. |
| FM-16 | Verify source-specific branch reservations. Maximum retained charge does not necessarily equal total temporary backing. | Test mutually exclusive branches with disjoint eligible sources. Require separate approved exposure or another approved funding construction. Distinguish search exhaustion from proven insufficient funds. |
| FM-17 | Verify fee cursor and balance publication as one native effect. | Test stale inputs, rejection, cancellation, replay, and permitted concurrent histories. Neither a cursor-only update nor a balance-only update may survive. |
| FM-18 | Preserve the distinction between funding grammar and capability operations. | Test accepted and rejected funding formers. Cover backed operator effects without silently accepting every capability former as a funding identity. |
| FM-19 | Verify lollipop, stack, and registry boundaries. A registered transformer is not automatically a verified conversion or funding proof. | Check complete stack movement, partial transfers, split/join backing, registry authorization, use counts, and revocation. Test stale and concurrent invocation. Preserve arbitrary finite transfer histories. |
| FM-20 | Verify issuance and mint identity. An arithmetic mint theorem does not establish authorization of every native mint path. | Enumerate genesis initialization, mint creation, privileged mint/burn, and ordinary deposits. Reject wrong mint identities and unauthorized issuance. Check one-use initialization and rollback. |
| FM-21 | Preserve validator custody roles and separate charges. The handler charge is three fuel units, distinct from the one-unit general deployment fee. | Check handler deferral, captured replay evidence, fuel top-up, bond, withdrawal, quarantine, and each redemption outcome. Assert role-specific balances, supply, generation, and idempotence. |
| FM-22 | Verify failure classification against retained state, not only return values. | Inject failures before reservation, after partial reservation, during execution, and through fee/refund publication. Check approved user-failure charges, platform-fault rollback, parser rejection, and system-deploy failures. |
| FM-23 | Close the production correspondence gap in formal evidence. Abstract traces and small models are not native execution proofs. | Bind models to source hashes and actual transitions. Document each abstraction and simulation relation. Use native regression or fault injection for boundaries that abstract models cannot execute. |
| FM-24 | Verify numeric-merge funding correspondence. The local accounting metadata repair does not change upstream numeric arithmetic or writer selection. | Preserve retained authority metadata and region multiplicity. Exclude consumed base authority and rejected outputs. Check regrouping invariance, subsequent funding, byte measurement, and replay. Reject conflicting region signatures and malformed numeric/resource-stack hybrids. |
| FM-25 | Verify authenticated initial-capacity discovery. Its historical repair is separate from the fee correction and later frontier expansion. | Exercise leaf-funded admission before the first authority event without intermediate compound pools or a later frontier. Exclude absent members and candidate-created balances. Aggregate physical aliases. Reject overflow and exhausted host-work budgets without mutation. |

FM-21 specifies verification of existing custody behavior.
It does not authorize a new Casper protocol, validator lifecycle, slashing rule, or finality rule.
Any discovered consensus defect must follow the user's separate Casper approval constraints.

FM-24 follows the [approved numeric reconstruction rule](cost-accounting-impl/numeric-merge-authority-preservation.md#approved-reconstruction-rule).
It preserves the distinction between an accounting integration repair and a change to Casper's branch decisions.
FM-25 retains the [historical initial-capacity regression](multi-wallet-funding-path-audit.md#historical-initial-capacity-repair-before-the-monetary-fee-repair) as a separate oracle.


## Required proof and test coverage

The [audit's proof inventory](multi-wallet-funding-path-audit.md#proof-and-test-coverage-refresh) contains the precise inspected bounds.
The following verification families must remain distinct.

| Evidence family | Current limit | Required completion evidence |
| --- | --- | --- |
| General Rocq statements | Some theorems quantify over arbitrary finite inputs but assume supplied monetary values or abstract native-named transitions. | Prove the missing input derivation and representation correspondence. State cryptographic and storage premises explicitly. |
| Minimax and feasibility proofs | Candidate optimality and complete finite enumeration do not establish an efficient production optimizer. | Connect the native algorithm to complete feasibility and the approved ordering. Report its arithmetic, space, and work bounds. |
| TLC state exploration | Existing configurations use small payer, worker, operation, and generation bounds. | Report every bound and explored result. Add concurrent alias, cancellation, top-up, transfer, failure, and replay schedules. Do not claim unbounded coverage from finite exploration. |
| Property tests | Some tests vary many payers. Others fix two vault entries or three feasibility sources. | Vary supported payer arity, roles, eligibility, aliases, histories, failures, and integer boundaries. Retain minimized counterexamples and test the independent oracle. |
| Loom tests | Several existing tests execute shadow state with two or three workers. | Identify which production synchronization and state types each test exercises. Explicitly record any remaining shadow-model gap. |
| Native integration tests | Source inspection found fee, stack, custody, and failure cases. The refreshed audit did not rerun these tests. | Run targeted regressions on the final source revision. Query complete retained state and replay the same inputs. Record commands, limits, outcomes, and source hashes. |
| Negative controls | A passing invariant can omit the incorrect economic input. | Mutate authorization, obligation, backing, role, cursor, price, and failure behavior independently. Show that each relevant checker or native regression rejects its mutation. |

Property tests must cover allocation, authorization, and pricing throughout execution.
Refinement evidence must connect those tests to formal statements and implementation behavior.
No one evidence family substitutes for the others.

## Decisions and scope controls

The [approved minimax design](cost-accounting-impl/lexicographic-minimax-funding.md) controls the allocation objective.
The [settlement review](cost-accounting-impl/funding-settlement-design-review.md) records the remaining policy boundaries.
The [three-paper traceability document](cost-accounting-three-paper-traceability.md) identifies specification inputs and their scope.

The following constraints remain in force:

- Keep logical authority complete under the paper's permitted reductions and regroupings.
- Share newly required funding without automatic reimbursement of earlier sponsors.
- Treat `phloPrice` as a maximum permitted price, not a contribution weight or an exchange rate.
- Preserve native SystemVault custody and one-checkpoint settlement without a persistent per-deploy escrow ledger.
- Preserve concurrency for independent custody and validators.
- Do not infer an unspecified rent tariff or currency-conversion policy from existing transport syntax.
- Keep deferred Casper architecture work outside this repair matrix.

## Completion procedure

1. Resolve the applicable policy decisions before implementing dependent transitions.
2. Record the production symbols and source hashes for each repaired row.
3. Establish the formal contract before changing the corresponding runtime behavior.
4. Extract each applicable invariant and negative control into a named regression.
5. Run the targeted proof, property, concurrency, and integration checks under resource limits.
6. Record each result against its matrix row.
7. Obtain independent acceptance without treating source inspection as execution evidence.

The matrix is complete as a mapping only when every finding has a disposition and an evidence owner.
The implementation remains incomplete until the required verification gates pass.
