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

`AdoptedResourcePolicy` owns the loaded policy after checking the adopted minimum, protocol version, shard, and native rule interpretations.
Its fields are private. Later changes to a configuration value cannot change the retained policy or its minimum.
The native family binding accepts this retained context instead of separate policy and configuration arguments.
It checks the captured execution parameters and the selected schedule before checking signed family consent and allocation.

Running Casper exposes this context through `accounting_context()`.
Concurrent requests share one successful initialization within that Casper instance.
Failed initialization does not publish a context or prevent a later retry.
Other Casper instances initialize independently. This mechanism does not serialize execution or establish a cross-validator lock.
Initialization queries the original approved genesis, not the latest finalized state or candidate wallet state.
Historical paths that do not request offered funding do not require a policy record merely to construct Casper.

### Executable native rules

The native resolver checks each declared measurement rule, measurement unit, and valuation rule before adoption succeeds.
It also checks the resource compatibility rule.
Matching genesis and signed schedule bytes alone does not establish that the binary implements those rules.

Each identifier below is the Blake2b-256 digest of its exact UTF-8 domain string.
The native implementation compares all 32 identifier bytes.

| Dimension | Measurement unit | Measurement domain |
| --- | --- | --- |
| Compute | `COMM` | `f1r3node:phlo-measurement:comm-occurrence:v1` |
| Introduction | `byte` | `f1r3node:phlo-measurement:canonical-introduction-bytes:v1` |
| Transfer | `byte` | `f1r3node:phlo-measurement:canonical-delivery-bytes:v1` |
| Trace | `byte` | `f1r3node:phlo-measurement:canonical-trace-footprint-bytes:v1` |

The [measurement contract](resource-units-and-measurement.md) defines these quantities and their observation boundaries.
Compute denotes COMM occurrences, not machine time or reducer instructions.
The valuation domain is `f1r3node:phlo-valuation:authority-leaf-occurrences:v1`.
This valuation counts ground and quoted authority occurrences, including repetitions, and assigns zero units to unit authority.
The selected class weight then scales that count. It is not a wallet-count multiplier or a cost-sharing weight.

The compatibility domain is `f1r3node:phlo-compatibility:exact-location-class-terms-authority:v1`.
It requires equality of location, class, acquisition terms, and complete authority structure.
The resolver does not authenticate prepaid acquisition provenance or replace the execution witness checks.

Native resolution supports one declared class per measurement dimension, with at most four classes in this implementation.
This limit applies to implemented measurement rules, not funding wallets, owners, or ownership transfers.
Duplicate dimensions reject because they would give one observation multiple class assignments.
Class order does not define measurement meaning. The resolver retains the exact index assigned by the authenticated policy.
Unknown rules or incompatible units reject without selecting a default.

A subset policy does not provide omitted resources for free.
Requesting an omitted dimension returns `Missing`, which the native producer must treat as an error.
Resolution leaves class identities, weights, offered prices, denominations, and the original policy record unchanged.
It does not enable an incomplete producer or bypass native admission, resource sufficiency, or settlement verification.

[`NativePhloRules`](../../../../rholang/src/rust/interpreter/accounting/native_phlo_rules.rs) implements this resolver.
Its tests cover all full-class permutations, rule-bit mutations, missing and duplicate dimensions, and generated weights and prices.
The `SignedPhloSchedule.v` rule lemmas prove preservation of supported interpretation fields, compatibility, and the complete accepted schedule.
They do not prove that runtime measurements or prepaid provenance satisfy the complete native producer contract.

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

### Original acquisition terms

`AdoptedResourcePolicy::check_acquisition_terms` checks a canonical original schedule against the complete retained policy.
The comparison excludes only the original actual price.
It retains protocol, network, shard, denomination, ordered classes, measurement rules, valuation rules, weights, and compatibility.
The existing schedule decoder also checks the fixed fee representation and all wire limits.

The result, `CompatibleAcquisitionTerms`, retains the exact input bytes, decoded original schedule, and a reference to the adopted context.
Private fields prevent callers from constructing an unchecked result.
The result exposes no mutable schedule or policy reference.
A later selected offer cannot replace the original price or acquisition bytes.

For example, compatible rights acquired at price 12 retain price 12 when a later process offers price 20.
The check does not use current wallet balances or apply a new owner's purchase ceiling to an earlier acquisition.
It does not authorize that earlier acquisition either.
The receipt producer must separately establish original funding authority, valid purchase terms, backing, and the actual live resource.
Even a zero-price schedule can pass compatibility without proving any spendable credit.

The returned type establishes policy compatibility only.
The [prepaid receipt contract](prepaid-receipt-storage.md) requires stronger evidence before consumption or issuance.
Original acquisition bytes remain part of the exact resource key.
Policy compatibility does not make different acquisition records interchangeable or permit fresh acquisition to bypass compatible prepaid rights.

The bound applies to each schedule input.
The enclosing producer must also limit aggregate receipt counts, bytes, and verification work before processing a batch.
This method does not establish a batch-wide memory or execution bound.

## Verification boundaries

[`SignedPhloSchedule.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloSchedule.v) proves price-independent policy preservation and exact offered-price reconstruction.
Its provenance lemmas prove missing-policy rejection, wrong-root rejection, validator agreement, and preservation when other state changes.
Its acquisition lemmas prove full-policy equality, original-record preservation, price independence of compatibility, and rejection of changed policy fields.
These acquisition lemmas do not prove backing or authorize credit creation.
The storage model assumes that authenticated reads preserve the contents at the genesis root.
These lemmas do not prove native storage integrity or ceremony authentication.

[`AdoptedPhloContext.v`](../../../../formal/rocq/cost_accounted_rho/theories/AdoptedPhloContext.v) composes the captured, adopted, and genesis minima.
Acceptance requires all three minima to agree.
The offered-price theorem then preserves the authoritative minimum and every required owner's ceiling.

[`GenesisResourcePolicy.tla`](../../../../formal/tlaplus/cost_accounted_rho/GenesisResourcePolicy.tla) models independent validator requests and completions, local advancement, restart, and state availability changes.
The checked configuration contains three validators and two distinct state roots with different policies.
Each validator has an independent set of supported policies. A restart can change that set and clears the retained context.
The safety invariants require genesis authority, agreement among successful loads, and supported interpretation for every adopted policy.
One negative control selects the latest local root and must violate genesis authority.
A second keeps the genesis policy but substitutes another root's minimum. This control must also violate genesis authority.
A third omits rule validation and must violate supported interpretation.
This model establishes neither month-long uptime nor unbounded network liveness.

Rust property tests exercise full-width prices, generated class lists, policy preservation, canonical round trips, and altered weights.
Example tests cover truncation, domain confusion, nonzero record prices, trailing data, and context mismatch.
Native tests construct genesis records, load them concurrently, reject absent records and altered schedules, and reread original policy after another genesis exists.
Context tests compare admission and captured parameters with the formal equality predicate across full-width numeric inputs.
Acquisition tests cover every policy component, class order and count, original price preservation, truncation, malformed lengths, domain errors, and oversized input.
Generated tests vary original prices, later prices, and class weights without changing previously checked records.
Native constructor tests check failed initialization, concurrent readers, and reconstruction from the same genesis with different bootstrap settings.
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
