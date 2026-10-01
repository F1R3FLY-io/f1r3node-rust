# Persistence and storage liability

## Purpose and scope

Persistence keeps process state and funding rights available across executions.
It does not give a process unlimited execution, duplicate its resources, or make its original installer fund every later activation.
This contract defines the required lifetime and release rules for the approved native funding design.
It does not claim complete native implementation or end-to-end verification.

The [resource measurement contract](resource-units-and-measurement.md) defines introduction, delivery, and trace charges.
The [resource bounds contract](resource-bounds-and-exhaustion.md) defines conservative funding and source-specific exposure.
The [delegation contract](delegation-persistent-authority-contract.md) defines authority, transfer, and persistent allowances.
These contracts apply together.

Storage liability means the obligations attached to retained state and its funding rights.
It does not mean a new byte-time rent charge.
Rent pricing, new linear operators, and changes to Casper pruning remain outside this contract.
A one-time introduction charge does not prove a bound on lifetime storage or node uptime.

## Specification basis

The *Cost-Accounted Rho Calculus* paper defines funding slots in `sec:funding-slots`.
A party with the unforgeable slot name can deposit resources without becoming the fixed owner of every later continuation.
The acceptance proof checks the available matching resources, not merely the depositor's identity.

Rule R1 of *Continued Interactive GSLTs and the Cost Endofunctor* consumes the matching head of a located stack.
The tail remains at the same surface.
Persistence must not recreate the consumed head, relocate the tail implicitly, or rewrap a continuation to create funding.

The first paper's `def:conservative-demand` requires sufficient funding for every reachable branch of a certified operation.
Its persistence remark describes replicated processes, not an exemption from resource consumption.
The [economic policy](economic-failure-policy-decisions.md) adds monetary backing, prepaid acquisition, and failure boundaries to those requirements.
Neither paper supplies a byte-time rent tariff or an automatic redemption price for unused rights.

## Distinct lifetimes

| Object | Lifetime | Effect of a later activation |
| --- | --- | --- |
| Stored datum or continuation | Remains until a permitted state transition removes or replaces it. | Its use can create new billable work. Persistence alone creates no new backing. |
| Introduction event | Identifies a semantic introduction and its sponsorship. | Retrying that introduction differs from introducing a new copy. |
| Firing occurrence | Identifies one completed communication, called a COMM. | A distinct firing needs distinct consumption evidence, even when it uses the same persistent contract. |
| Located prepaid right | Remains available until authorized consumption, transfer, or supported redemption. | Compatible use consumes the right without charging its acquisition again. |
| Persistent allowance | Retains one accounting lineage across executions and owner changes. | Each draw reduces available authorization. A new execution does not reset consumption. |
| Candidate reservation | Exists within the native candidate or proof lifecycle. | It cannot become an independently spendable balance or persistent deployment escrow. |
| Captured obligation | Remains attributable until the applicable settlement and replay requirements are satisfied. | New ownership cannot alter its source, terms, or refund destination. |
| Historical evidence | Remains available according to existing replay and storage requirements. | Local reclamation cannot change an accepted economic effect. |

Do not use one identifier for all these objects.
An execution scope, installation, grant, firing, right, and physical purse have different meanings.
Canonical aliases must preserve both logical multiplicity and shared physical backing.

## Installation and repeated execution

Installing a persistent process pays its applicable introduction charge under authenticated sponsorship.
The installer does not thereby authorize unlimited future draws from that purse.
Later activations must identify their own execution scopes, matching resources, available backing, and applicable grant terms.
An activation can use a funded slot or an authorized persistent allowance.
It cannot silently substitute the current deployer's purse for missing continuation authority.

Each distinct completed firing incurs its applicable interaction, delivery, and trace obligations.
Repeated observation of one persistent introduction does not multiply its introduction charge within that accounting scope.
Distinct nonpersistent introductions keep their multiplicity.
An explicit new installation remains a new introduction even if its source text matches an earlier installation.

A stored process can remain unfired when it lacks a required local resource outside a certified atomic operation.
That blocked interaction creates no COMM charge.
A certified atomic operation must cover its required continuation work before acceptance.
Persistence cannot justify accepting an insufficient certificate or billing work that never occurred.

For example, a persistent receiver can remain stored after two firings.
Both firings need their own authorized resource consumption and byte evidence.
The stored receiver is not a copy of either consumed resource.
If only one firing has funding, persistence does not authorize the second firing.

## Prepaid liability and cross-deploy custody

A prepaid right must retain its authority, location, remaining quantity, compatible resource terms, and authenticated backing provenance.
Physical custody also includes its network, asset, and purse role.
The [identity contract](authority-custody-identity-contract.md) keeps those dimensions separate from logical authority.
A raw signature stack or a wallet balance alone does not establish the complete prepaid obligation.

Compatible prepaid rights discharge matching obligations before the planner acquires new funding.
Their captured acquisition terms remain in force for that acquisition.
Later uncovered work follows its applicable schedule and signed price consent.
An incompatible right requires an authorized supported conversion or migration, or its use must be rejected.

Available-right transfers move only the authorized portion.
They preserve consumed history and cannot transfer an existing reservation's refund destination.
Deposits into the same purse increase backing, not the allowance, signed price ceiling, or an existing certificate's exposure.
Withdrawal cannot remove backing that the applicable retained state still requires.

Let $`Q`$ be issued allowance, $`A`$ available allowance, $`R`$ reserved allowance, and $`C`$ consumed allowance.
Within one compatible unit, the persistent lineage must preserve:

```math
A+R+C=Q.
```

Transfer does not change these quantities.
An explicitly authorized expansion increases issued and available allowance together.
Delegation partitions availability rather than copying it.
This equation concerns permission. Separate physical conservation must prove that grants and prepaid rights do not count the same backing twice.

For example, a five-unit allowance consumes two units before an ownership transfer.
The replacement owners receive three available units, not five.
A purse top-up leaves those three authorized units unchanged unless an explicit allowance amendment also succeeds.
Any earlier refund still follows its captured source custody.

## Release, removal, and failure

| Transition | Required economic result |
| --- | --- |
| Consume a stored nonpersistent datum | Retain authorized realized charges. Do not refund its historical introduction merely because the datum disappeared. |
| Fire a persistent continuation | Consume the firing's assigned resources once. Retain the persistent continuation without recreating consumed resources. |
| Remove application state | Apply the authorized state transition. Removal alone neither redeems prepaid rights nor creates a monetary credit. |
| Leave prepaid resources unused | Preserve those available rights and their provenance. Do not convert them automatically into cash refunds. |
| Release unused candidate funding | Return unused backing through the captured original source and asset path. Close the obligation once. |
| Redeem an available right | Require an explicitly supported, authorized redemption transition and its backing proof. Otherwise reject. |
| Abort provisional execution | Discard its provisional effects without restoring a stale snapshot over another committed operation. |
| Retain a classified user failure | Apply the approved failure projection consistently to rights, billable work, backing, allowance, and receipt. |
| Encounter a platform or certificate failure | Publish no partial economic effect. Restore the candidate pre-state or fail closed if restoration fails. |
| Reclaim local memory or storage | Preserve the same accepted economic state and required replay evidence. Reclamation does not authorize refunds or renewed allowances. |

Resource removal and funding release are different operations.
A funding slot can outlive the process that introduced it.
The end of one deployment does not prove that a retained slot or right is unreachable or safe to delete.
This contract does not authorize a new reachability algorithm or a change to finality retention.

Failure handling must distinguish rights that existed before execution from rights acquired or transferred during execution.
It must not restore a consumed prepaid right while retaining credit for that same consumption.
It must also preserve unused rights and original-source refunds.
Mixed histories require a joint application and economic projection, not independent rollback rules that can disagree.

## Native implementation boundaries

The [COMM observer](../../../../rholang/src/rust/interpreter/rho_runtime.rs) identifies persistent authority regions from matched data and continuations.
[`instantiate_persistent_regions`](../../../../rholang/src/rust/interpreter/accounting/authority.rs) derives occurrence-specific region identities from the original region and COMM identity.
This distinction prevents repeated firings from being treated as one installed authority occurrence.
It does not alone establish funding, unique native event delivery, or settlement correctness.

The [runtime budget](../../../../rholang/src/rust/interpreter/accounting/mod.rs) deduplicates persistent introductions within its current accounting scope.
It clears that set during budget reset.
The regression `persistent_introduction_identity_is_reset_between_deploys` explicitly exercises that reset behavior.
Thus, the in-memory set is not a lifetime installation registry or evidence of cross-deploy payment.

Integration must distinguish reconstruction of retained state from a new semantic introduction.
It must neither rebill a stored installation merely because a runtime restarts nor suppress a genuinely new introduction.
The reset behavior alone does not establish a reachable billing defect.
Tests must exercise the actual RSpace storage and observer paths before proposing a correction.

The current `CostStack` carries signatures, not the complete acquisition-price and redemption-provenance record required above.
Existing persistent-region and allowance primitives do not establish that complete native backing lifecycle.
The contract therefore specifies required refinement, not a claim that all release and redemption operations already exist.

## Verification and regression obligations

| Invariant | Required model and native regression |
| --- | --- |
| No replicated funding | Fire a persistent process repeatedly. Detect reused resource heads, copied allowance, and omitted occurrence identity. |
| Installation differs from reconstruction | Compare new installation, fixed-point retry, later activation, restart, and historical replay. Check actual introduction sponsorship and charge counts. |
| Every distinct firing remains billable | Generate persistent and nonpersistent matches with equal payloads and distinct causal occurrences. Detect omitted or duplicated consumption. |
| Prepaid acquisition is not charged twice | Acquire before execution, change current prices, transfer owners, then consume compatible rights. Charge only uncovered obligations. |
| Retained rights preserve provenance | Retain, transfer, partially consume, top up, and release across deployments. Reject asset, location, authority, and version substitutions. |
| Removal does not mint refunds | Remove application state or reclaim a local runtime. Detect an unsupported monetary credit or recreated right. |
| Backing and permission remain distinct | Race grants that share a physical purse. Interleave wallet top-ups, withdrawals, allowance amendments, and reservations. |
| Settlement closes once | Race two settlement or release attempts against one obligation. Check exact original-source refunds and unchanged state after rejection. |
| Failure preserves one economic history | Vary preexisting and newly acquired rights, transfers, consumption, and each failure boundary. Detect restored-right and retained-charge duplication. |
| Scope reset requires quiescence | Exercise completion, reset, and outstanding observer work. Establish the runtime's quiescence precondition instead of assuming it from a budget unit test. |
| Replay reconstructs the same liability | Compare independently executed validators and cold replay with different cache and local reclamation histories. |

`VaultBackedByteAccounting.v` proves arithmetic properties for persistent introductions and repeated deliveries in an abstract event trace.
Its lifecycle theorem does not prove the complete native event-identity or persistence path.
`PersistentFundingAllowance.v`, `ConsentedFundingAllowance.v`, and `CanonicalCustodyAliasing.v` supply separate quantity, consent, and custody obligations.
Their composition still requires authenticated resources and native publication refinement.

TLA+ models must include competing workers, shared backing, stale preparation, transfer, release, failure, and retry.
Independent resources must remain concurrent. Do not enforce correctness by serializing all validators or all purses.
Property tests need independent lifecycle and backing oracles with explicit generated bounds.
Loom tests must exercise native synchronization boundaries, including persistent introduction races and settlement closure.
Integration tests must retain state across deployments, restart runtimes, and independently replay accepted results.
Finite checks and partial proofs do not establish unbounded production uptime or complete storage-lifetime correctness.
