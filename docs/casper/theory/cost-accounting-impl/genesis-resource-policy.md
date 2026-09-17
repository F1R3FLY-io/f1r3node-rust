# Genesis resource policy

## Purpose and authority

The genesis resource policy authorizes the interpretation of accounting resources.
Payer signatures authorize spending under those rules. Payer signatures do not authorize replacement measurement rules or tariffs.
The [price contract](price-schedules-and-denominations.md) defines the separate signed offered price and owner ceilings.

The policy belongs to the approved fresh-genesis state.
The genesis ceremony checks the complete policy through the expected blessed deploys and their replayed state.
The policy does not add a committee, certificate, voting rule, finalization rule, or recovery policy.
Its activation remains subject to the accounting compatibility contract and required FIP approval.

## Record and encoding

[`PhloGenesisPolicy`](../../../../models/src/rust/phlo_schedule.rs) contains a canonical schedule with a zero price field.
Zero is a representation marker here, not an offered price, free execution, or permission to ignore minimum-price validation.
The wrapper uses the distinct domain `f1r3node:genesis-resource-policy:v1`.
Both wrapper fields use the existing length-prefixed phlo encoding.
The first field contains the domain. The second contains the complete canonical schedule.

| Policy field | Meaning |
| --- | --- |
| Protocol, network, shard | Identifies the execution environment. |
| Settlement asset, unit, decimal scale | Defines the denomination of resource charges. |
| Ordered resource classes | Retains every identity, measurement unit, measurement rule, valuation rule, and weight. |
| Compatibility rule | Identifies the rule for compatible prepaid rights. |
| Fixed schedule fee | Preserves the separate one-unit fee policy. |

The decoder rejects priced policy records, duplicate classes, empty required identifiers, malformed widths, unsupported domains, and trailing bytes.
Version 1 permits at most 4,096 classes, 1,048,576 total bytes, and 524,288 bytes per field.
Nested encoding must satisfy the same field limits.
These are representation limits, not wallet-count limits or economic prices.

The network identifier is explicit genesis input. It is not the hash of the genesis block that contains the record.
This avoids a self-referential hash requirement.
Network provisioning must select the intended network identity consistently across ceremony participants.
The approved genesis commitment authenticates that selection and the complete record.

## Genesis construction and storage

The genesis configuration accepts `resource-policy` as hexadecimal canonical record bytes.
`PhloGenesisPolicy::from_schedule` creates the record from a complete schedule and removes only its actual price.
The record does not supply default tariff values or derive resource rules from token display metadata.

The bootstrap and ceremony validators decode the configured record.
Genesis construction checks the protocol version, shard identifier, and token decimal scale before execution.
Ceremony validation includes the record when it reconstructs the expected blessed deploys.
Different policy records therefore produce different expected genesis contracts.

The existing immutable `TokenMetadata` contract exposes the bytes through its `resourcePolicy` method.
The contract contains a constant byte array and exposes no policy update method.
The existing registry entry provides its read-only interface. No new registry signing authority is introduced.
Existing name, symbol, decimals, and aggregate metadata methods retain their meanings.

An omitted record preserves the existing policy-free genesis construction path.
Such a genesis does not authorize offered funded admission through the genesis-policy binding.
The loader rejects a missing record. It does not substitute local configuration or the submitted schedule.
This distinction preserves existing fixtures and declared historical formats without silently activating the new funding contract.

## Authenticated loading and funding

[`GenesisResourcePolicy::load`](../../../../casper/src/rust/util/rholang/costacc/genesis_resource_policy.rs) requires the approved genesis block from the existing authority chain.
The caller must establish approval before this call. The loader is not a block-signature or ceremony verifier.
The loader rejects a block with parents and queries only its genesis post-state root.
It requires exactly one canonical byte-array result and checks the record against the genesis protocol, shard, and on-chain decimal scale.
The loader also reads the minimum phlo price from the PoS contract at that same root.
It publishes the loaded value only after all reads and checks succeed.

The loaded value retains the genesis root, its minimum price, and an immutable owned record.
Load the policy during accounting initialization and retain the value for funding checks.
Restart requires the authenticated genesis state or its verified recovery.
The loader does not accept a current wallet root, current finalized root, or local tariff as a replacement authority.
Missing state or malformed data returns an error.
The caller must recover required state or reject the operation, not construct a replacement policy.
A local read failure does not prove that a block is invalid.
Production admission must preserve the existing dependency-recovery and error-classification rules.

`bind_native_family_from_genesis` compares the selected funding schedule with the loaded policy.
It also requires the adopted minimum to equal the minimum captured from that genesis root.
Matching policy bytes alone cannot authorize a minimum taken from another genesis or local configuration.
The comparison excludes only actual price. Existing checks still require that price to equal signed `phloPrice`.
The adopted minimum, signed `phloLimit`, required owner ceilings, funding proof, allocation, and exposure checks remain separate obligations.
The lower-level `bind_native_family` helper accepts an explicit policy view and does not establish provenance itself.

Historical execution must use the original approved genesis and declared compatibility rules.
Later wallet deposits, ownership transfers, local restarts, and other available state roots cannot select replacement policy bytes.
The policy record does not implement arbitrary new measurement rules merely because their identifiers decode successfully.
The execution adapter must support every activated rule and reject unsupported combinations before economic mutation.

## Verification boundaries

[`SignedPhloSchedule.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloSchedule.v) proves price-independent policy preservation and exact offered-price reconstruction.
Its provenance lemmas prove missing-policy rejection, wrong-root rejection, validator agreement, and preservation when other state changes.
The storage model assumes that authenticated reads preserve the contents at the genesis root.
These lemmas do not prove native storage integrity or ceremony authentication.

[`AdoptedPhloContext.v`](../../../../formal/rocq/cost_accounted_rho/theories/AdoptedPhloContext.v) composes the captured, adopted, and genesis minima.
Acceptance requires all three minima to agree.
The offered-price theorem then preserves the authoritative minimum and every required owner's ceiling.

[`GenesisResourcePolicy.tla`](../../../../formal/tlaplus/cost_accounted_rho/GenesisResourcePolicy.tla) models independent validator requests and completions, local advancement, restart, and state availability changes.
The checked configuration contains three validators and two distinct state roots with different policies.
The safety invariants require genesis authority and agreement among successful loads.
One negative control selects the latest local root and must violate genesis authority.
A second keeps the genesis policy but substitutes another root's minimum. This control must also violate genesis authority.
This model establishes neither month-long uptime nor unbounded network liveness.

Rust property tests exercise full-width prices, generated class lists, policy preservation, canonical round trips, and altered weights.
Example tests cover truncation, domain confusion, nonzero record prices, trailing data, and context mismatch.
Native tests construct genesis records, load them concurrently, reject absent records and altered schedules, and reread original policy after another genesis exists.
Ceremony tests reject omitted or different expected policy records before approval.
The funded-settlement regression uses the loaded genesis policy with changing wallet roots and retained resource and fee cursors.
It also rejects identical policy bytes from a genesis with a different minimum, even when the supplied local context permits the offer.
The tests exercise the real contract and RSpace query path.
They do not replace the campaign's complete offered-deploy admission, execution, settlement, restart, and replay integration gates.

## Alternatives and compatibility

A compiled policy manifest selected by authenticated genesis identity can establish the same authority boundary.
That alternative requires each compatible binary to retain the correct manifest entry and reject unknown identities.
The genesis record makes the policy inspectable in authenticated chain state and avoids local manifest drift.
Both approaches require supported rule implementations and explicit historical compatibility.

This release provides no policy update API or in-place tariff upgrade.
Any later policy change requires an approved activation mechanism and a definition of historical replay behavior.
Changing genesis contents is consensus-visible, even though the Casper voting and finality algorithms do not change.
