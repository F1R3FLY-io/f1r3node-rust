# Threshold authorization and payer limits

## Purpose

Threshold authorization decides whether a signed deployment has sufficient authenticated authority.
Funding decides whether permitted resources and purses can satisfy the deployment's obligations.
Neither decision replaces the other.

This contract extends the [signed phlo contract](signed-phlo-contract-proposal.md) and [identity contract](authority-custody-identity-contract.md).
It preserves the [deployment envelope](deploy-envelope-v6-1.md), selected allocation policy, and historical replay rules.
It does not change Casper voting or add planned linear operators.

## Distinct counts

| Count | Meaning | Required bound |
| --- | --- | --- |
| Policy members | All principals listed in the signed authorization policy, including absent members. | The active member limit bounds decoding, canonicalization, and witness validation work. |
| Selected signers | Members selected by the committed presence bitmap with valid witnesses. | The count cannot exceed the policy-member count. Every selected witness must verify. |
| Logical occurrences | Required authority or resource occurrences across located execution regions. | Separate structural and work limits preserve multiplicity without permitting unbounded processing. |
| Physical payers | Distinct eligible custody entries after authenticated alias resolution. | A separate payer limit bounds the monetary cohort, including joint purses. |
| Presentation size | Encoded policy, witnesses, capabilities, and funding evidence. | Independent byte and work limits apply even when the physical-payer count is small. |

The planned initial member limit is 64, as recorded in the signed phlo contract.
This is not a two-owner semantic limit or a claim that every current decoder enforces 64.
Physical-payer limits remain separately configured. A member limit does not determine how many authorized joint purses exist.
All validators must derive consensus-relevant limits from the same activated rules and captured context.
Local transport limits must not silently redefine accepted historical block validity.

## Canonical threshold semantics

Let $`M`$ be the ordered policy-member list, $`N=|M|`$, and $`S`$ the selected member list.
The signed presence bitmap determines $`S`$. An allocator must not substitute a cheaper signer subset.

The active envelope permits these policy forms:

- `AllOf`: $`N>0`$ and every member is selected.
- `Threshold(k)`: $`1\le k<N`$ and $`|S|\ge k`$.

An all-members threshold uses the canonical `AllOf` wire form.
A singleton therefore uses `AllOf`, not `Threshold(1,1)`.
Construction helpers can accept a scalar threshold equal to the member count and encode `AllOf`.
This helper behavior does not authorize a noncanonical threshold wire form.

Members must have canonical principal order and no duplicate principal or duplicate ground authority.
Two verification schemes for the same ground key cannot fill two independent policy slots.
Duplicate-ground checks must cover the complete member list, not only adjacent principals in scheme order.

The bitmap must have exactly $`\lceil N/8\rceil`$ bytes and zero unused high bits.
Witness indices must exactly match selected bits in canonical order.
Missing, extra, duplicate, empty, or incorrectly indexed witnesses reject.
Every present signature must verify under its declared scheme and complete signed context.
A valid quorum does not excuse an invalid extra signature.

Changing the member list, threshold, selected subset, or signed context requires new valid signatures.
An absent member remains part of the committed policy but supplies no authenticated witness.

## From authorization to funding

Only verified selected signers contribute signer-derived funding authority.
An absent member's public key, purse balance, or placeholder position contributes no capacity and receives no debit.
Independent capability-backed funding remains subject to its own authenticated permission, backing, and captured consent.
Threshold success does not authorize access to arbitrary joint or component purses.

For a two-of-three policy with exactly two selected signers, only those two supply signer-derived authority.
If all three sign, all three belong to the selected authority projection.
The implementation must handle that projection without requiring an intermediate binary compound purse.
It must not truncate the selected set to two or silently choose exactly two of the three signers.

Compound authorization preserves every required logical occurrence.
Physical alias resolution gives each eligible physical purse one monetary allocation position.
The [allocation policy](authority-allocation-policy-decisions.md) selects contributions over complete feasible assignments.
Threshold cardinality does not define monetary weights, duplicate resource demand, or multiply the deployment limit.
Each required payer's applicable price ceiling and permission must hold for that payer's assigned funding.

Owner transfer does not impose a fixed lifetime count of owners.
Each accepted state and execution must satisfy its active size limits and captured allowance constraints.
Arbitrary finite transfer histories remain subject to the [delegation contract](delegation-persistent-authority-contract.md).

## Boundary and failure behavior

Let $`C_m>0`$ be the member limit and $`C_p>0`$ the physical-payer limit.
Validate declared lengths and independent byte bounds before allocating proportional work where the transport permits it.
Use checked integer conversions for lengths, counts, indices, and bitmap arithmetic.

| Input | Required result |
| --- | --- |
| Zero policy members | Reject the authorization policy. A zero resource obligation does not authorize an unsigned deployment. |
| One through $`C_m`$ members | Apply complete canonical, signature, threshold, and funding checks. Count alone is not acceptance. |
| $`C_m+1`$ members, even with a small selected subset | Reject under the active member-limit rule. Absent members still require validation work. |
| Zero eligible physical payers with positive new funding | Reject the funding plan without wallet fallback. |
| Zero newly required funding with compatible prepaid backing | Do not invent a new payer or contribution. Verify backing and preserve the separate deployment-fee obligation. |
| One through $`C_p`$ physical payers | Retain complete authority and eligibility checks, then run the approved allocation rule. |
| $`C_p+1`$ physical payers | Reject under the active payer-limit rule. Do not drop a purse to fit the limit. |
| Many logical aliases of one purse | Count one physical payer but enforce independent presentation, occurrence, and work limits. Aggregate all physical exposure. |
| Host search or resource exhaustion | Report the applicable resource failure. Do not claim a proof of insufficient funds or retain partial debits. |

Zero-new-funding behavior does not mean the current monetary-cohort constructor accepts an empty input.
The current constructor rejects an empty eligible map and requires a nonzero configured payer cap.
The composed planner must distinguish an unnecessary allocation from an attempted unfunded allocation.
It must not bypass authentication, resource consumption, or an outstanding fee.

Local refusal, consensus-rule rejection, and proven funding infeasibility must remain distinct.
Rejected admission must not publish partial reservations, balances, receipts, allowances, or cursor changes.
Historical replay must apply the original supported rules rather than the current deployment limits.

## Existing implementation and proof boundaries

The [crypto implementation](../../../../crypto/src/rust/signatures/signed.rs) canonicalizes envelope signers and verifies all nonempty selected signatures.
Its `canonical_envelope_signers` checks duplicate ground keys across the complete list.
The [wire decoder](../../../../models/src/rust/casper/protocol/casper_message.rs) checks canonical policy form, bitmap dimensions, and exact witness indices.
These paths do not, by themselves, establish the planned end-to-end member-limit enforcement.

The [monetary cohort](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/cohort.rs) bounds distinct physical custody entries after alias resolution.
That bound does not replace a policy-member or encoded-presentation bound.

The [threshold model](../../../../formal/rocq/cost_accounted_rho/theories/ThresholdEnvelopeAuthority.v) separates policy membership, presence, witnesses, and ground authority.
Its valid-authorization predicate requires a well-formed policy, matching presence length, exact witness selection, and sufficient quorum.
Its witnesses are abstract principals. They do not implement signature verification or byte parsing.
`bounded_authorization` adds a positive policy-member cap without changing the underlying quorum predicate.
`accepted_funding_arity_bounds` bounds the selected ground-authority count by that cap.
`member_cap_checks_absent_members` rejects excess policy members even when few members sign.
These theorems do not establish native decoding limits or bound distinct capability-funded physical purses.

`canonical_authorization_construction` constructs exact witnesses and presence bits for a fixed, previously canonicalized policy list.
The selected input must contain distinct policy members and satisfy the policy's quorum.
The reconstruction helper is not a malformed-input validator.
Its filter removes duplicate and out-of-policy selected entries if the caller omits these precondition checks.
Native validation must reject those inputs before reconstruction, not silently repair an invalid signed presentation.
`canonical_construction_ignores_selected_input_order` proves that permutations of that selected input produce the same constructed authorization.
This is not permission to reorder a signed policy or its bitmap independently.
The model preserves the supplied policy order. Native refinement must establish that this order matches the required principal-byte order.

`every_positive_arity_has_allof_authorization` constructs an authorization for every positive finite arity within a supplied cap.
`accepted_funding_has_no_duplicate_ground` proves that accepted funding cannot duplicate a ground owner through another signature scheme.
`paired_input_permutation_preserves_funding` preserves the funding multiset when each presence value moves with its principal.
The negative example `presence_must_move_with_its_principal` shows why moving principals without their presence values changes authority.
Native refinement must establish authentication, encoding, and custody boundaries before composing these results with funding conservation proofs.

## Required verification

| Invariant | Required generated cases and negative controls |
| --- | --- |
| Cardinality is general | Cover zero, one, two, three, the active cap, and cap-plus-one. Include independently varied physical and logical counts. |
| Threshold selection is exact | Vary every subset for bounded small policies. Generate larger subsets, including more selected signers than the minimum. |
| Placeholders have no funding authority | Give absent members large balances. Require unchanged eligibility, contributions, and refunds for the selected deployment. |
| Signatures bind selection | Mutate policy, threshold, bitmap, witness index, scheme, or signed scope. Reject even when the remaining witnesses meet quorum. |
| Canonical identity prevents duplicate slots | Generate duplicate principals and nonadjacent duplicate ground keys across schemes. |
| Binary grouping does not limit arity | Generate equivalent compound shapes with three or more verified signers and no intermediate compound purse. |
| Physical capacity is not multiplied | Generate joint purses, repeated lanes, aliases, zero balances, and independent occurrence counts. |
| Limits do not change authority | Vary member and payer caps independently. Reject excess inputs without truncation or partial effects. |
| Replay retains its rules | Replay supported historical fixtures after current limit, owner, and price changes. Compare complete captured identities and effects. |
| Parallel candidates preserve capacity | Interleave shared-custody reservations, top-ups, transfer, rejection, and settlement. Preserve independent-custody concurrency. |

Extract native properties from the formal invariants, with independent reference selection and capacity calculations.
Use exhaustive subset enumeration only within declared bounds. Do not describe finite generation as a proof for every policy size.
Use Loom for actual shared-memory publication and integration tests for independent validators and durable replay.
Keep expected-failing controls for signer truncation, absent-member funding, skipped invalid witnesses, alias overdraw, and cap bypass.
Completion requires signed admission, resource planning, metering, settlement, and replay evidence, not only helper-level tests.
