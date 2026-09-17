# Conversion and withdrawal authority

## Purpose

Conversion and withdrawal require authority for the exact assets, quantities, destinations, and operations involved.
Process funding permission does not grant unrestricted access to the backing wallet.
This contract defines those requirements for individual, joint, threshold, delegated, and capability-controlled sources.
It specifies native integration obligations, not completed support for every authorization path.

The [identity contract](authority-custody-identity-contract.md) separates principal, resource authority, grant, and physical custody.
The [delegation contract](delegation-persistent-authority-contract.md) preserves inherited restrictions and consumed allowance.
The [quote contract](conversion-quotes-and-rates.md) defines exact-output pricing and provider capacity.
Authorization, sufficient backing, and a valid quote are independently necessary.

## Separate permissions

| Permission | Required scope | Does not authorize |
| --- | --- | --- |
| Execute a process | Identified operation and compatible located resources. | Withdrawal from every wallet represented in its authority. |
| Fund a process | Permitted funding scope, backing sources, quantities, prices, and exposure. | Transfer of unrelated available wallet balances. |
| Convert input | Identified input asset, quoted rule, maximum input, composition, and destinations. | A different quote, external live price, or unrelated asset debit. |
| Commit provider output | Identified output asset and provider capacity under the selected quote. | Minting absent output or reusing the same capacity twice. |
| Withdraw available value | Exact source asset, allowed quantity, and destination. | Spending backing retained for another obligation. |
| Transfer available rights | Identified available portion and authorized replacement authority. | Copying consumed allowance or rewriting existing refunds. |
| Complete a refund | Captured obligation, source asset, source custody, and exact unused amount. | A new discretionary withdrawal to the current owner's chosen destination. |

An operation can require several permissions from the same principal or from different principals.
Combining them in one signed message does not erase their separate constraints.
A valid deployment signature alone does not prove all required permissions.

## Authorization forms

| Source form | Required checks |
| --- | --- |
| Individual purse | Verify its supported principal or actual capability authority and the exact signed operation terms. |
| Joint purse | Verify the purse's actual controlling policy and every applicable consent. Treat it as distinct custody, not a sum of member balances. |
| Threshold policy | Verify canonical members, selected witnesses, threshold, and exact signed selection. Absent members supply no authority or consent. |
| Delegated grant | Verify the authenticated delegation lineage, current version, permitted operations, limits, validity, and retained restrictions. |
| Unforgeable capability | Verify actual possession and the operations exposed by that capability. A printed address or hash does not prove possession. |
| Provider-controlled output | Verify the provider's output permission, quote-use terms, asset custody, and available capacity independently of the customer's input authority. |

Do not infer individual withdrawal rights from membership in a joint policy.
Do not infer joint withdrawal permission from signatures that authorize only separate individual funding.
Threshold success must follow the actual policy, not a count of arbitrary supplied signatures.
All selected witnesses must verify under the [threshold contract](threshold-and-payer-limits.md).

The implementation must support the approved arbitrary payer domain within independent configured limits.
Binary authority syntax is not permission to truncate authorization to two owners.
Physical aliases share capacity but do not remove logical occurrences or required restrictions.
Equal monetary shares do not permit averaging price ceilings or replacing a missing authority with a richer wallet.

## Delegation, funding slots, and lollipop

A grant authorizes only its permitted operations and available allowance.
Conversion or withdrawal must be expressly within that scope.
Redelegation must not widen recipients, assets, price ceilings, exposure, expiry, or other inherited restrictions without a separately authorized amendment.
Partial delegation partitions availability rather than copying the parent's full permission.

The rho paper's funding-slot construction permits deposits by a party that knows the unforgeable slot name.
Depositing there does not give a consumer unrestricted access to the depositor's original wallet.
Available resources still need compatible authority, location, and backing when the process consumes them.
Different depositors can fund the same slot without becoming an undifferentiated source of withdrawal permission.

The authority of a capability depends on the interface actually exposed.
A deliberately broad purse capability can carry broad powers.
A narrow funding grant must not be implemented by leaking that broader purse capability to its delegate.
The wrapper must enforce its own limits and permitted operations before it invokes underlying custody authority.

Lollipop assigns rendezvous authority to its source and continuation authority to its destination.
It does not itself convert assets, replenish allowances, or grant unrestricted withdrawal permission.
The continuation must satisfy its own resource and funding obligations.
Any additional transfer or conversion requires its corresponding explicit authorization.
This contract adds no new linear operators.

## Complete operation checks

1. Validate the supported operation format and bounded canonical input representation.
2. Resolve the exact resource, grant, asset, network, and custody identities.
3. Establish every required authority and its current applicable restrictions.
4. Verify that signed terms cover the operation and its permitted funding domain.
5. Check quote terms, expiry, resource limits, price consent, allowance, and source exposure where applicable.
6. Aggregate obligations against shared physical backing without erasing logical authority.
7. Prove that the complete operation has sufficient input and provider output capacity.
8. Bind the selected plan and captured terms to authenticated causal evidence.
9. Publish authorized effects and their receipt through the existing native checkpoint boundary.

Address derivation belongs to identity resolution, not authorization.
Finding an existing purse or observing its balance does not establish a right to debit it.
The planner cannot use a protocol-internal capability to supply missing user consent.
Internal settlement authority must apply only the independently checked user and resource proof.

Conversion must not create an unbacked circular funding plan.
Input and provider resources must be available under a valid executable transition, not merely balanced after speculative outputs appear.
The conservation proof must account for physical aliases, retained obligations, and explicit authorized issuance separately.

## Withdrawal and refund boundaries

Withdrawal can transfer only available value that its authority actually controls.
Existing resource backing and accepted obligations must remain solvent after the withdrawal.
General-balance and validator-fuel roles remain distinct even when they share a displayed address.
A general wallet permission does not authorize protocol mint, burn, quarantine, or validator-fuel disposition.

A refund completes an existing captured obligation.
It follows its original source and asset under the chosen conversion composition.
Replacement owners cannot redirect it through a new withdrawal instruction.
Do not require a new owner's permission to redefine already authorized refund terms.

Available rights can transfer under current authority while previous obligations retain their original capture.
New draws then use replacement terms and any inherited restrictions.
Transferred rights do not reset consumed allowance or permit stale signed operations when an old owner returns.
There is no fixed lifetime transfer-count limit, although each operation has representation and resource limits.

## Concurrent publication and rejection

Two valid permissions can compete for the same physical balance.
Independent signature verification is not proof that both operations can commit together.
Check aggregate capacity and relevant authority versions at the existing native publication boundary.
Disjoint custody and independent grants must retain concurrency.

Preparation must not create spendable intermediate assets or persistent per-deploy escrow.
Abort must discard provisional effects without restoring a stale snapshot over another committed operation.
Duplicate delivery must not repeat conversion, withdrawal, allowance consumption, or refund closure.
Historical replay uses captured accepted authority rather than the current owner configuration.

Missing authority, stale permission, expired new use, or insufficient backing must reject without partial candidate effects.
An unsupported joint or delegated path must reject explicitly rather than use deployment-envelope or system authority as fallback.
Classified user failures and platform failures follow the [approved economic policy](economic-failure-policy-decisions.md).
None of these requirements adds a new Casper voting, branch-selection, or pruning rule.

## Native evidence and limitations

[`SystemVault.rho`](../../../../casper/src/main/resources/SystemVault.rho) creates authorization keys from authenticated deployer identity or an unforgeable name.
Its `_transferTemplate` checks an authorization key against the vault instance and source address before the transfer path proceeds.
This is an existing capability boundary, not a complete implementation of priced conversion or arbitrary delegated grant terms.

`SystemVault.applyCost` separately checks an internal system authorization token.
That internal permission does not establish user consent by itself.
The upstream native proof and settlement path must provide the correct source, bounds, and authorized effect.
Do not expose internal settlement authority as a general wallet API.

[`vault_payer`](../../../../casper/src/rust/util/rholang/costacc/vault_payer.rs) derives a logical lane, address, and physical custody key from canonical authority.
It rejects unit authority, but successful address derivation is not a withdrawal proof.
The complete verifier must establish permissions independently before it uses the returned balance.

`ThresholdEnvelopeAuthority.v` models policy membership, selected witnesses, and authority projection with abstract principals.
The allowance and consent-history models separately cover quantities and captured terms.
Those models do not themselves verify cryptographic capabilities, complete grant restrictions, or real SystemVault calls.
Their native composition remains an explicit implementation and verification requirement.

## Verification requirements

| Invariant | Required example, property, or negative control |
| --- | --- |
| Identity is not permission | Derive valid addresses and query balances without obtaining their authority. Reject attempted debits. |
| Joint policy controls joint custody | Vary individual, all-of, threshold, and delegated inputs. Detect unauthorized individual access to joint funds. |
| Selected witnesses are exact | Include absent, invalid, duplicated, cross-scheme, and surplus selected witnesses. Never silently drop a required constraint. |
| Grants cannot widen authority | Redelegate while changing operation, destination, asset, price, expiry, or exposure. Reject unauthorized expansion. |
| Funding capability is not full withdrawal | Give a delegate a narrow funded grant. Attempt direct withdrawal, unrelated conversion, and access to the underlying broad purse capability. |
| Provider and input permissions are independent | Present a valid quote without output authority, or valid provider authority without input consent. Reject both incomplete cases. |
| Shared capacity is not duplicated | Race aliased grants, withdrawals, and provider commitments against one purse. Preserve total available backing. |
| Transfers preserve obligations | Generate long mixed histories with partial transfers, owner returns, top-ups, settlement, and original-source refunds. |
| Internal authority cannot replace user consent | Attempt otherwise invalid funding through protocol-only settlement, mint, or custody-role paths. |
| Publication and replay agree | Inject failures and duplicate delivery across prepare, apply, checkpoint, restart, and independent replay. |

Rocq proofs must preserve authorization scope, allowance conservation, and captured obligations across arbitrary finite mixed histories.
TLA+ must model competing actors and shared custody without assuming one serial validator.
Native property tests must drive actual policy, capability, and custody paths with independent permission and balance oracles.
Loom must cover native synchronization, followed by cross-deploy and independent-validator integration tests.
Record finite bounds and abstract inputs explicitly. Helper proofs alone do not establish complete authorization enforcement.
