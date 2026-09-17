# Conversion provenance and refunds

## Purpose

Conversion provenance identifies where funding came from, what authorized its use, and where unused funding must return.
Those identities must survive allocation, execution, ownership transfer, failure, restart, and historical replay.
This contract defines the required retained evidence and conservation boundaries.
It does not claim complete native multi-asset conversion or refund support.

The [quote contract](conversion-quotes-and-rates.md) defines exact-output pricing and conservative exposure.
The [authority contract](conversion-and-withdrawal-authority.md) separates input, provider, withdrawal, and refund permissions.
The [economic policy](economic-activation-policy-ratification.md) selects separate conversion and atomic quote-backed funding as distinct compositions.
These contracts govern the same complete funding lifecycle.

## Distinct provenance records

| Record | Required retained meaning |
| --- | --- |
| Original source | Network, asset, integer scale, custody role, and actual physical purse that supplied backing. |
| Logical obligation | Resource identity, authority, location, execution scope, and applicable allowance lineage. |
| Funding permission | Authenticated owners or grant, permitted domain, signed debit and exposure caps, and captured terms. |
| Quote use | Exact quote, provider, composition, operation identity, output amount, required input, and fee decomposition. |
| Candidate hold | Maximum source-specific exposure established for the complete certified outcome family. |
| Prepaid right | Compatible acquisition terms, available quantity, backing lineage, and current authorized ownership or location. |
| Retained result | Realized resource effects, debits, conversion, fees, refunds or releases, allowance consumption, and required cursor transitions. |

These are semantic requirements, not a demand for a second ledger or independently spendable receipt token.
The wire and native state contracts must select canonical representations with complete authenticated bindings.
A positional vector needs an immutable source mapping before its amounts can identify physical refunds.
Do not reconstruct that mapping from current owner order, current aliases, or a later quote response.

## Separate conversion before funding

A separately committed trade transfers assets under its own authorized result.
The output then becomes a source for a later independent funding operation.
That later operation captures the output asset and its actual custody as its reservation source.

If the later operation uses less funding, its unused reservation returns in that captured reservation asset.
The earlier conversion remains committed.
The later failure does not authorize a reverse exchange or a refund in the trade's original input asset.
Client consent and receipts must expose these separate failure boundaries.

For example, an authorized trade exchanges ten A units for five B units.
A later operation reserves five B units and consumes three.
Its unused two B units remain with, or return to, the captured B source under the native lifecycle.
The trade's original ten A units are not restored by that later settlement.

## Atomic quote-backed funding

Atomic funding captures original input custody and provider-output custody before publication.
For one quote use, let $`H_{\mathrm{in}}`$ and $`H_{\mathrm{out}}`$ denote their authorized holds.
Let $`y`$ be the approved realized output and $`q(y)`$ its exact quoted input, including conversion fees.
The unused amounts satisfy:

```math
R_{\mathrm{in}}=H_{\mathrm{in}}-q(y),\qquad
R_{\mathrm{out}}=H_{\mathrm{out}}-y.
```

Each subtraction requires a proved nonnegative result in that source's own asset units.
Input and output quantities are not interchangeable merely because their numeric values match.
The input decomposition must account for every transfer and conversion-fee recipient required by the captured quote.
The output decomposition must identify its approved resource acquisition, fee, or other retained funding obligation.

Only the exact quoted input for the retained output is converted.
Unused input is released to its original input asset and custody.
Unused provider capacity is released to its original provider source.
No later market rate or reverse trade participates in either release.

For example, a quote requires two A units for one B unit without conversion fees.
Holds of ten A and five B cover a maximum output of five B.
If the approved result requires three B, the operation converts six A and releases four A and two B of unused capacity.
The operation does not first convert all ten A and then trade two B back.

Holds remain native candidate or proof state, not persistent per-deploy escrow balances.
Releasing an unused hold need not create a transfer when the native implementation never published its debit.
The observable final state must nevertheless preserve the same source attribution and exact conservation.

## Per-asset conservation and physical aliases

For each original source $`i`$, let $`H_i`$ be its hold, $`D_i`$ its retained debit, and $`R_i`$ its unused remainder.
Under the captured source mapping and compatible units:

```math
D_i+R_i=H_i,\qquad 0\leq D_i\leq H_i.
```

Aggregate aliases by physical purse, asset, and custody role before checking available capacity.
Retain logical authority and allowance provenance when physical entries combine.
Different quotes or grants cannot each claim the same complete available purse balance.
General funds and validator fuel remain distinct custody roles.

For each asset, account for every actual debit, recipient credit, authorized burn, and authorized mint exactly once.
Cross-asset conversion conserves each asset through its own provider and customer transfers.
It does not assert equality between unrelated asset quantities or create the provider's output from nothing.
Resource acquisition and later consumption must not count the same monetary backing as two independent spendable balances.

Nominal totals alone are insufficient.
A plan can balance numerically while swapping recipients, misidentifying an asset, duplicating backing, or breaking a located-resource restriction.
Verification must compare typed provenance and complete effects in addition to aggregate sums.

## Prepaid rights and ownership transfers

An unused current reservation and an unused prepaid right have different meanings.
The reservation releases unused backing to its captured source.
The prepaid right remains an available right with its current authorized ownership and compatible acquisition terms.
It does not automatically become a cash refund to the historical sponsor.

A previously committed transfer of available prepaid rights must remain effective after a later operation settles.
The refund rule must not undo that committed transfer by paying the former sponsor.
Provisional transfers inside the current candidate follow its applicable failure projection.
Do not retain a provisional transfer when the selected rollback boundary rejects it.
Any redemption requires its own explicitly supported authorization and backing transition.
The [persistent-liability contract](persistence-and-storage-liability.md) defines these lifetime boundaries.

Owner changes cannot redirect an earlier reservation's refund or reset consumed allowance.
A partial transfer moves only its identified available portion.
Captured obligations and their original asset paths remain attached to their established lineage.
A wallet top-up supplies additional backing without changing those captures.

For a long-lived funding slot, retain enough provenance to distinguish deposits, acquired rights, later draws, and captured releases.
The slot's capability does not erase the underlying asset or grant restrictions.
It also does not create an unlimited obligation for every past depositor to finance future activations.

## Failure projection and recovery

| Event | Required retained result |
| --- | --- |
| Invalid consent, quote, or funding before acceptance | No candidate trade, user debit, conversion fee, or partial release. |
| Successful atomic operation | Retain the exact authorized conversion and funding result. Release each unused source amount through its capture. |
| Classified deterministic user failure | Retain only approved billable work and applicable fees. Convert only the output needed for that retained obligation. |
| Platform, certificate, or late publication failure | Publish no partial economic result. Restore the candidate pre-state or fail closed if restoration fails. |
| Separate trade followed by funding failure | Preserve the earlier committed trade. Apply the later funding operation's own captured refund rule. |
| Retry or duplicate delivery after commitment | Reconstruct the same receipt without repeating debit, conversion, refund, or allowance consumption. |
| Restart before publication | Discard provisional work without restoring a stale snapshot over committed state. |
| Restart after publication | Recover the accepted economic result and operation identity through existing state and replay commitments. |

The failure projection must distinguish resources present before execution from resources acquired, transferred, or consumed during execution.
It cannot restore a consumed prepaid right while retaining credit for the same consumption.
It must not lose unused rights or undo their previously committed ownership transfers.
Roll back provisional transfers when the applicable rollback boundary rejects those transfers.
Mixed platform or unclassified failures override user-failure charging under the approved policy.

Publication must retain one consistent application result, economic result, allowance state, and receipt.
Do not publish a refund and then discover that its corresponding debit or conversion cannot commit.
Fault injection must cover each intermediate boundary, not only the final return value.

## Replay and operation identity

The operation identity must distinguish separate permitted draws from duplicate delivery of one draw.
It must bind the accepted quote use, grant version, execution context, and captured economic evidence.
Changing a custody presentation, retry count, current owner, or current quote cannot create a fresh claim on the same accepted refund.

Historical replay uses the captured source mapping, units, pricing rule, and accepted context.
It must not ask current owners where to send an earlier refund.
Independent validators must reproduce complete economic effects, not merely the same total cost.
This requirement uses existing causal and conflict rules, not a new global ledger or finality mechanism.

Live provenance storage must not grow solely because a grant has an unbounded history of completed ownership transfers.
Historical audit evidence and live authorization data have separate retention requirements.
Any compaction must preserve identity and proof requirements under existing retention rules.
This contract does not authorize Casper pruning changes.

## Existing primitives and proof limits

[`FundingBranchReservation`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_reservation.rs) computes per-source holds and checked unused amounts for supplied branch plans.
Its source vectors do not themselves authenticate assets, quote identities, or a multi-asset custody mapping.
Its refund tests therefore establish arithmetic for the supplied mapping, not the full conversion provenance contract.

`CanonicalCustodyAliasing.v` proves original-custody refund conservation under a supplied fixed lane-to-purse function and bounded actual draws.
It does not establish native asset authentication, quote semantics, or concurrent atomic publication.
The native integration must preserve that function's meaning through actual custody resolution and retained evidence.

[`ApplyCostDeploy`](../../../../casper/src/rust/util/rholang/costacc/vault_cost_deploy.rs) matches native allocations and settlements by address and custody role.
It checks that burn plus fee fits the corresponding allocation.
Those current native fields do not encode the complete multi-asset quote and acquisition provenance required here.
Extending the integration must preserve those safety checks without interpreting address equality as arbitrary asset equivalence.

## Verification requirements

| Invariant | Required model, property, and integration evidence |
| --- | --- |
| Each remainder belongs to its original source | Permute owner lists, aliases, source vectors, assets, and quote uses. Reject mismatched mappings despite equal totals. |
| Atomic input is not reverse-traded | Change the current rate after capture. Preserve exact original-input release under the captured quote. |
| Separate trades remain separate | Fail the later funded operation at each boundary. Retain the earlier trade and refund only the later reservation asset. |
| Committed prepaid transfers survive later settlement | Commit a transfer, then consume or release through a later operation. Preserve the committed owner and approved resource consumption. |
| Provisional transfers follow failure projection | Transfer inside the candidate, then inject user, platform, mixed, and late failures. Preserve rights without retaining rejected transfers. |
| All physical backing is counted once | Share input and provider purses across concurrent candidates and aliases. Detect duplicate holds and unbacked circular funding. |
| Closing an operation is idempotent | Race settlement, abort, duplicate delivery, and restart. Preserve one accepted debit and one exact release result. |
| Failure projection is complete | Vary preexisting, newly acquired, transferred, and consumed resources under user, platform, mixed, and late failures. |
| Arithmetic is typed and exact | Cover zero, maximum quantities, overflow, fee boundaries, distinct scales, and incompatible assets. |
| Replay preserves complete effects | Compare independent validators and cold replay with different current owners, quote caches, and local reclamation histories. |

Rocq proofs must compose per-source arithmetic with authenticated asset provenance and backing conservation across mixed histories.
TLA+ models must include concurrent quote uses, shared custody, publication, release, failure, and restart.
Property tests need independent per-asset balance, resource, and allowance oracles.
Loom must exercise actual native synchronization, with integration tests for retained RSpace and SystemVault effects.
Bounded checks must state their limits. No isolated refund equation proves this complete lifecycle.
