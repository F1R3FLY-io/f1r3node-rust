# Delegation and persistent authority contract

## Scope and evidence status

This contract specifies persistent delegation under the [ratified policies](economic-activation-policy-ratification.md).
It specifies obligations for delegation, expiration, replay protection, lollipop, persistent allowances, conversion, and withdrawal.
It does not claim completed native integration or new formal proofs.

This contract governs current funding and delegation behavior and records compatibility obligations for future operator integration.
Additional planned linear operators are outside the current implementation scope.
Their compatibility obligations do not establish completed operator support.
Existing cost-accounting repairs, including the required lollipop funding behavior, remain distinct from adding new operators.
Classify mixed implementation tasks before execution so future operator work does not become an implicit release requirement.

The [ownership proof record](ownership-transfer-consent.md) covers abstract consent histories.
The [allowance proof record](persistent-funding-allowance.md) covers quantitative conservation and local publication models.
Their stated limitations remain applicable.
In particular, abstract authorization booleans do not prove native signature or capability verification.

## Specification basis

The [rho paper](../../../../../publications/cost-accounting/cost-accounted-rho.tex) defines token-metered delegation under “Programmable tokenized capabilities.”
A parent can give a subordinate a portion of its resource stack.
The delegated stack limits the subordinate's funded actions.
Delegation does not duplicate the parent's resources.

Definition `def:sugar-lollipop` assigns rendezvous funding to the source authority and continuation funding to the destination authority.
Its composability remark permits compound authorities and chains of transfers.
Definition `def:funding-slot` permits funding through an unforgeable channel rather than a fixed depositor identity.

Rule `eq:R1` in the [continued-GSLT paper](../../../../../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex) consumes a compatible cell from a located purse.
The remaining purse retains its location.
Equal monetary value alone cannot replace the required authority, location, or resource compatibility.

## Distinct authority domains

| Domain | What it permits | What it does not imply |
| --- | --- | --- |
| Process authority | Execute the identified operation with compatible resource evidence. | Unrestricted access to an owner's wallet. |
| Funding allowance | Draw within captured quantitative and economic consent. | Ownership of every asset in the backing purse. |
| Capability transfer | Move the identified available right under current authority. | Transfer consumed authorization or rewrite existing refund destinations. |
| Conversion authority | Perform the selected signed conversion composition. | Withdraw unrelated balances or change the resource schedule. |
| Withdrawal authority | Transfer available value to an authorized destination. | Consume value already committed to another obligation. |

The native verifier must identify the applicable authority for each operation.
A valid deployment signature alone does not supply all five permissions.
An eligible threshold presentation supplies only the principals and rights that the verified policy actually authorizes.

## Grant identity and captured terms

A **grant** is an identified persistent funding permission, not another monetary balance.
Its accounting identity survives authorized owner changes.
Each current grant version binds its issuer authority, delegate authority, permitted operations, resource units, limits, price consent, and applicable schedule.
Bind backing custody, asset identity, validity interval, and delegation restrictions where applicable.
Bind conversion and withdrawal permissions separately when the grant includes them.

The signed representation must distinguish omitted consent from explicitly unbounded consent where that form is supported.
Missing consent must not become unlimited permission.
Use canonical representations and authenticated causal context for identity and version checks.
The wire contract must specify exact bytes and domain separation before implementation acceptance.

Delegation must attenuate every inherited restriction unless the authority that controls that restriction explicitly authorizes an amendment.
The child's permitted operations must remain within the parent's delegable operations.
Its usable validity interval must remain within every applicable inherited interval.
Its resource and asset domain must remain compatible with inherited rights.
Its quantitative allocation and temporary exposure must fit the portion explicitly assigned to it.
Each inherited price ceiling remains applicable unless its controlling authority authorizes replacement.

A delegate cannot treat authority to redelegate as authority to relax these restrictions.
Changing owners does not prove that every controlling authority consented to new economic terms.
Authenticate each required amendment separately from possession of the transferred capability.

Existing reservations capture the grant version and terms that authorized them.
Later transfers, expiry, and term changes cannot replace those terms or redirect their refunds.
New draws must use current authority and current applicable terms.
Changing current authority must not invalidate a completed historical effect during replay.

## Quantitative conservation

For one unsplit allowance, let $`Q`$ denote issued authorization, $`A`$ available authorization, $`R`$ reserved authorization, and $`C`$ consumed authorization.
All four quantities use the same compatible unit.

```math
A + R + C = Q.
```

Wallet top-ups change backing, not $`Q`$.
Authorized expansion changes issued and available authorization by the same amount.
Execution consumes the applicable deployment limit and persistent allowance independently.
Neither limit establishes resource sufficiency without the required proof and physical backing.

Delegated subdivisions must partition available authorization, not create independent copies of its total.
Each subdivision retains a link to the original accounting lineage and any parent restrictions.
Internal delegation links represent routing, not additional issued authorization.
Across a delegation tree, count each available, reserved, or consumed unit exactly once.
Only explicitly authorized new issuance can increase the tree's total authorization.

For a quantitative projection, let $`G`$ contain each distinct accounting fragment exactly once.
Let $`A_g,R_g,C_g`$ denote a fragment's available, reserved, and consumed authorization.
Let $`Q_{\mathrm{root}}`$ denote the lineage's original authorization plus explicitly approved expansions.
The projection must satisfy:

```math
\sum_{g\in G}(A_g+R_g+C_g)=Q_{\mathrm{root}}.
```

The index set excludes internal routing links and duplicate references to the same fragment.
Different units or incompatible resources require separate conservation equations.
This equation describes authorization, not an additional token balance or a substitute for physical backing conservation.

Splitting available quantity $`x`$ creates a fresh child fragment and removes exactly $`x`$ from the parent's availability.
The child starts with available quantity $`x`$, no reserved quantity, and no consumed quantity.
The parent's reserved and consumed quantities remain unchanged.
The operation must publish the removal and creation together.
The consumed history remains in the same lineage even if a live fragment is later compacted.

For example, a parent has seven available units and three consumed units from ten authorized units.
Delegating four available units leaves three available units with the parent and four with the child.
The lineage still contains ten units, including its three consumed units.
Creating a child with four units while retaining seven available parent units violates conservation.

Native representation must support this conservation law without storing every superseded owner in live grant metadata.
Historical evidence and live authorization state have separate retention requirements.
This contract does not introduce a persistent deployment escrow table or a global reservation map.

## Operation contract

| Operation | Preconditions | Retained effect |
| --- | --- | --- |
| Delegate available rights | Current authority permits delegation. The proposed portion is available and satisfies retained restrictions. | Transfer that portion or partition its authorization without duplicating backing or allowance. |
| Transfer a whole grant | Current grant authority permits the named new owners and terms. | Preserve accounting identity and stocks. Change authority for future draws only. |
| Transfer a partial right | Identify the exact available portion and destination. | Move only that portion. Leave reserved obligations and consumed history attached to their original accounting identities. |
| Reserve funding | Verify current applicable authority, price consent, execution limit, allowance, resource sufficiency, and shared physical capacity. | Capture terms and source exposure within the native candidate lifecycle. |
| Settle | Match the captured obligation and authenticated realized effect. | Consume authorized amounts once and release unused backing to captured destinations. |
| Abort a candidate | No retained billable effect is authorized, or the ratified failure policy requires rollback. | Discard provisional effects without overwriting another committed operation. |
| Expand an allowance | Current authority explicitly permits the new quantity and terms. | Increase issued and available authorization without resetting consumption. |
| Top up a purse | The incoming asset transfer is authorized. | Increase backing without changing grant permission, price consent, or an existing reservation. |

Grant transfer does not require an arbitrary lifetime transfer-count limit.
Per-operation signer, representation, resource, and funding limits still apply.
Machine arithmetic must reject overflow without changing state or reusing old authorization.
Finite formal-model bounds must not become production transfer-count limits.

## Lollipop and activation

The compiler's [lollipop translation](../../../../rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/desugar.rs) separates source and continuation authority.
The source pays only the obligations attributed to its rendezvous scope.
The continuation uses its destination authority and applicable captured funding terms.
The allocator must not substitute deployment-envelope funding for a missing destination permission.

Lollipop alone does not transfer all source wallet assets, create destination withdrawal permission, or replenish either allowance.
An explicit asset transfer or grant operation must authorize those additional effects.
Nested transfers repeat the same rule for each activation without resetting earlier consumption.

For persistent continuations, each activation must identify its deployment execution scope and persistent grant obligations.
The sufficiency certificate must cover all obligations inside the selected atomic operation, including any continuation covered by that operation.
Unfunded local interactions outside such a certificate follow the paper's blocking semantics.
A certified operation cannot silently continue after exhausting its proved sufficient bound.

An unforgeable funding-slot name can identify a resource capability without fixing the depositor's identity.
Depositing resources does not itself create unrestricted withdrawal permission over their original backing purse.
Validate the available located resources and their provenance when drawing against the slot.
Cross-deploy persistence must preserve these checks, not reuse an earlier deployment's limit as unlimited future consent.

## Expiration and replay protection

Expiration governs new authorized actions, not the existence of already accepted historical effects.
Use authenticated execution time for consensus-visible validity checks.
Node wall-clock checks may filter ingress, but cannot replace historical validation context.

The current [deployment helper](../../../../models/src/rust/casper/protocol/casper_message.rs) treats a timestamp equal to expiry as valid.
The current [validator](../../../../casper/src/rust/validate.rs) supplies the block timestamp to that helper.
Preserve this historical boundary for existing envelopes.
For new grants, use inclusive validity endpoints in authenticated execution time, consistent with the existing expiry boundary.
Reject inverted intervals and invalid timestamp representations before mutation.
Encode each endpoint and its presence explicitly in the signed contract.
Do not infer a grant lifetime from the deployment's expiration field.

Expiration must prevent new draws under expired permission.
It must not erase captured obligations, prevent authorized refund completion, or resurrect previously consumed allowance.
Test expiration before preparation, between preparation and publication, after accepted execution, and during later historical replay.

Preparation is not acceptance.
A candidate must validate new draws against its authenticated execution context before publication.
Rebuilding a candidate under a different context requires new validity checks.
Historical replay reuses the original accepted context instead of treating replay time as a new funding activation.
Delayed settlement of captured accepted obligations does not create permission for an additional draw.

An operation identity must bind the exact operation, grant identity, expected authority version, and applicable network context.
Returning to an earlier owner set must not make an old authorization usable again.
Duplicated requests and competing candidates must not publish the same economic obligation twice.
Replay protection must use authenticated causal identities and existing conflict rules, not a new global sequence shared by validators.

Distinguish the grant's authority version from an individual operation identity.
The authority version identifies consent that can cover multiple expressly permitted draws.
The operation identity distinguishes those draws and binds each one to its execution scope.
Reusing a valid grant version does not authorize repeating an already retained operation.
Advancing authority must prevent old signed amendments from becoming valid again when owners return.

The native identity representation must prevent wraparound and ambiguous canonical encoding.
It must not depend solely on an ever-growing list of closed reservation positions from an abstract model.
The wire and lifecycle contracts must establish replay-safe retention before any identity record is reclaimed.

## Conversion and withdrawal

Delegated conversion requires authority for the input debit and the selected conversion terms.
The provider must authorize the output commitment and have sufficient available capacity.
The grant's restrictions still apply after conversion.
Different market quotes do not authorize different consensus resource tariffs.

Atomic funding preserves original-input release and captured quote terms under the ratified economic policy.
A separately committed trade keeps its separate failure boundary.
Neither composition may bypass a grant's allowance, exposure consent, or the complete feasible allocation domain.

Withdrawal requires current authority for the exact asset, available amount, and destination.
Available backing excludes obligations retained by the applicable state.
Joint and threshold withdrawals must satisfy their actual authority policies, not merely count supplied signature fields.
Refund completion follows captured provenance rather than a fresh withdrawal instruction from replacement owners.

## Formal refinement and executable obligations

| Invariant | Required negative control or regression |
| --- | --- |
| Delegation preserves total authorization | Copy an available parent portion to a child without removing or partitioning the parent's permission. |
| Delegation preserves inherited restrictions | Extend expiry, increase a price ceiling, enable withdrawal, or change assets using redelegation authority alone. |
| Consumption survives owner changes | Transfer repeatedly, including return to old owners, then attempt to reuse consumed allowance. |
| Reservations preserve captured consent | Replace owners, price terms, custody, or expiry before settling an existing obligation. |
| Source and continuation remain distinct | Omit destination funding and attempt to charge the source or deployment envelope instead. |
| Every required owner permits the price | Use the highest owner ceiling, an average, or only one valid signature. |
| Shared backing cannot be counted twice | Delegate to multiple grants that reference the same purse and submit concurrent draws. |
| Partial transfer preserves obligations | Move a reserved portion or refund destination with an available portion. |
| Top-ups do not expand permission | Increase backing, then draw above the remaining allowance or captured exposure. |
| Expiration uses authenticated context | Change local time or current ownership during cold replay. |
| Grant consent and operation identity remain distinct | Reject a legitimate second distinct draw, or accept a duplicate draw because the grant version remains current. |
| Rejection is mutation-free | Inject nested mixed errors and failures during debit, fee, refund, and publication. |

Extend inductive proofs over arbitrary finite mixed histories, including delegation trees and partial transfers.
Keep signature verification, resource sufficiency, and native publication as explicit refinement obligations rather than successful boolean assumptions.
Model independent validators and candidates, with shared and disjoint custody, delayed delivery, retries, and restart.
Do not prove concurrency by replacing those actors with one serial worker.

Generate property tests from the invariants and compare complete state against an independent transition oracle.
Use Loom for actual shared-memory synchronization boundaries and native tests for RSpace publication and distributed replay.
Include more than two owners, nested delegations, changed owner sets, zero quantities, integer boundaries, and long generated transfer histories.
Require actual signed-envelope admission, execution, settlement, and cold replay before claiming native completion.

## Remaining implementation assignments

The [identity contract](authority-custody-identity-contract.md) specifies canonical authority and physical-custody identities.
Presentation limits and threshold handling require a separate contract.
The wire contract specifies nonce representation, validity encoding, and signed bytes.
Capability transfer must implement the transfer and activation obligations.
The conversion authority and provenance tasks implement their distinct asset permissions and refund boundaries.
These assignments retain required work. They do not claim that abstract proofs already establish the native contract.

## Requirement coverage and review boundary

| Task requirement | Contract boundary | Remaining implementation evidence |
| --- | --- | --- |
| Delegation | Inherited restriction attenuation and unique fragment conservation. | Authenticated amendments, partition atomicity, and generated delegation-tree histories. |
| Expiration | Explicit inclusive endpoints and original authenticated execution context. | Signed endpoint vectors and native expiry-boundary replay tests. |
| Nonces | Separate authority version and unique operation identity. | Canonical wire identity, stale amendment rejection, duplicate handling, and retention safety. |
| Lollipop transfer | Source rendezvous and destination continuation funding remain distinct. | Native located attribution and multi-stage transfer tests. |
| Persistent allowances | Deployment limits, lineage quantities, and physical backing remain separate constraints. | Mixed-history conservation and actual admission/settlement refinement. |
| Conversion authority | Input authority, provider commitment, captured quote, and selected composition. | Asset-provenance, exposure, and concurrent provider-capacity tests. |
| Withdrawal authority | Current permission for available assets and exact destination. | Joint-policy verification and encumbered-value rejection. |

This table maps requirements to planned evidence, not to completed verification results.
Independent contract review remains required before this task can be verified.
