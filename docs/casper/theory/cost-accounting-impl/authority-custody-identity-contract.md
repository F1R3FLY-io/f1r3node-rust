# Authority and custody identity

## Purpose

Funding must preserve required logical authority while counting each physical balance only once.
This contract specifies payer identity, custody aliases, logical multiplicity, capabilities, and joint purses.
It applies the [allocation policy](authority-allocation-policy-decisions.md) and [economic policy](economic-failure-policy-decisions.md).
It does not add planned linear operators or change Casper's consensus rules.

## Identity domains

| Identity | Meaning | Required distinction |
| --- | --- | --- |
| Cryptographic principal | A canonical public key with its verification scheme. | Different schemes need separate signature verification even when they share custody. |
| Ground authority | A key-family and canonical-key identity used by the authorization policy. | A duplicated ground identity cannot fill independent signer slots. |
| Logical lane | A canonical authority expression associated with resource requirements. | Repeated required occurrences retain their multiplicity. |
| Physical custody | One balance under a particular asset, custody role, contract, and network context. | Aliases do not create another balance. |
| Grant or resource identity | A specific delegated permission or located resource instance. | Sharing a purse does not merge distinct permissions or resource instances. |
| Execution identity | The authenticated operation and its captured causal context. | A custody projection must not replace the signed deployment identity. |

The [deployment identity specification](deploy-envelope-v6-1.md) defines current principal and ground encodings.
Its distinction between signature schemes and ground custody remains applicable.
The [delegation contract](delegation-persistent-authority-contract.md) separately governs grant versions and individual operation identities.

## Authentication before allocation

Decode the supported signed format and validate its canonical identities before admitting funding evidence.
Verify each required cryptographic presentation under its declared scheme and signed context.
Apply the actual threshold or compound authority policy without counting placeholders as authenticated principals.
Do not infer funding permission from an unverified public key, balance query, or supplied payer address.

Resolve the eligible logical lanes from verified authority, available capabilities, and authenticated resource evidence.
Resolve physical custody only through the applicable native contract and supported canonical projection.
Bind every balance observation to its asset, role, contract, network, and causal state.
A proposer-supplied alias map cannot establish this mapping by itself.

Physical resolution does not replace logical authorization.
The validator must independently reconstruct both mappings from the accepted inputs.
Unknown authority, missing custody, inconsistent balance observations, and unsupported projection must fail explicitly.
Failure must not select another wallet as an automatic fallback.

## Current native projection

The [payer resolver](../../../../casper/src/rust/util/rholang/costacc/vault_payer.rs) returns a canonical signature, logical lane, vault address, and custody digest.
It rejects unit authority as a payable vault.
Canonical secp256k1 ground keys and supported family-1 principal encodings resolve to the same native public-key vault.
Their logical lane identities remain distinct.

A supported isolated private name resolves to its unforgeable vault address.
Other accepted authority shapes resolve through their canonical lane digest to a distinct unforgeable address.
Malformed or unsupported principal encodings must not alias the native public-key purse.
Resolution to a distinct address is not proof of permission, positive balance, or resource compatibility.

The current custody digest contains a domain tag and the vault address.
It does not itself encode all asset and role distinctions listed above.
The current caller context must supply any implicit distinction.
Before integrating additional roles or assets into one inventory, make those distinctions explicit in its key or authenticated namespace.
Do not silently change historical custody hashes to repair an insufficient new inventory type.
Preserve historical decoding, execution identity, and applicable native address mappings.

General funding and validator-fuel balances at one address are different custody roles.
They cannot satisfy each other's obligations merely because their addresses match.
The same rule applies to different assets and independent shard contexts.

## Direct public-key wallet authorization

[`authorize_direct_wallet_funding`](../../../../casper/src/rust/util/rholang/costacc/direct_wallet_funding.rs) implements the direct public-key wallet case for signed funded deployments.
It uses the existing `vault_payer` mapping rather than a new wallet-address scheme.
Each requested custody identity must match a native wallet whose owner supplied a verified envelope signature.
An unsigned threshold member cannot authorize its wallet, even when the selected members satisfy the envelope's quorum.
A valid envelope cannot authorize an unrelated wallet merely by naming its custody identity.

The checker validates the complete signed funding record and retains the envelope, decoded record, and canonical payer map together.
`authorize_offered_direct_wallet_funding` applies the same wallet authorization to the distinct signed offered-price envelope.
It preserves the envelope type through wallet snapshots, family binding, native policy selection, and capture.
The two public authorization entry points share a private implementation.
Callers cannot supply a substitute funding record to that implementation.
It counts all policy members against the member cap, including absent threshold members.
The funding decoder independently enforces source and representation limits and rejects repeated custody entries.
Malformed native custody identifiers and inconsistent custody projections cause rejection.

For example, a two-of-three policy contains Alice, Bob, and Carol.
Alice and Carol sign a record that requests funds from Bob's wallet.
The envelope can satisfy its quorum, but direct-wallet authorization rejects the requested source because Bob did not sign.
A delegated capability could supply a different authorization path, but the direct-wallet checker does not infer such a capability.

This checker reads no balances and publishes no state.
It does not validate withdrawal grants, authorize unforgeable funding slots, or implement joint-wallet policies.
Those source kinds require their own state-bound proofs and must not fall back to direct public-key authorization.
An empty source list passes this membership check without granting funding or admission.
The later funding check must still cover every required charge.

Admission must bind the resulting sources to the applicable SystemVault instance, custody role, asset, shard, and certified state.
It must check balances, resource permissions, exposure, schedule consent, and settlement through the existing funding-family contract.
The direct-wallet result alone does not authorize a system cost application or activate the funded deployment format.
The offered-envelope family binder separately enforces exact offer and limit equality through `check_offered_signed_family`.
Wallet membership alone does not establish that the selected price matches the signed offer.

`CanonicalCustodyAliasing.v` proves that the direct-wallet membership checker accepts exactly the requested custodies projected from selected principals.
It also proves unsigned-custody rejection and invariance under corresponding list permutations.
The projection represents the native wallet mapping, not a proof of its cryptography or on-chain balance.
Native tests exercise real signatures, threshold exclusion, every member count through 64, increased-cap operation, malformed custody, and both supported signature schemes.
Offered-envelope regressions retain both signature schemes and reject unsigned scalar changes, unsigned wallet owners, and malformed custody identifiers.
A property test compares authorization against an independent requested-wallet subset check and repeats it after source-order reversal.
A composed native test checks direct-wallet authorization, signed consent, fee obligations, and canonical capture for 1, 3, and 64 wallets.
The test uses supplied balances and does not establish on-chain state authentication or checkpoint publication.

## State-bound direct-wallet inventory

The envelope extension does not change concurrent read limits, wallet addresses, cursor contexts, or canonical allocation.
Real-runtime tests compare both formats at roots before and after transfers and top-ups.
They compare complete inventories, resource and fee cursors, captured amounts, refunds, and the original envelope reference.
Controlled-reader tests also exercise out-of-order completion, read failure, negative balances, and root mismatch with offered envelopes.

`NativeFundingSnapshot.v` composes selected-owner authorization, complete snapshot-row binding, and offered-family refinement.
The composition retains the original root, rows, payload, offer, and execution snapshot.
Each family row retains its custody, capacity, hold cap, and debit cap.
The proof assumes the wallet projection, authenticated payload, and fixed-root storage correspondence.
It does not infer authority from requester-supplied balances or prove cryptographic properties of the address mapping.

[`DirectWalletFunding::read_snapshot`](../../../../casper/src/rust/util/rholang/costacc/direct_wallet_funding/snapshot.rs) reads the authorized wallets through a `SupplyReader`.
The caller supplies the expected pre-state root and a nonzero parallel-read limit.
The reader must execute every query against that fixed state, including resource-stack queries and SystemVault balance queries.
The method checks the reported root before and after the reads.
These checks detect a reported mismatch but cannot establish that an incorrect reader actually queried its reported root.

`RuntimeManagerSupplyReader` sends both queries to its stored `pre_state_hash`.
`RuntimeOpsSupplyReader` requires its isolated runtime to remain at the supplied root.
The caller must not substitute mutable latest-state reads or an unverified remote balance response.
The expected root remains an admission input, not evidence of consensus certification by this helper.

The snapshot retains the signed authorization, root, source identity, consent limits, and complete purse inventory.
An absent balance supplies zero monetary capacity. A negative balance causes rejection.
Stored resource stacks retain their identities, channels, provenance, and persistence flags.
The snapshot does not count those stacks as additional monetary balance or independently authorize their consumption.

The method bounds concurrent queries by the smaller of the source count and the requested limit.
Queries can complete out of order, but results retain the signed source order.
An empty source set requires no balance queries and supplies no capacity.
A query error discards the incomplete result. The method neither publishes state nor returns a partial snapshot.
Separate callers need an aggregate admission budget because the parallel-read limit applies to each call, not the whole node.

For example, two wallet queries must observe the same selected root even when a later state contains a transfer between those wallets.
A top-up in that later state does not increase capacity in the earlier snapshot.
Reading snapshots alone does not reserve balances or prevent two operations from requesting the same backing.
Funding and settlement must still enforce combined capacity through the accepted state-transition rules.

### Binding a funding family

[`DirectWalletSnapshot::bind_family`](../../../../casper/src/rust/util/rholang/costacc/direct_wallet_funding/binding.rs) connects a checked funding family to the authenticated inventory and signed funding intent.
Every family source must retain its snapshot custody identity, balance capacity, exposure limit, and debit limit.
The family must include every snapshot source exactly once. The existing family checker rejects duplicate custody identities.
This requirement prevents omission of an authorized source from changing the allocation cohort.
Source order can differ because the binding compares custody identities rather than array positions.

The binding then checks the funding record against the original signed envelope and verifies funding terms and source permissions.
Its result retains both the snapshot and checked signed consent.
The selected capture can therefore remain associated with the state that supplied its backing.
Returning a generic checked family without this binding does not establish authenticated wallet capacity.

These checks reject both increased and decreased source capacities supplied by a caller.
The snapshot defines available backing. Signed caps and the selected assignment separately define permitted exposure and actual use.
Changing an observed balance to simulate a tighter limit would change allocation inputs rather than express consent through the signed policy.
The binding does not reserve money, execute resource proofs, publish settlement, or certify the supplied state root.

### Snapshot verification boundary

[`FundingSourceSnapshot.tla`](../../../../formal/tlaplus/cost_accounted_rho/FundingSourceSnapshot.tla) models independent readers, concurrent queries, query failure, and later-state changes.
The checked configuration contains two readers, two sources with different balances, two state versions, and up to two active queries per reader.
The model checks fixed-state observations, complete results, bounded active reads, and agreement between completed readers of the same state.
Negative controls expose mixed-state reads and incomplete publication.
This finite safety check does not prove arbitrary-size liveness, cryptography, RSpace implementation correctness, or distributed settlement.

[`snapshot_tests.rs`](../../../../casper/tests/direct_wallet_funding/snapshot_tests.rs) tests the native helper with signed envelopes and controlled asynchronous readers.
Generated cases vary balances, absence, source order, wallet count, and parallel-read limits.
Additional cases check query errors, root mismatch, negative balances, empty funding sets, and preservation of stored resources.
[`wallet_snapshot_state.rs`](../../../../casper/tests/util/rholang/wallet_snapshot_state.rs) tests the real SystemVault reader across transfers and top-ups at distinct roots.
That test exercises fixed-state inventory reads, not complete signed deployment admission or replay publication.

`CanonicalCustodyAliasing.v` defines `snapshot_rows_match` over arbitrary lists of source rows with decidable equality.
`snapshot_rows_match_exact` proves that successful matching includes only rows from the snapshot.
`snapshot_rows_reject_substitution` rejects any family row absent from that snapshot, including an altered balance or limit.
`snapshot_rows_permutation` preserves matching under independent reorderings of the two lists.
The native row includes custody, capacity, exposure limit, and debit limit.
The binding additionally checks equal source counts and relies on the family checker's duplicate-custody rejection.
The row proofs do not establish cryptographic validity or authenticate state reads. Those remain separate obligations.
The composed native test connects signatures, inventory reads, family binding, signed consent, and canonical fee capture for 1, 3, and 64 wallets.
Its property test varies wallet counts and balances, rejects each source-field substitution, and accepts reordered source rows.
The inventory reader in that composed test is controlled. The separate SystemVault regression checks the real fixed-root query implementation.

## Alias collapse and logical multiplicity

Let $`L`$ be the admitted logical lanes and $`P`$ the physical custody entries.
The authenticated map $`\pi:L\to P`$ maps each admitted lane to its balance source.
Let $`d_l`$ denote a logical lane's selected monetary debit in compatible units.
The physical debit for purse $`p`$ is:

```math
D_p = \sum_{l\in L:\pi(l)=p} d_l.
```

Require $`D_p\le B_p`$, where $`B_p`$ is the available authenticated balance after applicable obligations.
Use checked arithmetic when forming this sum.
Reject a missing map entry or inconsistent observation before publishing effects.

The sum visits each lane key once after aggregating its required occurrences into $`d_l`$.
An occurrence-list representation must instead assign each occurrence its own debit before projection.
Do not repeat an already aggregated lane debit for each occurrence.

The [physical inventory](../../../../rholang/src/rust/interpreter/accounting/authority.rs) already separates lane mapping from stored physical balances.
`insert_balance_lane` rejects conflicting mappings and inconsistent balances for known custody.
`physicalize_balance_debit` aggregates lane debits with checked addition.
These primitives do not authenticate their supplied inputs by themselves.

For example, two authorized lanes map to one purse containing ten units.
Debits of six and five require eleven physical units and must reject.
Two views of a ten-unit balance do not create twenty units.

Alias collapse gives that purse one monetary allocation position.
It must not delete required logical occurrences, merge distinct located resources, or make a required signature optional.
The [monetary cohort](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/cohort.rs) retains logical-lane membership beside each physical payer.
Its membership set is not a replacement for the resource multiset's occurrence counts.
The complete assignment witness must preserve both physical contributions and the logical obligations those contributions discharge.

## Joint purses and capabilities

A joint purse is a separately identified custody source controlled by its actual authority policy.
It is not the sum of member balances and does not duplicate those balances.
An admitted cohort can include both a joint purse and authorized individual purses.
Each distinct physical purse receives one allocation position under the selected policy.
Configured limits must distinguish signer count from physical-purse count.

For a joint purse, verify the required policy and consent before including its balance.
Do not assume that every signer can withdraw independently from that purse.
Do not require a joint purse to fund unrelated individual obligations without permission.

An unforgeable funding slot identifies a resource capability, not automatic ownership of the depositor's entire wallet.
The [rho specification](../../../../../publications/cost-accounting/cost-accounted-rho.tex), section `sec:funding-slots`, permits deposits without a fixed depositor identity.
Consumed resources still require valid location, authority, backing, and provenance.
The [continued-GSLT specification](../../../../../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex), rule `eq:R1`, preserves the remaining purse's location.
Canonical custody must not flatten that resource-location distinction.

## Captured identity through settlement

Capture the exact funding assignment, grant terms, resource identities, and physical refund sources for the accepted operation.
Transfers and top-ups must not rewrite this capture or retroactively expand its permission.
Settlement validates the realized effect against the capture and aggregate physical capacity.
Unused backing returns through its captured asset and custody path under the selected conversion composition.

Duplicate delivery, retry, and concurrent candidates must preserve operation identity independently from payer identity.
The same purse can fund distinct permitted operations without making those operations duplicates.
The same operation cannot be charged twice by changing its custody presentation.
Use existing causal dependency and conflict rules, not a new global funding lock.

## Verification requirements

| Invariant | Required test or negative control |
| --- | --- |
| Projection preserves authority | Change scheme, key family, encoded length, key bytes, or principal presence without a valid signature. |
| Aliases preserve physical capacity | Generate multiple lanes per purse and compare aggregate debit with an independent sum. |
| Custody roles remain distinct | Give one address general and validator-fuel balances, then attempt cross-role funding. |
| Assets and networks remain distinct | Reuse address bytes under incompatible asset, contract, or shard contexts. |
| Logical multiplicity survives collapse | Repeat required resource occurrences while retaining one physical payer position. |
| Joint funding requires actual consent | Add an unsigned joint purse or treat a partial presentation as full withdrawal authority. |
| Captured refunds remain stable | Change owners or alias presentation between accepted funding and settlement. |
| Identity is canonical | Permute equivalent encodings, introduce noncanonical encodings, and compare signing, lane, and custody results separately. |
| Publication is atomic | Race shared-custody candidates and inject failures before debit, refund, and checkpoint publication. |

Formal refinement must establish that native resolution constructs the same alias relation assumed by the resource and monetary proofs.
Keep cryptographic verification, hash assumptions, custody observation, and causal-state authenticity explicit.
Generate property tests for arbitrary admitted arity, aliases, repeated occurrences, zero balances, and full-width arithmetic.
Use Loom for native synchronization boundaries and integration tests for independent-validator execution and replay.
No address lookup, hash equality, or abstract conservation theorem alone establishes complete authorization.

## Existing evidence and remaining correspondence

The [custody model](../../../../formal/rocq/cost_accounted_rho/theories/CanonicalCustodyAliasing.v) separates physical debits from logical stack consumption.
Its lane and purse types are parameters. Its arithmetic uses natural numbers rather than bounded machine integers.
Its custody map is an input, not a result of cryptographic verification or native vault resolution.

| Existing artifact | Established specification boundary | Required native correspondence |
| --- | --- | --- |
| `physical_draw_is_permutation_invariant` | Reordering the lane list preserves its aggregate physical debit. | Canonical decoding and inventory construction must preserve the same obligations and custody map. |
| `physical_reservation_batches_use_remaining_capacity` | Sequential residual capacity equals the combined arithmetic obligation under the stated premises. | Competing candidates need validated observations and atomic publication. This theorem does not model their concurrent execution. |
| `physical_refund_matches_original_custody` | Bounded realized debits and unused reservation sum to the original reservation under one captured map. | Native refunds must retain that map, asset, custody role, and conversion provenance. |
| `physical_alias_does_not_merge_stack_authority` | A physical alias does not consume a distinct stack whose debit is zero. | Location, permission, and occurrence identity must remain separate through actual execution. |
| `occurrence_aggregation_preserves_physical_draw` | Grouping repeated occurrences into unique lane keys preserves every assigned debit under one complete custody map. | The native map must retain all occurrence contributions and visit each aggregated lane key once. |
| `occurrence_aggregation_preserves_capacity` | Occurrence-level and grouped-lane capacity checks agree under the same map and key coverage. | Missing custody entries, incompatible units, and machine overflow must reject explicitly. |
| `jointly_backed_settlements_commute` | Two jointly backed settlements remain admissible in either order and conserve their combined draw. | Concurrent publication must validate shared capacity and preserve both updates. Separate snapshot checks are insufficient. |
| `protocol_principal_uses_the_existing_public_key_vault` | The native resolver test compares legacy and family-1 custody while retaining distinct lane keys. | Signed admission and replay must preserve this projection without accepting duplicate policy members. |
| Resolver malformed-family and malformed-length properties | Unsupported presentations must not alias the tested native public-key purse. | Complete authorization must reject invalid presentations before funding, not merely resolve another address. |

The arithmetic model accepts a lane list, which can contain duplicates.
Native map-based projection aggregates each lane before iteration.
Refinement must relate these representations without dropping occurrences or counting an aggregated debit twice.

`aggregate_occurrences` groups assigned monetary debits by lane before physical projection.
An occurrence's amount is its assigned debit, not an automatic charge for each authority owner.
The grouping theorem permits repeated occurrences but requires distinct aggregate keys and coverage of every occurrence's lane.
It does not select a monetary allocation, value authority expressions, or replace linear-resource validation.
The original occurrence identities and resource permissions remain separate inputs to those checks.

The formal custody map is total. The native map is partial and rejects unknown custody before summing a debit.
An explicit zero-valued native entry also needs a custody mapping.
The projection theorem describes successful arithmetic, not proof of native missing-map rejection or checked integer arithmetic.

The commutation theorem uses one fixed custody map and combined per-purse backing.
It permits different draw amounts for the same lane across the two operations.
It does not require a global execution lock or prove that two independently admitted snapshots can both commit.
`independently_admitted_snapshots_can_overdraw` refutes that weaker condition with one shared unit of backing.

`repeated_occurrences_are_not_deduplicated` preserves two contributions when they share one aggregate lane.
`repeating_aggregated_lane_keys_double_counts` shows why the grouped-key list must be duplicate-free.
These examples complement the arbitrary-list theorems. They do not establish native execution or concurrent publication by themselves.

These artifacts do not prove mint authorization, cross-asset conversion, distributed replay, or complete native publication.
Tests must distinguish genesis allocation, ordinary deposits, authorized issuance, and resource acquisition.
A deposit changes available backing. It does not create another asset identity or duplicate a located resource.
