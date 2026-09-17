# Price transitions and replay

## Purpose and activation scope

An accepted execution must retain its original pricing rules when a node validates or replays it later.
New funding operations must use the rules authorized for their own execution context.
This contract separates those requirements from permission to change a running shard's economy.

The [ratified activation policy](economic-activation-policy-ratification.md) selects fresh genesis for the new economy.
Imported economic state and in-place upgrades remain outside that release scope.
This contract does not choose an activation height, new Casper version, voting rule, or schedule-update authority.
It does not authorize dynamic tariff changes in a running shard.

Transition tests must nevertheless distinguish different supported historical contexts and changed client or owner terms.
Conditional tests for a future approved schedule transition cannot be presented as proof that such activation already exists.
Legacy and activated replay remain explicit release requirements under the [compatibility contract](activation-migration-policy-decisions.md).
Fresh genesis does not waive those requirements.

## Compatibility identity

An executable compatibility entry must identify the complete accepted semantics, not only a single version number.
Keep these identities separate:

- Genesis and network identity, including the shard context.
- Casper block protocol version.
- Deploy authorization format and signing domain.
- Authority-accounting and resource-measurement versions.
- Price schedule, valuation, settlement asset, and fixed-fee rules.
- Native funding, allowance, conversion, and receipt interpretation.

Each supported combination needs a concrete decoder, verifier, execution path, and retained fixture corpus.
Successful decoding alone does not establish executable historical compatibility.
Unknown or unsupported combinations must stop before economic mutation.
They must not receive current default prices, unlimited limits, or a guessed asset mapping.

Signed terms use the [exact schedule commitment](price-schedules-and-denominations.md#schedule-identity-and-authenticated-context) or an explicitly authorized compatibility entry.
A lower new price does not make an unrelated schedule compatible.
A higher signed ceiling does not authorize different resource weights, fee units, or acquisition semantics.
Changing to the restored signing contract requires new signatures rather than editing an old envelope in place.

## Preparation, acceptance, and later settlement

| Boundary | Required pricing behavior |
| --- | --- |
| New request | Validate its supported signing format, exact schedule consent, units, limits, and permitted source domain. |
| Candidate preparation | Capture the authenticated causal inputs, applicable schedule, owner terms, source capacities, and proof assumptions without publishing economic effects. |
| Candidate rebuild | Recompute affected proofs and allocations when relevant captured inputs change. Do not reuse an obsolete price or capacity certificate. |
| Publication | Check that the accepted result satisfies its captured evidence and the existing native dependency rules. Publish one complete economic result. |
| Accepted obligation after owner transfer | Preserve its original price, schedule, allowance capture, source exposure, and refund provenance. |
| New draw after owner transfer | Validate the replacement authority and terms, including retained restrictions, under the applicable execution context. |
| Historical replay | Reconstruct the original accepted inputs and rules, then reproduce the original economic result. |

Stale means that a relevant precondition no longer holds for the candidate's intended publication context.
It does not mean that an unrelated purse changed or that another validator has a newer local finalized block.
Do not require equality with one latest global root to validate an otherwise valid captured history.
Use existing causal dependencies and conflict rules without introducing a global price or funding lock.

A plan based on old owner consent cannot authorize a new draw after that authority changes in its applicable state.
An already accepted obligation can still settle under its captured consent.
The distinction prevents both stale authorization and retroactive repricing.
It does not introduce a persistent per-deploy escrow record or a new cross-validator settlement protocol.

For example, owners initially permit price three and later replace that permission with price one.
An accepted draw at price two keeps its captured terms.
A pending new draw at price two must pass the applicable current-authority checks before publication.
Historical replay of the earlier accepted draw must not reject it merely because the later owners permit only one.

## Exact arithmetic across contexts

Integer resource weights and integer prices use exact checked multiplication and addition.
The [pricing contract](price-schedules-and-denominations.md#signed-ceilings-and-exact-arithmetic) defines the compatible native denomination for adding usage and fee.
Every supported historical context must retain its original measurement, integer scale, overflow rules, and fee treatment.
An implementation upgrade must not reinterpret an old weighted byte total as a new resource vector.

Quote-based conversion requires the captured quote's exact-output, fee, and rounding rules.
Replay cannot obtain a replacement quote, reverse-trade a refund, or use a current exchange rate.
Atomic conversion releases unused input to its original asset and custody.
Separately committed conversion remains committed when a later funding operation fails.

Allocation residuals remain distinct from conversion rounding and resource-price arithmetic.
Replay must use the captured canonical cohort and applicable cursor rules.
It must not use the current cursor to recalculate an earlier allocation or charge separate rounded bills per wallet.
Overflow, narrowing failure, or incompatible dimensions reject the candidate without partial economic publication.

## Prepaid rights and retained liabilities

Compatible prepaid acquisition keeps its captured terms across later use and ownership changes.
An accepted old acquisition does not become unpaid when a current price changes.
New uncovered obligations still require their applicable schedule, funding proof, and consent.
Incompatible rights need an authorized supported transition or rejection, not silent reinterpretation.

Preserve consumed allowance, available rights, captured obligations, and original refund sources across restart and replay.
A wallet top-up changes backing but cannot amend an existing certificate or recreate consumed authorization.
An ownership transfer must not reset a grant merely because the new owner list resembles an earlier list.
The [persistent-liability contract](persistence-and-storage-liability.md) supplies the separate installation, firing, and release boundaries.

## Restart, failure, and retained evidence

Retained evidence must reconstruct one consistent application and economic result through the existing accepted block commitments.
That result includes consumed resources, new acquisition, debits, fee, refunds, allowance changes, receipts, and required cursor transitions.
Neither a producer's unsigned summary nor matching aggregate cost proves that these components agree.

After restart, discard unpublished provisional candidate state rather than treating it as a committed charge.
Recover published obligations and their identity through the applicable retained state and existing replay rules.
Duplicate delivery cannot charge or refund the same obligation twice.
Reconstruction of stored state must not become a new billable process installation.

Failure before complete publication must leave no partial economic result.
If restoration fails, stop that execution path in fail-closed recovery.
Do not continue with guessed balances or classify a platform failure as a chargeable user error.
The [approved failure precedence](economic-failure-policy-decisions.md#recommended-failure-matrix) applies after replay or late settlement failures too.

Local cache contents, process scheduling, wall-clock time, and reclamation timing must not alter accepted prices or refunds.
This requirement does not authorize changes to Casper pruning or storage retention.
Replay must have the required authenticated data. Missing local data is not evidence that historical consent was invalid.

## Current native support and its limits

The current [Casper support predicate](../../../../casper/src/rust/casper.rs) accepts only `CURRENT_CASPER_PROTOCOL_VERSION`, currently version six.
The presence of older version constants does not establish executable support for those histories.
The restored-controls compatibility inventory must expose this boundary rather than silently declare every older format supported.
Any required consensus compatibility change remains subject to upstream review.

[`FundingCertificate::verify_with_allocation`](../../../../rholang/src/rust/interpreter/accounting/authority.rs) checks protocol, program, and supplied pre-state identity.
It also compares the byte schedule with the compiled current byte-schedule version and digest.
The witness verification path compares its schedule and certificate identity with that certificate.
These checks protect the present schedule, but do not implement general historical economic-schedule dispatch.

The [replay mutation tests](../../../../casper/src/rust/util/rholang/replay_evidence_tests.rs) include byte-schedule version and digest mutations in certificates and witnesses.
Those tests establish payload-binding requirements within their tested paths.
They do not establish cold execution of every historical schedule or the restored signed-price lifecycle.
The [replay runtime](../../../../casper/src/rust/rholang/replay_runtime.rs) supplies existing certificate, witness, and rollback integration boundaries.
The restored contract must refine those boundaries without inventing different Casper branch semantics.

## Verification requirements

| Invariant | Required model and test cases |
| --- | --- |
| Version identity is complete | Vary each compatibility dimension independently. Reject unsupported mixed combinations, even when a decoder accepts them. |
| Exact consent survives context changes | Use equal prices with different schedule identities and lower prices with different weights. Reject unauthorized compatibility. |
| Stale candidates cannot publish | Prepare two workers against shared grants or backing. Interleave transfer, top-up, publication, and rejection. |
| Unrelated changes do not serialize funding | Interleave independent purses and grants. Detect a global freshness check that rejects otherwise valid local dependencies. |
| Accepted terms remain historical | Change current owner terms after acceptance. Replay the earlier effect with its captured price, source, and schedule. |
| Prices and units remain exact | Exercise maximum values, intermediate overflow, asset scales, fee units, and version-specific byte measurements. |
| Quote and residual rules remain captured | Change current quotes and cursors. Preserve original exact-output amounts, integer residual allocation, and input refunds. |
| Restart preserves one economic result | Interrupt before and after each publication boundary. Reconstruct accepted state without duplicated debit, release, or allowance. |
| Prepaid rights remain backed | Replay acquisition, transfer, consumption, failure, and release under compatible historical terms. Detect double acquisition and unsupported migration. |
| Independent validation agrees | Re-execute the same accepted fixture with different cache state, local clocks, worker interleavings, and current owner views. Compare complete economic effects. |

Every executable manifest entry needs example-based and property-based tests through the actual dispatch path.
Use independently specified expected results, not two calls to the same current-price helper.
Preserve fixed signing vectors and mutation controls for altered schedule and captured-context fields.

Rocq history proofs must retain schedule and consent identity across mixed operations.
TLA+ must distinguish preparation, publication, restart, and replay with competing workers and independent validator state.
A local atomic-ledger model alone does not prove distributed replay correspondence.
Loom must exercise actual shared-memory boundaries, not only a separate idealized ledger.
Report finite model bounds and unproved assumptions explicitly.
This specification is not evidence that the required historical execution paths or verification runs are complete.
