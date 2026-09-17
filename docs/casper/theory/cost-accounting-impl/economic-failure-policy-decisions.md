# Economic failure policy decisions

## Status and scope

This decision package defines economic failure policy.
It separates existing source behavior from recommendations that still require review and approval.
It does not authorize runtime changes or introduce an escrow ledger.

The [allocation ratification](authority-allocation-policy-ratification.md) already selects contribution fairness, restricted priority rotation, zero-funding contribution state, and equivalent-acquisition ordering.
Those choices are not open questions in this package.
The [settlement review](funding-settlement-design-review.md) supplies the remaining prepaid, reservation, conversion, and failure boundaries.

## Specification constraints

The [rho paper](../../../../../publications/cost-accounting/cost-accounted-rho.tex) distinguishes communication atomicity from financial-transaction atomicity in `rem:db-atomicity`.
One communication must either complete or have no effect.
A multi-step financial operation additionally needs sufficient funding and the applicable deployment boundary.

In `def:conservative-demand`, the paper requires enough funding for every covered branch and refunds unused funding from the actual run.
An accepted operation exhausting a certified sufficient bound contradicts that certificate's contract.
It is not evidence that the user authorized a larger debit.

The paper also describes subordinate capability exhaustion and dynamically supplied token stacks under programmable delegation and just-in-time funding.
A blocked located interaction and a failed atomic financial operation are not interchangeable events.
The implementation must distinguish their scopes rather than apply a universal deployment rollback rule to both.

The [continued-GSLT paper](../../../../../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex) supplies the matching located-resource discipline.
Equal monetary value does not substitute for compatible authority or resource rights.
The [funding audit](../multi-wallet-funding-path-audit.md) records the inspected specification and implementation boundaries.

## Current source observations

These observations come from source inspection on September 11, 2026.
They are not fresh test results or proof of the replacement contract.

| Boundary | Source evidence | Limit of the evidence |
| --- | --- | --- |
| Deterministic user failure | `failed_state_bound_body_rolls_back_writes_and_commits_its_charge` checks rolled-back user writes, realized charges, one deployment fee, and matching replay roots. | This example does not classify every interpreter error or prove the new signed exposure contract. |
| Reservation failure | `state_bound_reservation_failure_rolls_back_the_retained_user_execution` checks rejected admission and unchanged payer, recipient, proposer, and validator-fuel balances. | It does not prove every interruption or concurrent reservation history. |
| User evaluation | `process_deploy` uses an inner checkpoint and records authority events, byte events, failure status, and exhaustion. | Error classification and the retained economic projection still need correspondence proofs. |
| Atomic settlement | The state-bound runtime rejects excessive settlement and applies resource debits, fee, handler charge, and cursor evidence through the native vault path. | Conservation of supplied amounts does not prove that each supplied charge is authorized. |
| Platform and settlement errors | Error paths reset the candidate root or reject the candidate without a successful processed result. | A reset failure needs fail-closed recovery. It cannot justify publishing a partial state or charging users. |

Capacity discovery can exhaust a trial before certification, reset candidate effects, and retry with additional authority evidence.
These trials do not demonstrate accepted execution exhausting a certified sufficient bound.

Sources:

- [Runtime lifecycle](../../../../casper/src/rust/rholang/runtime.rs)
- [Native runtime regressions](../../../../casper/tests/util/rholang/runtime_manager_test.rs)
- [SystemVault settlement](../../../../casper/src/main/resources/SystemVault.rho)
- [Handler cost definition](../../../../casper/src/rust/util/rholang/costacc/mod.rs)

## Recommended prepaid and exposure rules

| Decision | Recommendation | Required boundary |
| --- | --- | --- |
| Prepaid acquisition price | Preserve the acquired right's captured terms while that right remains semantically compatible. | Do not charge the acquisition again at consumption. Price genuinely uncovered work separately. |
| Incompatible prepaid right | Require an authorized migration or conversion, or reject its use. | Do not silently reinterpret its denomination, authority, location, or resource version. |
| Current reservation refund | Return unused backing to its captured source custody and asset path. | Ownership transfer cannot redirect an earlier reservation's refund. |
| Available-right transfer | Transfer only available rights under the existing ownership rules. | Preserve consumed allowances and encumbered obligations. A partial transfer cannot duplicate either. |
| Restricted branch exposure | Permit larger source-specific temporary backing only with explicit signed exposure consent. | Otherwise require a sufficient authorized common source or branch evidence, or reject admission. |
| Wallet top-up | Increase available balance without increasing allowance, price consent, or an existing certified reservation. | A new plan must capture any newly permitted funding. |

For example, one branch can require one unit from A while another requires one unit from B.
The maximum retained charge is one unit, but conservative source-specific backing can require one unit from each purse.
The extra temporary exposure requires explicit consent.
It does not authorize retaining both units when only one branch executes.

Temporary reservation calculations remain private or proof state inside the existing native lifecycle.
They must not create independently spendable intermediate balances or persistent per-deploy escrow.
The reservation proof must preserve every certified outcome's feasibility within its captured source bounds.

## Recommended failure matrix

A **billable event** is a completed event that the approved resource schedule and captured authorization permit charging.
A trace entry alone does not establish billability.
The formal refinement must derive the retained economic effect from authenticated event evidence.

| Outcome | Recommended application effect | Recommended user economic effect |
| --- | --- | --- |
| Invalid syntax, signature, consent, price, or insufficient certified funding before acceptance | Publish no application effect. | No user charge or fee from the rejected candidate. |
| Successful accepted operation | Publish the authorized application result. | Charge approved realized work and the separate deployment fee. Refund unused current backing to original sources. |
| Accepted deterministic user-code failure | Roll back the applicable failed user operation. | Retain only authorized billable work and the separate fee within captured limits. Refund unused backing. |
| Missing local capability for an interaction outside a certified atomic operation | Do not fire that interaction. Preserve the applicable located process state. | Do not charge an unfired communication. Other independently permitted work follows its own scope. |
| Exhaustion that contradicts a certified sufficient bound | Reject the candidate and restore the applicable pre-state. | Do not publish user charges from the invalid certificate or bill the unperformed work. Record a correctness failure. |
| Host-work search exhaustion | Reject without a claim of proven insolvency or an optimal plan. | No candidate user debit. Existing host admission limits remain necessary. |
| Storage, interpreter-platform, certificate, or settlement failure | Reject the candidate. Restore its pre-state or enter fail-closed recovery if restoration fails. | Publish no partial charge, refund, allowance, or cursor mutation. |
| Duplicate delivery or retry after an already committed result | Apply existing identity and replay rules. | Do not charge or refund the same obligation twice. |

The matrix does not introduce a relaxed underfunded mode for certified atomic financial operations.
Signed limits that cannot cover required certified demand cause rejection before acceptance.
Persistent processes require the correct activation and allowance boundary for each funded execution scope.

Failed user execution must not both restore a consumed prepaid right and retain credit for its consumption.
Conversely, rollback must not destroy unused rights or redirect refunds.
Deriving that projection across births, transfers, consumption, and failure is an explicit proof and regression requirement.
It is not established by the existing single failure example.

The validator handler charge remains separate from the user's monetary fee and purse role.
The current handler constant is three validator-fuel units.
The current monetary deployment fee is one total unit, not one unit per signer.
This package does not change either amount or authorize slashing for a local execution fault.

Unknown error variants must not default to billable user failure.
Each variant needs an explicit classification and retained-state test.
Node-local timing, panic text, or operating-system errors must not select a consensus-visible user charge.

Mixed failures require an order-independent classification.
Any platform failure, invalid certificate, or unclassified failure prevents publication of all candidate economic effects, even when user errors also occur.
Flatten nested aggregate errors before classification.
Only an entirely classified user-failure result can retain the authorized billable prefix and fee.
Later settlement or publication failures override that result and require rollback or fail-closed recovery.
Failure classification must preserve this distinction. It must not charge every event described as exhaustion.

## Conversion ordering

The recommendation supports two explicitly signed compositions.
Neither composition is an implicit fallback for the other.

| Composition | Commit boundary | Unused funding |
| --- | --- | --- |
| Separate conversion before funding | An authorized trade commits before an independent funding request. | Refund the later reservation in its captured reservation asset and custody. The earlier trade remains committed. |
| Atomic quote-backed funding | Conversion, approved charges, rights, refunds, allowances, and receipts publish through one native checkpoint. | Release unconverted input to its original input asset and custody. No reverse trade occurs. |

The separate composition cannot satisfy the atomic original-input refund obligation.
Client consent and receipts must identify the selected composition and its failure boundary.

### Atomic exact-output contract

An exact-output quote defines the input required for each permitted output amount.
The output funds the approved realized obligation, including applicable user-failure work and the separate deployment fee.
It does not determine the consensus resource tariff or override allocation fairness.

The signed quote must bind these terms:

- Input and output asset identities, original custody, provider authority, and recipients.
- A deterministic exact-output pricing rule, integer rounding, fixed fees, minimum fees, and maximum fees.
- The authenticated evaluation context, expiry rule, and supported output range.
- Final input and output debit caps, distinct from temporary input and provider-output holds.
- Zero-output behavior and the treatment of classified user failures.
- The conversion composition and the applicable funding, schedule, and exposure commitments.

Quote evaluation uses captured authenticated context, not wall-clock timing or an external market lookup during replay.
Zero required output causes no conversion and no conversion fee in this composition.
A quote that requires a nonzero fee for zero output is unsupported and must be rejected.
Positive output includes its captured fees and rounding in input bounds before acceptance.

Reserve sufficient input and provider output capacity for every certified outcome within signed exposure limits.
These holds remain native candidate state or proof state, not independently spendable balances or persistent deployment escrow.
At retained settlement, debit only the exact quoted input for the approved realized output.
Release all unused input and provider capacity to their original custody.
Provider output cannot become independently spendable before atomic publication.

For example, a quote requires two input units per output unit and no fees.
A ten-unit input hold can cover five output units.
If the approved realized obligation requires three output units, debit six input units and release four input units to their original custody.

Preacceptance rejection and platform or certificate failures publish no conversion, user charge, or conversion fee from the candidate.
Classified user failures follow the failure matrix and convert only the retained authorized obligation.
Unsupported quotes fail explicitly rather than trigger separate conversion or a later reverse trade.

Different input assets require approved acquisition equivalence and a common valuation before comparing monetary contribution ranks.
Output totals alone cannot justify bypassing the approved lexicographic minimax allocation.
Asset identity, physical aliasing, and provider capacity must remain explicit in the complete feasible funding domain.

The opaque carrier swap remains distinct from an authenticated priced vault conversion.
No new wallet-specific consensus resource tariff follows from different market quotes.

## Formal and executable acceptance

- Complete failure classification and retained charges
- Prepaid compatibility, transfer, and refund provenance
- Source-specific charge and exposure limits
- Authorized conversion and asset recovery
- Paper-to-native correspondence
- Parallel candidate and publication behavior
- Generated histories and concrete state assertions

The formal model must distinguish user failure, local lack of capability, invalid sufficiency, search exhaustion, and platform failure.
It must model independent candidates and shared custody rather than serialize all operations by assumption.
Generate native regressions from each invariant and expected-failing control.
Assert complete retained balances, rights, allowances, application effects, receipts, and cursor states.

Inject failures before and after acquisition, reservation, each consumption, fee transfer, refund, and publication.
Include concurrent top-ups, ownership transfers, duplicate delivery, stale evidence, and restart.
Preserve independent-scope concurrency and existing Casper architecture.

Test every permutation of mixed errors, including nested aggregates and late settlement failures.
Cover prepaid rights that predate execution, arise during execution, or change owners before failure.
Verify unique retained effects without restoring consumed rights or destroying unused rights.

For conversion, test zero output, fee boundaries, integer rounding, expiry boundaries, insufficient provider capacity, custody aliases, and exact original-input release.
Test concurrent candidates that share provider capacity and candidates that use independent providers.
Reject quotes whose certified output range cannot fit input, output, fee, and temporary exposure limits.

## Decision boundary

This package still requires independent technical review and explicit policy approval.
In particular, conversion composition and failure-charge classification must not become production behavior through an implementation assumption.
No new storage rent tariff or activation schedule is selected here.
