# Activation and migration policy decisions

## Status and scope

This record defines activation and migration policy. Fresh-genesis release scope
and the Route A channel-key rule below have received explicit user approval;
their implementation and release verification remain separate obligations.
Other proposed economic and migration details retain their own approval gates.
This record selects no activation date, block height, new version number, or
operational deployment.

The [ratified activation scope](economic-activation-policy-ratification.md) selects fresh genesis for this release.
The [signed-envelope contract](signed-phlo-deploy-envelope.md) defines the subsequent integration of dev's offered-price semantics.
That contract supersedes this proposal's fresh-tag recommendation for `phloPrice` and `phloLimit`.
The offered funded format signs these fields at tags 7 and 8, separately from owner ceilings.

The [allocation ratification](authority-allocation-policy-ratification.md) remains authoritative for funding allocation.
The [economic proposal](economic-failure-policy-decisions.md) separately requires approval for failure charges, prepaid terms, exposure, and conversion.
Neither proposal authorizes a new Casper protocol, voting rule, finality rule, global funding lock, or persistent deployment escrow.

## Current implementation evidence

These observations describe inspected source on September 11, 2026.
They are not new test results.

| Domain | Current source value or behavior | Implication |
| --- | --- | --- |
| Casper block protocol | Active version 6. The support predicate accepts only the current version. | Version constants for older protocols do not establish runnable historical support. |
| Deploy authorization | Version v6.1 has identifier `0x0006_0001`. | This identifier is not the Casper block version. |
| Authority accounting | Version 9. | Migration prose that describes the current authority version as 8 is stale. |
| Byte accounting | Schedule version 1. | This schedule is distinct from authorization and block versions. |
| Wire fields | `DeployDataProto` reserves tags 7, 8, and 15. Current fields extend through 20. | Restored controls require fresh tags and signed commitments. |
| Historical decoding | Processed deploy decoding retains legacy branches. | Decode compatibility does not prove historical execution or replay. |
| Genesis funding | Vault allocations and client grants increase `general_balance`. Validator grants increase `validator_fuel_balance`. | Importing existing balances and applying new grants can duplicate supply unless the manifest prevents it. |

Source references:

- [Casper version support](../../../../casper/src/rust/casper.rs)
- [Approved-block validation](../../../../casper/src/rust/validate.rs)
- [Authorization encoding and historical decoding](../../../../models/src/rust/casper/protocol/casper_message.rs)
- [Deploy wire schema](../../../../models/src/main/protobuf/CasperMessage.proto)
- [Authority accounting version](../../../../rholang/src/rust/interpreter/accounting/authority.rs)
- [Byte accounting schedule](../../../../rholang/src/rust/interpreter/accounting/byte_accounting.rs)
- [Genesis funding](../../../../casper/src/rust/genesis/genesis.rs)

## Recommended activation scope

Recommend a new-economy fresh genesis for the initial restored-control release.
This follows the recorded direction in the [signed phlo proposal](signed-phlo-contract-proposal.md).
Fresh genesis means an approved new initial state, not reinterpretation of a running shard's state.

| Mode | Recommendation | Required boundary |
| --- | --- | --- |
| New-economy fresh genesis | Initial activation mode. | Approve initial supply, allocations, custody roles, bonds, and supported execution rules. |
| Successor genesis with imported economic state | Require a separate explicit scope decision before support. | Preserve authenticated balances, rights, obligations, and issuance without duplicate grants. |
| In-place existing-shard upgrade | Require a separate explicit scope decision and upstream review where consensus compatibility changes. | Preserve accepted history and use an authorized boundary supported by Casper. |

This table proposes scope rather than silently removing migration requirements.
Imported-state and in-place activation are excluded from the ratified initial
release, not completed features. If either is separately approved for a later
release, its migration contract and verification become release blockers.
Unsupported modes must reject before mutation.

## Ratified Route A — channel-key namespace at activation

The user selected **Route A** on October 6–7, 2026. A *channel key* is the
canonical identity by which RSpace and its retained indexes locate a channel.
A *canonical `Par` channel* is the node's canonical Rholang process value used
as that channel, not a rendered source string. A channel is *cost-bearing* if
cost-accounting structure occurs anywhere in its recursively reachable
canonical `Par` content; testing only the outermost constructor is
insufficient. An *activation epoch* is the authenticated protocol context
that selects which key rule applies, not a local clock, command-line flag or
inference from the stored bytes.

| Authenticated context and channel | Required key rule |
| --- | --- |
| Pre-activation history, ordinary channel | Keep the historical key function and its exact stored and event bytes; reject cost-bearing source rather than assigning it a legacy key. |
| Activated context, ordinary channel with no recursively contained cost structure | Keep the legacy ordinary-channel key function and exact bytes. |
| Activated context, recursively cost-bearing canonical `Par` channel | Use a separately domain-tagged cost-channel key over that canonical value. Never alias it with an ordinary-channel key. |
| Unknown epoch, non-canonical channel, malformed or ambiguous pathmap key | Reject before lookup, mutation or replay publication; never guess a namespace or fall back to the ordinary key. |

The rule is **selective**: Route A does not re-key ordinary channels merely
because cost accounting is active. Conversely, hiding a cost term inside a
send payload, quote, collection, nested channel or other reachable `Par`
position must not accidentally route it through the ordinary namespace.
Domain separation applies to the canonical key preimage and its persisted
identity, not to a display string. The precise tag bytes, canonical traversal
recipe and versioned encoding require a separately reviewed wire contract;
this policy does not silently allocate them. Distinct ordinary and cost
preimages must have disjoint declared domains, with the usual cryptographic
collision-resistance assumption made explicit rather than claiming an
impossible unconditional hash-injectivity proof.

The authenticated epoch must be fixed by the accepted genesis/protocol
commitment and recovered from that committed context for validation and
replay. A validator must not select Route A from its local software version,
current wall clock, the apparent shape of a pathmap key, or the newest local
schedule. The same epoch and canonical channel must yield the same key bytes
in live execution, independent validation, event generation and checking,
historical replay, restart recovery and cold-store import. Events and stored
indexes must refer to the very key used by the live RSpace operation; an
equivalent-looking but byte-different reconstruction is a consensus failure.

Malformed pathmap keys are a particularly important negative case. Decode
with one canonical, bounded pathmap-key interpretation, reject truncated,
overlong, non-canonical and cross-domain representations, and verify that
re-encoding preserves the exact declared key bytes. A malformed cost key must
not be retried as an ordinary key, nor may two encodings of one logical key
produce different event or cold-store identities. Rejection precedes any
partial state or economic mutation.

Route A is an **approved design choice, not an activated implementation**.
The pre-activation fixed goldens—including ordinary key bytes, event bytes,
accepted effects and cold-replay results—must remain unchanged. The current
source and tests have not been established by this decision record to pass
the new cost-key or replay gates. No activation, migration, or historical
support claim follows merely from this approval.

The implementation and release gates are:

1. Pin the exact authenticated activation context, domain tag and canonical
   key encoding. Model ordinary-key preservation, recursive cost detection,
   namespace separation, malformed-input refusal and epoch selection before
   changing the key path. State cryptographic assumptions separately from
   structural invariants.
2. Implement one checked classifier/key function reused by produce, consume,
   peek, pattern matching, join matching, retained RSpace indexes and
   merge/conflict bookkeeping. Compare deeply nested cost-bearing channels,
   ordinary channels and canonical pathmap channels with independently
   specified examples and property tests; include deep, bounded traversal and
   malformed-key negative controls.
3. Bind the same key bytes to event creation, event verification, accepted
   block validation, hot replay, historical execution, restart recovery and
   cold-store export/import. Compare complete event and state bytes across
   these paths, not just a decoded channel or aggregate balance. Verify that
   an unsupported or unauthenticated epoch fails before effects.
4. Rerun untouched pre-activation goldens and historical fixtures through
   their original epoch. Run activated fixtures through independent validators
   and cold replay, including mixed ordinary/cost channels, nested cost
   structure, branch merge, duplicate delivery and malformed pathmap-key
   cases. Require byte-identical keys, events and accepted effects wherever
   the authenticated context is the same; require explicit compatibility
   rejection where it is not.

These gates are additional to the wire-consent, historical-execution and
funding gates below. Passing a local key-unit test does not establish native
replay, cold-store compatibility or activation safety.

## Compatibility and signed consent

Define an explicit compatibility manifest before production activation.
Each supported entry must identify the genesis identity, block protocol, authorization format, authority accounting version, and resource schedule.
Include the relevant native contract identities and historical execution implementation.
Reject undeclared combinations rather than infer compatibility from matching version numbers.

New funding consent must bind the exact applicable schedule, resource limit, maximum permitted price, assets, fees, payer exposure, and conversion terms.
A higher price ceiling does not authorize an unrelated resource schedule.
Preserve the approved ownership, delegation, and persistent allowance rules across all applicable authorizations.

Use fresh wire tags and a domain-separated signed format for restored controls.
Do not reuse reserved tags or insert unsigned economic defaults.
Require fresh signatures when a submission changes to the new contract.
Reject mixed formats, missing required consent, and incompatible schedules before economic mutation.

The wire contract task must specify the exact format and compatibility entries.
No new Casper version is assumed here.
If old nodes can accept new semantics without validating them, activation is unsafe even with fresh genesis.
Resolve that compatibility blocker with upstream reviewers before release.

## Historical replay contract

Legacy and activated replay must preserve their original identity, pricing, limits, allocation, and settlement rules.
That requirement remains open until implementation and verification satisfy it, or the user separately approves narrower historical support.
Fresh-genesis approval does not waive it.
Current decode-only limitations are implementation gaps where the requested historical replay requires execution, not grounds for silently excluding that history.

Preserve the original signing bytes, deploy identity, rules, and accepted effects for every history declared executable in the compatibility manifest.
Select historical rules from authenticated execution context, not the latest local schedule or current owner preferences.
Separate historical replay from permission to submit new deployments in an old format.

The manifest must enumerate supported historical combinations and the fixture corpus that demonstrates each one.
Its inventory must account for every requested legacy and activated combination, including unresolved implementation gaps.
An unsupported required combination blocks release unless the user explicitly narrows that requirement.
Do not label an old combination executable merely because a decoder accepts it.
Existing decode-only support must remain explicitly decode-only unless a verified execution implementation exists.
Do not silently remove a currently supported replay guarantee while adding restored controls.

Require cold replay of retained supported history after newer schedules, authorizations, and ownership states exist.
Historical replay must reproduce original effects rather than apply current pricing retrospectively.
Unsupported history must produce an explicit compatibility error without partial state mutation.

The current [migration guide](../cost-accounting-migration.md) limits active execution to protocol 6.
The historical support inventory must reconcile that statement with actual runtime dispatch and retained fixtures.
This proposal does not claim complete replay for every older branch version.

## Genesis and migration integrity

For a new economy, the genesis manifest must distinguish approved new grants from imported or pre-existing supply.
Validate overlaps across vault allocations, client grants, and validator fuel without combining distinct custody roles.
Use checked arithmetic and deterministic canonical identities.

If imported activation is approved, preserve each applicable item below:

- Balances, asset identities, physical custody, and prepaid backing.
- Ownership, threshold authority, delegation, restrictions, and consent.
- Persistent allowances and previously consumed amounts.
- Outstanding obligations, captured refund provenance, and allocation cursors.
- Validator fuel, stake, quarantine state, and issuance history.

Require an authenticated source and explicit destination mapping for every imported balance and right.
Reject missing provenance or ambiguous mappings.
Do not replace missing consent with unlimited consent or reset consumed allowances.
Do not add default grants to copied balances unless the approved supply manifest explicitly includes those new grants.

Live proof-of-stake migration must derive the issuance frontier from complete canonical evidence.
The fresh-genesis `mintedThroughEpoch = -1` value is not a safe default for imported live state.
Reject disagreement between canonical history, supply, and the proposed frontier.
The [issuance migration requirements](../cost-accounting-migration.md#116-epoch-issuance-replay-protection) describe this existing boundary.

## Publication, clients, and recovery

Publish retained application effects and economic effects through one complete native checkpoint.
Include debits, refunds, allowance consumption, receipts, and cursor changes.
Incomplete candidates must not leave independently spendable intermediate assets or partial economic effects.

Preserve independent validator execution and existing Casper dependency and merge rules.
Do not require identical latest local state or serialize unrelated funding scopes.
Test competing candidates against shared custody separately from candidates with independent custody.

Update CLI, SDK, API schema, signing, validation, receipts, and diagnostics as one compatibility change.
Receipts must expose the selected terms, actual charges, released backing, and historical execution identity.
Recommend authentication through the existing accepted block commitment rather than an additional receipt-signing authority.
An external consumer still needs a verifiable binding from the receipt to that commitment.

Retain a verified recovery package before activation.
An older binary that cannot verify accepted new-format history must reject that history.
Software downgrade does not authorize economic rollback or reinterpretation of committed effects.
This proposal authorizes neither remote rollout nor data deletion.

## Formal and executable acceptance

The existing [version lifecycle model](../../../../formal/tlaplus/deploy_recovery/ProtocolVersionLifecycle.tla) models fresh-genesis version agreement.
It does not prove restored-control migration or arbitrary rolling upgrades.
The [activation coherence model](../../../../formal/tlaplus/deploy_recovery/ProtocolActivationCoherence.tla) models defensive version composition using earlier abstract version constants.
It does not define activation authority or establish current numeric-version correspondence.

Extend models for immutable historical rules, consent preservation, atomic publication, and concurrent independent validators.
If imported activation is selected, add migration conservation and issuance-frontier obligations before implementation.
Bind every model assumption to source behavior and identify unsupported cases explicitly.
Bounded state exploration does not prove unbounded network liveness, cryptographic security, or native storage atomicity.

| Obligation | Required evidence |
| --- | --- |
| Wire and consent | Fixed signing vectors, round trips, field mutations, missing consent, mixed formats, and unsupported versions. |
| Genesis | Independent builders and cold replay agree. Overlapping allocations cannot create unapproved supply or merge custody roles. |
| Historical rules | Supported historical fixtures reproduce original effects after later schedules and ownership changes. |
| Native activation | Actual proposal, independent validation, restart, recovery, and replay use the declared execution path. |
| Concurrency | Concurrent top-ups, transfers, shared provider capacity, competing candidates, and reversed admissible merge order preserve invariants. |
| Failure atomicity | Interrupt reservation, execution, settlement, checkpoint, and publication. Assert complete rollback or one complete retained effect. |
| Negative controls | Detect current-schedule historical replay, unsigned defaults, duplicate grants, reset allowances, and partial publication. |

Generate property tests from the formal invariants and use Loom for the corresponding shared-memory ownership boundaries.
Use native integration tests for routes that abstract models and direct helper calls cannot establish.
Record model bounds, generated cases, negative controls, revisions, and actual tool outcomes.

Activation, wire compatibility, and historical replay each require verification.
Client verification must cover signing, diagnostics, and phlo controls.
The activation release gate requires evidence from actual node execution paths.

## Approval requested

Approve the recommended new-economy fresh-genesis release scope, or explicitly include imported-state or in-place activation.
Approve exact schedule binding, explicit compatibility entries, preservation of declared executable history, and fail-closed unsupported modes.
Approve receipt authentication through existing block commitments without adding a new signing authority.
Detailed wire assignments and executable support entries still require their planned implementation and verification evidence before release.
