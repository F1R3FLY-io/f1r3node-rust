# Multi-wallet funding path audit

## Status and scope

This source audit covers selected funding paths at commit `9e58cf211885b2cdb319e030efa2af0491d71471` on September 9, 2026.
The audit does not establish complete multi-wallet support or authorize a consensus change.
Casper changes remain subject to the user's upstream-review restriction.

The local `dev` worktree was clean at `cdf447ac18710d9702a27379bce6c946f421be46` during the subsequent provenance check.
Its Rust sources do not contain the inspected `state_bound_capacity_signatures`, `fee_authority_event`, or `certify_state_bound_admission` paths.
Its runtime still uses precharge, refund, and `deploy.data.phlo_limit`.
This evidence does not classify the demonstrated state-bound defects as blockers shared with `dev`.
The check used the local reference without a new remote fetch.
Changes to these cost-accounting paths would affect settlement and replay, even without changing voting or fork choice.

The September 9 implementation had two distinct allocation paths.
The structural reservation helper uses a binary group shape.
The state-bound execution path uses authority events and a physical settlement search.
It is incorrect to infer a universal two-wallet limit from the structural helper alone.

## September 10 source refresh

This refresh inspects the working tree based on the same commit, with subsequent uncommitted campaign changes.
The September 9 findings below remain historical evidence, not descriptions of every current path.
This refresh does not claim that the entire audit or integration verification is complete.

| Path | September 10 source evidence | Remaining obligation |
| --- | --- | --- |
| State-bound monetary fee | `plan_fee_from_cursor` requests one total unit from an authenticated `MonetaryCohort`. `required_fee_plan` rejects a fee allocation whose sum differs from one. | Verify every admission, execution, replay, and failure path against the same fee contract. Do not carry the historical multiplied-fee observation forward as current behavior. |
| Fee cursor publication | Execution attaches the recorded cursor transition to `ApplyCostDeploy` before the native vault application. | Retain atomic cursor and balance publication through success, rejection, replay, and merge. |
| Fee replay | Replay obtains the captured cursor, recomputes the one-unit fee plan after resource debits, and compares both evidence and allocation. | Source correspondence does not replace end-to-end runtime evidence. |
| General restricted funding | `solve_funding_feasibility` implements integer-flow feasibility and is publicly re-exported. The Rust-source search found no external production caller. | Implement and verify the complete minimax optimizer and connect it to authenticated resource obligations and native settlement. |
| Monetary fairness objective | The approved restricted objective is lexicographic minimax. The capped allocator remains applicable to certified all-to-all scopes. | Prove that reduction and specify restricted ties. Do not apply an aggregate equal split to incompatible obligations. |
| Signed phlo controls | `CasperMessage.proto` still reserves `phloPrice` and `phloLimit` and their former tags. | The signed price and limit restoration remains incomplete. Solver helper limits do not supply client consent. |

Current source references:

- [Monetary fee planning and checks](../../../casper/src/rust/util/rholang/acceptance/monetary_fee.rs)
- [Execution and vault application](../../../casper/src/rust/rholang/runtime.rs)
- [Fee recomputation during replay](../../../casper/src/rust/rholang/replay_runtime.rs)
- [Restricted funding feasibility](../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_feasibility.rs)
- [Native fee integration tests](../../../casper/tests/util/rholang/multi_payer_fee.rs)
- [Approved minimax contract](cost-accounting-impl/lexicographic-minimax-funding.md)

The integration test source includes `state_bound_joint_deploy_pays_one_total_fee_through_vault_and_replay` and repeated-root fee rotation checks.
This refresh inspected those tests but did not rerun them.
The 110 passing funding-kernel tests do not establish that these Casper integration tests pass on the current tree.

The following digests identify the inspected source contents independently of the unchanged base commit.

| Repository path | SHA-256 |
| --- | --- |
| `casper/src/rust/util/rholang/acceptance/monetary_fee.rs` | `f368cedb93cf56baad560922b8767434c790d43a4a698d92bf4f60de0522c27b` |
| `casper/src/rust/rholang/runtime.rs` | `c1a240979ace51c93de82a68cf21c857811efa26df36a4b24b55deef7aae0a14` |
| `casper/src/rust/rholang/replay_runtime.rs` | `bea2e968c07bdd03cd0aa9386116c537fe36c3a8f1311687b4d75233e3bc381e` |
| `rholang/src/rust/interpreter/accounting/monetary_allocation/funding_feasibility.rs` | `0341c841cf633f9e8db31c78430d4922336640844672c84ee248d9251ee28477` |
| `models/src/main/protobuf/CasperMessage.proto` | `0609d692dd7c5232a52b2d8b3e8835ab3c6cfbbca2c23e66a268df5369a2aec1` |

### Native funding entry points

The following inventory separates callable contracts, runtime operations, and planned economic behavior.
An **authorization key** is a capability that permits a specific vault operation.
The **system authorization token** permits privileged protocol operations.
Neither capability is a wallet balance or a conversion rate.

| Entry point and caller | Authorization and retained effect | Audit boundary |
| --- | --- | --- |
| Genesis parameter construction → `SystemVault.init` | Genesis combines initial general balances, client allocations, and validator fuel. Checked additions reject overflow. | This is initial issuance, not a user deposit during ordinary execution. General custody and validator fuel remain distinct. |
| Genesis PoS initialization → `createWithBalance` | A single-use receive accepts the fixed PoS vault address and creates its initial bond balance. | The receive does not check a system token. Safety depends on genesis consuming it before ordinary deploy admission. |
| Public `findOrCreate` | A valid absent address receives a vault with zero general balance and zero validator fuel. | Creating a wallet address does not create spendable value. |
| Vault `transfer` → `_transferTemplate` | The source authorization key must match its vault. Positive transfers move existing purse value to a valid target address. | Repeated deposits can refill the same target vault. The transfer does not assign process funding consent. |
| Vault `transferBatch` | Source authorization precedes validation of distinct targets, positive amounts, and a bounded total. The source purse supplies that total. | The contract distributes a list, not only two entries. Later deposit errors need failure-path evidence, not an assumed all-or-nothing property. |
| Vault `fundValidatorFuel` → `_fundValidatorFuelFrom` | Source authorization and a positive amount precede movement from general custody into an available target validator-fuel purse. | This is a custody-role transfer. It does not implement arbitrary assets or wallet-specific exchange rates. |
| Privileged `protocolMint` and `protocolMintValidatorFuel` | A valid system token and positive amount permit minting into the designated custody role. | Rust `ProtocolMintDeploy` exists, but the source search found no external production caller of that wrapper. |
| PoS epoch processing → `mintPhlogiston` | PoS calls validator-fuel minting. Epoch progression and halted-validator checks constrain issuance. | This reachable issuance path differs from the currently unused general mint wrapper. |
| Privileged `protocolBurn` | A valid system token permits withdrawal from general custody. The contract discards the resulting purse capability. Zero is a no-op. | The Rust wrapper requires a positive amount. The source search found no external production caller of that wrapper. |
| `rho:lang:exchange` → `Exchange.swap` | The contract consumes one datum from each supplied carrier and swaps the payloads. | It does not call SystemVault. It is not an implemented priced REV-to-resource acquisition or redemption path. |
| Normalizer → signed terms and token stacks | `recognize_signed_term` applies lollipop desugaring. `recognize_token_stack` emits the declared cell sequence. | Normalization records syntax. It does not independently authorize a debit or certify backing. |
| Reducer → `prepare_authority_stack_transfer` | Metered stack production prepares checked authority demand and unique birth records before producing the datum. Commit follows successful production. | Preparation, commit, and abort bookkeeping must agree with retained tuplespace state. Syntax alone must not mint funded resources. |
| Execution and replay → `apply_stack_pops` | The runtime checks stack identity, contents, and pop count. It removes selected prefixes and releases tails under a soft checkpoint. | Failure restores that local checkpoint. Whole-deploy atomicity still requires the enclosing execution and replay paths. |
| Runtime → `ApplyCostDeploy` → `SystemVault.applyCost` | A system token authorizes list-based reservation and settlement. Fee-bearing calls require a cursor transition. | The contract checks supplied amounts. It does not calculate minimax shares or establish signed payer consent. |
| `reserveAll`, `refundReserved`, and `settleReserved` | Reservation splits source purses. Settlement returns unused value to its source address and role and transfers the specified fee. | Refund failures return errors. Do not claim universal recovery from the happy path or from list support alone. |
| PoS `redeemSlashed` → custody disposition and resolution | A system token, verified multisignature flag, matching generation, and quarantine state constrain validator redemption. | This is validator lifecycle adjudication, not generic redemption of located resource tokens for wallet funds. |
| Numeric state merge → authority merge | Numeric merge combines attached authorities and preserves the resulting metadata in the output datum. | Numeric value conservation alone does not prove funding authorization. The funding-correspondence properties remain a separate obligation. |

#### Mint identity and validator custody paths

The native handler charge is separate from the one-unit general-custody client fee.
`VALIDATOR_HANDLER_COST_PER_DEPLOY` is three.
The runtime reserves and burns those units from the proposer's validator-fuel purse for a settled handler.
Client payer count does not determine this handler charge.

| Path | Authorization and retained effect | Audit boundary |
| --- | --- | --- |
| Public `MakeMint` factory | Each call creates a distinct mint capability. Possession permits purse creation within that mint. Deposits check mint identity. | Creating another mint cannot create funds accepted by SystemVault's private mint instance. This is not a native conversion route. |
| Proposer handler admission and settlement | The runtime checks current proposer fuel before candidate execution. Settlement includes a three-unit `ValidatorFuel` reservation and burn. | Insufficient fuel defers candidates. Candidate-created top-ups cannot bypass the initial check. This cost is not shared among client signers. |
| Proposer handler replay | Replay checks captured pre-state fuel and supplies the corresponding role-tagged reservation and settlement. | Capture must bind the correct root, proposer, balance, and runtime phase. A live query during active replay is not equivalent. |
| PoS bonding | Authenticated deployer identity selects the payer. A bounded positive bond transfers existing general custody into PoS before bond state changes. | Bonding does not issue validator fuel. Existing lifecycle and generation checks still apply. |
| PoS withdrawal | Authenticated withdrawal records a pending lifecycle. Later close-block processing transfers eligible stake and rewards to the validator vault. | The withdrawal request itself is not immediate payment. Transfer failure must preserve the corresponding claim. |
| PoS human-control transfer | `posVaultTransfer` delegates to a handle that checks the configured public key through deployer identity and obtains an unforgeable vault authorization key. | This privileged control of the PoS vault is separate from ordinary process funding and validator-fuel withdrawal. |
| Slash quarantine | PoS checks the system token, invalid-block evidence, and bond generation before privileged fuel quarantine. The vault retains removed fuel in a quarantine purse. | Quarantine makes that fuel unavailable without converting it into client general custody. General wallet custody remains separate. |
| Vindicated redemption | PoS requires authorized adjudication for the recorded quarantine generation. Stake disposition moves no stake. Fuel resolution restores quarantined fuel. | Lifecycle restoration does not issue replacement fuel or reimburse unrelated process sponsors. |
| Guilty redemption | PoS requires a nonnegative penalty strictly below quarantined stake. Stake disposition transfers the penalty. Fuel resolution transfers a bounded penalty and restores the remainder. | The fuel penalty enters the penalty recipient's `General` custody. This privileged adjudication is not a public fuel-withdrawal route. |
| Burned redemption | Stake disposition splits and discards the specified stake-and-reward purse. Fuel resolution discards the quarantined fuel purse. | The burned lifecycle blocks ordinary rebond. These operations are not resource-token redemption for wallet funds. |

PoS applies stake disposition before fuel resolution.
Stake disposition records generation and outcome parameters to reject conflicting retries and avoid repeated effects.
Fuel resolution checks its stored generation and outcome parameters for exact retries.
Restoring one contract state variable cannot prove atomic rollback across both custody operations.
The enclosing runtime checkpoint and native failure regressions remain necessary evidence.

The mint identity boundary is implemented in [MakeMint](../../../casper/src/main/resources/MakeMint.rho).
SystemVault creates and retains its own mint instance before it creates vault purses.
The [validator handler constant](../../../casper/src/rust/util/rholang/costacc/mod.rs), execution path, and replay path define the separate handler debit.

The lollipop implementation assigns the interaction to the source authority and its continuation to the destination authority.
It does not impose a two-transfer lifetime limit in this desugaring function.
This observation does not prove arbitrary ownership histories with the planned price limits and persistent allowances.
Those controls require separate signed-state and settlement integration.

The audit did not identify a complete native buy–Split/Join–redeem cycle with the proposed one-unit-per-cell monetary valuation.
Therefore, the conditional counterexample in the design review remains a design constraint, not a demonstrated native exploit.
Structural regrouping and carrier transport must not be described as priced vault conversion without an identified caller and retained custody effect.

#### Operator and capability boundary

The funding grammar and capability operations are different interfaces.
The following matrix records their native boundaries without claiming complete monetary integration.

| Constructor or operation | Native boundary | Funding consequence |
| --- | --- | --- |
| `Unit`, `Ground`, `Quote`, and recursive `And` | `Sig::is_funding_former` recognizes these constructors. Stack production separately rejects unit cells. | Recursive `And` permits arbitrary tree shape. This grammar check does not establish arbitrary-payer allocation correctness. |
| Threshold authorization | Authenticated envelope selection chooses actual signers. `Sig::Threshold` is not a funding constructor. | Only selected verified signers contribute funding authority. Placeholder members must not supply capacity. |
| Surface lollipop | The normalizer rewrites a transfer-signed receive into source-authorized interaction and destination-authorized continuation. Raw transfer signatures are not ordinary wire funding signatures. | Transfer syntax does not itself debit wallets, establish a price, or authorize a new persistent allowance. |
| `Plus`, `With`, `Bang`, `WhyNot`, and value-level `Lolly` | `is_funding_former` rejects these value/capability constructors as funding signatures. Admission checks the grammar. | Their type-level presence does not make them funded purse keys. Do not remove this distinction to generalize wallet counts. |
| Capability registration | Genesis installs `CapabilitiesRegistry`. Registration derives the delegator identity, obtains a nonce, and stores the transformer and usage metadata. | Registration does not validate a priced monetary acquisition or mint SystemVault backing. |
| Capability invocation | The registry rejects unknown, revoked, or exhausted handles. It updates a bounded usage count and calls the registered transformer. | Invocation uses the handle and supplied channel. It does not itself check SystemVault balances or prove the transformer's resource conservation. |
| Capability revocation | The registry compares the supplied deployer identity with the stored delegator. Successful revocation updates the entry. | Revocation controls future invocation. It is not a wallet refund or retroactive reversal of completed funding. |
| Capability lookup | The registry returns stored metadata for a known handle. | Metadata visibility does not establish ownership of wallet funds or authorize their withdrawal. |

The registry's stored source and destination signatures do not establish that the transformer implements those signatures correctly.
Invocation publishes its usage update before it waits for the transformer result.
Any claim about failure rollback must include the enclosing deploy semantics, not only the registry's map update.

Detailed source references:

- [Genesis balances](../../../casper/src/rust/genesis/genesis.rs)
- [Vault operations and settlement](../../../casper/src/main/resources/SystemVault.rho)
- [PoS issuance and redemption](../../../casper/src/main/resources/PoS.rhox)
- [Carrier exchange](../../../casper/src/main/resources/Exchange.rhox)
- [Privileged deploy wrappers](../../../casper/src/rust/util/rholang/costacc/vault_cost_deploy.rs)
- [Signed-term and stack normalization](../../../rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/recognize.rs)
- [Lollipop desugaring](../../../rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/desugar.rs)
- [Funding signature normalization](../../../rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/sig.rs)
- [Callable capability registry](../../../casper/src/main/resources/CapabilitiesRegistry.rhox)
- [Stack transfer accounting](../../../rholang/src/rust/interpreter/accounting/mod.rs)
- [Stack production](../../../rholang/src/rust/interpreter/reduce.rs)
- [Stack settlement](../../../casper/src/rust/util/rholang/supply.rs)
- [Numeric merge implementation](../../../rholang/src/rust/interpreter/merging/rholang_merging_logic.rs)
- [Numeric merge funding correspondence](cost-accounting-impl/numeric-merge-authority-preservation.md)
- [Valuation and conversion design boundaries](cost-accounting-impl/funding-settlement-design-review.md)

| Additional inspected source | SHA-256 |
| --- | --- |
| `casper/src/main/resources/SystemVault.rho` | `df180147d47ee9d92d7860baf4848cbd188368c5322666d4e9b2d66ba74eed7b` |
| `casper/src/main/resources/Exchange.rhox` | `972b2a712aeea7f886eca53840aacec1f8861f01f4425f20dbeb0b6134554241` |
| `casper/src/main/resources/PoS.rhox` | `bacefccc66db7d8fcd617bb7bfe6d29a1c91e7e02eed6cdb212a5e27435b1172` |
| `rholang/src/rust/interpreter/accounting/mod.rs` | `e0abda8c09acfe9e3f6dadd37c7414372464b6575c4f06993c0208174fa8aad6` |
| `rholang/src/rust/interpreter/reduce.rs` | `265eb7f5a03600580e32746bf993d477a732e25aea1a62f5a8a1a32e910ba589` |
| `casper/src/rust/util/rholang/supply.rs` | `72ebfe246e7c9411edfe5bda5480d1126248ff0e855fe80ba71d0b0228a3309f` |
| `casper/src/rust/genesis/genesis.rs` | `496a1c15cd42743518f138d7ddfbf76c1d0999beb1d5716ae0cdd6af770a4575` |

This inventory is source-review evidence, not an end-to-end verification receipt.
The separate proof/test audit must map each retained effect and each failure boundary to an independent invariant and executable regression.

### Proof and test coverage refresh

This audit distinguishes a general theorem from a bounded test of that theorem's implementation.
A parameterized model does not establish correctness for all parameter values when only one finite configuration was checked.
A **shadow model** substitutes test state and synchronization for production state.
Its result needs an explicit correspondence argument before it can support a claim about the runtime.

| Evidence | Generality or inspected bound | What the evidence does not establish |
| --- | --- | --- |
| `CompoundSettlement.tla` | The state explicitly represents one compound pool, two component pools, and a second group sharing one component. | This legacy focused model does not establish arbitrary nested funding or the approved monetary split. |
| `AuthorityResourceValuation.v` | Theorems quantify over signature trees, lists, and arbitrary finite regrouping histories. Component values are additive at a fixed unit price. | These theorems do not implement resource acquisition, changing tariffs, monetary redemption, or native custody correspondence. |
| `EligibleFundingAssignment.v` | Assignment checking quantifies over arbitrary natural-number source and obligation counts. It checks eligibility, source capacity, and exact obligation funding. | The theorem assumes the supplied eligibility and capacities. It does not authenticate them from wallet state. |
| `LexicographicMinimax.v` | Candidate selection minimizes descending contribution vectors. Equal-ranked optima need not identify the same payer allocation. | Optimality over supplied candidates is not global optimality without complete candidate coverage. It does not select the residual tie policy. |
| `CompleteFundingCandidates.v` | Rectangular tables enumerate bounded assignments for arbitrary finite dimensions. Valid entries fit the total-demand bound. | Exhaustive enumeration is a specification construction, not a production performance guarantee. |
| `FundingFifoSearch.v`, `FundingParentEdges.v`, and the residual-graph proof chain | The reference breadth-first search derives a simple positive path or a closed reachable set. Parent operations connect paths to residual updates. | Manual correspondence and selected shared Rust helper proofs do not prove the complete native optimizer or its callers. |
| `RotatingMonetaryAllocation.v` | List-based allocation proves exact totals, no overdraw, residual bounds, and refunds under the stated capacity premises. | The all-to-all contract does not solve restricted source-to-obligation assignments. |
| `RotatingMonetaryAllocation.cfg` | The inspected configuration has four payers and capacities from zero through three. | A bounded model-checking result cannot establish all payer counts or machine-integer behavior. |
| `RotatingFeeSettlement.cfg` | The configuration has three payers, two workers, two transactions, and capacity bound two. | The abstract atomic transition does not by itself establish native cursor, vault, and checkpoint publication. |
| `FundingPriceConsent.v` | The required ceiling checks every element of an arbitrary consent list. Its effective ceiling is the minimum of required maxima. | It does not restore signed wire fields or prove that a runtime collected every required consent. |
| Persistent allowance and consent-publication models | The inspected configurations use three workers and two grants or rights. Consent publication permits one attempt per worker. | These finite explorations do not establish unrestricted ownership histories or native wallet integration. |
| Native capped-allocation properties | Generated capacity lists contain one through 64 payers. Some properties use the full `u64` range. | These properties cover the capped allocator, not an implemented general minimax optimizer. |
| Native feasibility properties | A complete independent oracle checks three sources and up to three obligations. Other properties generate up to 128 sources. | Large certificate checks and small complete-oracle comparisons have different strengths. Neither proves optimizer fairness. |
| Native branch reservation properties | Generated fixed plans contain one through 32 sources and one through eight branches. The oracle checks per-source maxima and refunds. | These checks do not establish client authorization for the larger temporary hold that mutually exclusive restricted branches can require. |
| Native lollipop desugaring properties | Generated authorities contain one through eight components. Receive forms use one through eight bindings. Explicit and sugared forms must have identical syntax trees and bytes. | Compiler equivalence does not establish runtime funding consent, retained balances, or arbitrary ownership-transfer histories. |
| Native cursor history properties | Generated histories use two worker slots, three scopes, one through 65 payers per scope, and up to 95 operations. | The property executes an explicit interleaving sequence. It does not execute concurrent native vault mutation. |
| `atomic_cost_request_is_permutation_invariant` | The property generates amounts for exactly two allocation entries and reverses their order. | It does not cover arbitrary list permutations, repeated custody aliases, or all failing reservation positions. |
| Native joint-fee integration source | Cases cover one, two, and three selected signers, plus two selected members of a three-member envelope. | These examples do not establish every payer count or the complete phlo restoration. This refresh did not rerun them. |
| `loom_multi_sig_fanout.rs` | Two commit workers and an optional revert worker operate on a mutex-protected shadow PoS map. | This legacy precharge model is not proof of current native SystemVault settlement or arbitrary-payer concurrency. |
| `loom_vault_byte_reservation.rs` | Shadow counters cover charge races, top-up isolation, and persistent-introduction idempotence. | It does not connect restricted allocation, signed consent, fee cursor publication, and native multi-wallet custody. |

#### Existing stack and operator evidence

The following evidence supplements the monetary-allocation inventory.
These models and regressions already exist. Their presence does not establish complete native multi-wallet semantics.

| Evidence | Premises, bounds, and checked behavior | Remaining boundary |
| --- | --- | --- |
| `StackTransferConservation.v` | A fresh event and sufficient source cells permit exact transfer. Duplicate and underfunded transfers preserve the ledger. | The ledger has scalar source and target counts. It does not identify every native wallet, role, or conversion path. |
| `StackIntroductionAtomicity.v` | One pending introduction and unrelated committed cells model preparation, visibility, commit, abort, and enclosing-deploy rollback. | Arbitrary cell counts do not imply arbitrary concurrent native reservations. Retained byte charges need the runtime correspondence. |
| `LocatedStackConservation.cfg` | The safe fixture uses two stack cells and initial source capacity four. Defect fixtures exercise duplicate identity, partial debit, and replay omission. | These finite scenarios do not establish all stack sizes or arbitrary payer graphs. |
| `StackIntroductionAtomicity.tla` and its configurations | The model explicitly assumes two operations. The safe fixture has initial capacity three. Controls cover early visibility, missing abort, missing rollback, and replay omission. | The model cannot stand as an unbounded concurrency proof. Increasing a constant alone cannot remove its two-operation state structure. |
| `ParallelStackMaterialization.v` and `.tla` | The model covers initial declaration, parent reduction, nested declaration, and replay. The TLA+ scenario begins with four payer units. | This proves the stated materialization scenario, not general parallel-validator behavior or monetary allocation. |
| `formal/loom/.../loom_stack_introduction_atomicity.rs` | Shadow state checks preparation, visible production, commit, cancellation, byte rejection, and enclosing-deploy rollback. Concurrent cases use two workers. | The test does not execute production RSpace, the native vault, or the entire allocation-to-publication path. |
| `formal/loom/.../loom_stack_frontier.rs` | Two-worker shadow tests cover duplicate transfer, distinct transfers sharing one source, and duplicate frontier discovery. | These examples do not establish all alias relationships or arbitrary concurrent funding cohorts. |
| Native stack-transfer property | `stack_transfer_reserves_exactly_one_authority_cell_per_output` generates one through 64 cells and zero through 64 slack units. | It exercises one authority lane and the native budget API, not complete vault settlement. |
| Native Join-partition property | `every_contiguous_partition_is_a_valid_physical_join` generates one through eight atoms and contiguous partitions. | This tests physical authority presentations. It is not a monetary equal-share oracle or every possible partition topology. |
| Native runtime stack regressions | Tests cover same-deploy transfer and replay, candidate-minted rejection, persistent Split surfaces, and cross-deploy wallet-funded lollipop execution. | These fixed examples are stronger than helper-only tests but do not establish all arities, ownership histories, or new signed phlo controls. |
| Native funding-grammar regression | `is_funding_former_accepts_funding_grammar_rejects_capability_connectives` includes a three-atom nested `And` and rejected capability constructors. | Grammar acceptance is not a proof that admission and settlement can fund every accepted tree. |
| Native capability-registry regression | `capabilities_registry_shorthand_registers_and_invokes_a_bounded_lollipop` registers a one-use transformer and invokes it. | It checks registry resolution and a concrete transform, not arbitrary resource-transformer correctness or monetary backing. |

The theorem name `authorized_mint_is_the_only_supply_increase` needs a specific qualification.
Its statement says that its abstract mint operation increases the scalar ledger total by the supplied amount.
It does not authenticate the caller or exclude every other native supply-increase path.
The native entry-point inventory supplies separate obligations for those claims.

Relevant sources:

- [Stack transfer conservation](../../../formal/rocq/cost_accounted_rho/theories/StackTransferConservation.v)
- [Stack introduction atomicity](../../../formal/rocq/cost_accounted_rho/theories/StackIntroductionAtomicity.v)
- [Located stack configuration](../../../formal/tlaplus/cost_accounted_rho/LocatedStackConservation.cfg)
- [Two-operation introduction model](../../../formal/tlaplus/cost_accounted_rho/StackIntroductionAtomicity.tla)
- [Introduction configuration](../../../formal/tlaplus/cost_accounted_rho/StackIntroductionAtomicity.cfg)
- [Materialization model](../../../formal/tlaplus/cost_accounted_rho/ParallelStackMaterialization.tla)
- [Materialization theorems](../../../formal/rocq/cost_accounted_rho/theories/ParallelStackMaterialization.v)
- [Stack introduction Loom tests](../../../formal/loom/cost_accounting/tests/loom_stack_introduction_atomicity.rs)
- [Stack and frontier Loom tests](../../../formal/loom/cost_accounting/tests/loom_stack_frontier.rs)
- [Native authority presentation properties](../../../rholang/src/rust/interpreter/accounting/authority.rs)
- [Native stack runtime regressions](../../../casper/tests/util/rholang/runtime_manager_test.rs)
- [Native funding-grammar regressions](../../../rholang/tests/accounting/ll_rejection_spec.rs)
- [Native capability-registry regressions](../../../casper/tests/genesis/contracts/capabilities_registry_spec.rs)

#### Existing custody, issuance, and failure evidence

The client fee and proposer handler cost use different custody roles.
Their proof obligations must not be combined into one payer-count formula.

| Evidence | Scope and premises | Remaining boundary |
| --- | --- | --- |
| `ValidatorEconomicsLifecycle.v` | Role-specific arithmetic covers general custody, validator fuel, stake, quarantine, located value, burns, and authorized issuance. Handler cost is three. | Abstract transition functions do not authenticate native callers or prove all contract interleavings. |
| `ValidatorEconomicsRefinement.v` | Role-tagged events relate abstract and native-named trace definitions. | The native-named definitions are formal definitions, not extracted Rust or Rholang execution. |
| `ValidatorEconomicsReplay.v` | Snapshot validity checks root, proposer, balance, ordinary-runtime origin, and capture timing. Prefix results relate selected handlers to replay. | Runtime capture and use must satisfy those premises. The theorem does not make an unvalidated snapshot trustworthy. |
| `ValidatorEconomicsLifecycle.tla` | The model fixes two validators, three proposals, handler cost three, client fee one, and six steps. Defect controls cover role substitution, top-up, stale generation, and sibling overdraw. | Its bounded checks do not establish arbitrary validators, indefinite lifetimes, or every native custody transition. |
| `EpochMintAtomicity.v` | Supplied validator eligibility and mint outcomes determine staging, publication, failure identity, retry, and replay. | The proof does not independently certify the native eligibility calculation or every callback result. |
| `EpochMintAtomicity.tla` | The model fixes three validators and one epoch. The safe configuration uses issuance amount one, with separate zero-issuance and defect configurations. | Finite epoch publication checks do not establish every lifecycle or arbitrary mint batch size. |
| `ConcurrentRedemptionCustody.tla` | Two validators, four workers, and maximum generation two cover generation-specific resolution, retries, conflicts, and lifecycle restoration. | This is a bounded custody model, not a proof of the full Casper protocol. |
| `formal/loom/.../loom_validator_economics.rs` | Shadow vault and snapshot state cover top-up, handler debit, quarantine, resolution, and stale snapshots. Sibling reservations use three workers. Other concurrency cases use two. | Shadow mutexes and atomics are not native RSpace or vault execution. They need an implementation correspondence argument. |
| Native handler regressions | Exact three-unit debit, same-candidate self-funding rejection, two handlers from six units, and active-replay query rejection are explicit tests. | These tests do not make handler fuel another client contribution or establish arbitrary-payer allocation fairness. |
| Native lifecycle regressions | Runtime cases cover mint rollback and retry, zero issuance, fresh bond without subsidy, quarantine, redemption outcomes, rebond generation, and top-up authorization. | Their presence is not a fresh passing result. Generalization still requires generated lifecycle and custody histories. |
| Native mint regressions | `MakeMintTest.rho` checks deposits, splitting, overflow, and cross-currency rejection through the mint contract harness. | A separate mint identity cannot supply SystemVault backing merely because its amounts have the same numeric representation. |

Current native failure tests distinguish retained charges from rejected operations:

| Runtime regression | Intended observation |
| --- | --- |
| `failed_state_bound_body_rolls_back_writes_and_commits_its_charge` | A failed user body restores application writes while retaining its specified charge. |
| `state_bound_reservation_failure_rolls_back_the_retained_user_execution` | A reservation failure restores the retained user execution. |
| Parser rejection case in `runtime_manager_test.rs` | Rejected syntax does not produce processed cost evidence. |
| Rejected and errored system-deploy cases in `runtime_manager_test.rs` | The surrounding checkpoint restores system effects, including protocol mint effects. |
| Settlement failure paths in `runtime.rs` | Failure resets the current root rather than publishing a partial successful settlement. |

These existing observations are implementation evidence to retain and extend, not merely future test ideas.
This refresh inspected their source without rerunning them.
They do not yet establish the new signed price, limit, allowance, or minimax contracts.

Source references:

- [Validator economics arithmetic](../../../formal/rocq/cost_accounted_rho/theories/ValidatorEconomicsLifecycle.v)
- [Role-tagged trace refinement](../../../formal/rocq/cost_accounted_rho/theories/ValidatorEconomicsRefinement.v)
- [Validator fuel replay premises](../../../formal/rocq/cost_accounted_rho/theories/ValidatorEconomicsReplay.v)
- [Bounded validator economics](../../../formal/tlaplus/cost_accounted_rho/ValidatorEconomicsLifecycle.tla)
- [Epoch publication theorems](../../../formal/rocq/cost_accounted_rho/theories/EpochMintAtomicity.v)
- [Epoch publication model](../../../formal/tlaplus/cost_accounted_rho/EpochMintAtomicity.tla)
- [Concurrent redemption custody model](../../../formal/tlaplus/cost_accounted_rho/ConcurrentRedemptionCustody.tla)
- [Validator economics Loom tests](../../../formal/loom/cost_accounting/tests/loom_validator_economics.rs)
- [Native runtime regression cases](../../../casper/tests/util/rholang/runtime_manager_test.rs)
- [Mint contract harness](../../../casper/tests/genesis/contracts/make_mint_spec.rs)
- [Mint contract assertions](../../../casper/src/test/resources/MakeMintTest.rho)

Source references for the allocation bounds:

- [Authority valuation theorems](../../../formal/rocq/cost_accounted_rho/theories/AuthorityResourceValuation.v)
- [Eligible assignment theorems](../../../formal/rocq/cost_accounted_rho/theories/EligibleFundingAssignment.v)
- [Minimax objective](../../../formal/rocq/cost_accounted_rho/theories/LexicographicMinimax.v)
- [Complete candidate construction](../../../formal/rocq/cost_accounted_rho/theories/CompleteFundingCandidates.v)
- [BFS reference](../../../formal/rocq/cost_accounted_rho/theories/FundingFifoSearch.v)
- [Parent-operation correspondence](../../../formal/rocq/cost_accounted_rho/theories/FundingParentEdges.v)
- [Capped allocation properties](../../../rholang/src/rust/interpreter/accounting/monetary_allocation/tests.rs)
- [Feasibility properties](../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_feasibility.rs)
- [Branch reservation properties](../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_reservation.rs)
- [Lollipop desugaring properties](../../../rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/sugar_properties.rs)
- [Cursor history properties](../../../rholang/src/rust/interpreter/accounting/monetary_allocation/cursor/tests.rs)
- [Legacy fanout shadow model](../../../rholang/tests/loom_multi_sig_fanout.rs)
- [Byte reservation shadow model](../../../rholang/tests/loom_vault_byte_reservation.rs)

The September 10 kernel log records 110 passing tests and no failures.
The parent-edge proof log records six assumption reports closed under the global context.
These are earlier local results, not fresh checks of the full working tree or the entire campaign.
The [minimax implementation record](cost-accounting-impl/lexicographic-minimax-funding.md) gives the proof chain and its remaining boundaries.

The following digests identify central proof statements and test sources from this refresh.

| Repository path | SHA-256 |
| --- | --- |
| `formal/rocq/cost_accounted_rho/theories/AuthorityResourceValuation.v` | `f71aeafe7d7523e46b7d57e0c4b085182af15c1372af23e89e31b61b7d63ffc4` |
| `formal/rocq/cost_accounted_rho/theories/EligibleFundingAssignment.v` | `736cc95a9f46109ccf532fa7462737e11f58a5fcca99caf1931ef93c479dde2c` |
| `formal/rocq/cost_accounted_rho/theories/LexicographicMinimax.v` | `9978c42a7c98e9ab62af04cfd6db10778539b37c19df6994c88a79514ffca4c3` |
| `formal/rocq/cost_accounted_rho/theories/CompleteFundingCandidates.v` | `d9b2d687578fdf300cd5163e787ceaeb09d0bcfc8ef0eef0ba0c430195bdb852` |
| `formal/tlaplus/cost_accounted_rho/RotatingMonetaryAllocation.cfg` | `01b694c3db06685dce80596d3600d462a4920ee514a74a804402ae9881bc8ecc` |
| `formal/tlaplus/cost_accounted_rho/RotatingFeeSettlement.cfg` | `193068164e22574213595d7a6f6f7b671640692275697898c89298f465a8862b` |
| `rholang/src/rust/interpreter/accounting/monetary_allocation/tests.rs` | `8f068852c3b349c8aba8ac979b3f88b6c7cbd3ee0d58fe8305cc29eae1458926` |
| `rholang/src/rust/interpreter/accounting/monetary_allocation/cursor/tests.rs` | `feb92f296a7d468ada0aa6dbd5f41c10777e7f5ea0a47fa3fedb96105198be31` |
| `rholang/tests/loom_multi_sig_fanout.rs` | `a47feb06cf58dbc8ead15c665da20cb45215fd829bfdde9a6c9031bf221ab7f3` |
| `rholang/tests/loom_vault_byte_reservation.rs` | `1083cbb1d46c445808092bd733905dbfcff6abec5dc6d192d2effad4b87ec464` |

Required follow-up coverage remains explicit:

1. Connect authenticated native inventories to the assignment checker's eligibility and capacity premises.
2. Prove the production minimax optimizer against complete feasible assignments, including the selected tie rule.
3. Derive generated-arity vault tests from conservation, alias aggregation, refund provenance, and failure-position invariants.
4. Connect concurrent native state publication to the consent, allowance, fee, and settlement models.
5. Test repeated ownership transfers, top-ups, revocation, stale plans, and replay within that same native contract.
6. Preserve existing bounded regressions as examples without presenting them as unrestricted end-to-end proofs.

The audit does not justify replacing independent native operations with a global lock.
Concurrency verification must test the synchronization that the implementation actually uses.

## Terms and specification boundary

An **authority** states which signatures must fund a reduction.
A **custody key** identifies the physical balance that a debit consumes.
Different logical resource keys can refer to the same custody key.
A **monetary obligation** is the amount of an asset that the payer set must pay in total.

The rho paper's `prop:join-conservation` preserves the multiset of participating authorities.
Its `eq:reverse-curry` permits complete regrouping of compound tokens.
The subsequent weakening restriction forbids discarding an authority component.
These rules do not equate the number of physical cells with a fixed monetary obligation.

The monad paper's `eq:R1` consumes a matching token from a located purse when it forces a wrapped redex.
It preserves the signatures on the resulting wrapped continuations.
A monetary allocation must not replace this authority check with a sum of unrelated wallet balances.

The [three-paper audit](cost-accounting-three-paper-traceability.md#specification-inputs) identifies the inspected paper revisions.
The source labels above are in `cost-accounting/cost-accounted-rho.tex` and `cost-accounting-as-monad/continued-gslt-cost-v2.tex`.

## September 9 paths and required dispositions

The table separates observed behavior from required follow-up work.
It does not classify all uninspected paths as correct.

| Path | Current evidence | Required disposition |
| --- | --- | --- |
| Signer identity | `funding_sig` selects authenticated funders. `funding_sig_compound` constructs a left-associated `And` tree. | Preserve signer authentication. Do not infer monetary shares from tree depth or signer count. |
| Structural reservation | `group_shape_from` reads the compound balance and its two immediate component balances. `group_capacity` uses the minimum component balance. | Replace the binary economic assumption when the new solver contract is approved. Determine every production caller before removal. |
| Structural settlement | `compute_settlement_debits` invokes `DefaultApportionment` for each demand-bearing group. Nested decomposition entries do not recursively create draws. | Keep admission and settlement consistent during replacement. Do not merely remove the three-signer rejection test. |
| State-bound compute settlement | `allocate_physical_settlement_with_host_work` expands authority atoms and searches physical balances and stacks. | Preserve complete authority consumption and custody checks. Add a separate monetary allocation contract. |
| Transferred bytes and introduction bytes | `allocate_quantitative_events_with_custody` scales authority funding options by each byte-event amount. | Separate measured resource units, authority requirements, and monetary prices. Preserve exact replay inputs. |
| Structural fee | `FlatFeeApportionment` draws from the combined pool, then one immediate component. | Replace canonical-first bias with the approved residual policy. Preserve the total fee obligation. |
| State-bound fee | `fee_authority_event` uses the full funding signature. The actual vault regression observes recipient credit of one unit per selected signer. | Reconcile this path with the fixed monetary fee requirement. Preserve the regression as an independent monetary oracle. |
| Custody aliases | `physicalize_balance_debit` sums logical debits by physical custody with checked arithmetic. | Preserve alias aggregation and no-overdraw checks. Alias aggregation does not itself calculate fair shares. |
| Vault settlement | `runtime.rs` passes computed burn and fee amounts into `VaultSettlement`. `canonical_settlements` aggregates by address and role. | Preserve reservation bounds and refund provenance. Do not expect the vault layer to repair allocation policy. |
| Replay | `recompute_authority_settlement_debits` verifies compute draws and recomputes byte and fee draws. | Require the same monetary plan on execution and replay. Equality alone does not prove economic correctness. |
| Signed phlo controls | Protobuf reserves the removed names and tags. `DeployData` no longer has the former price and limit arithmetic. | Restore signed limits and price semantics through the planned wire and signing tasks. Do not treat helper budgets as client consent. |

Source files:

- [Funding signatures](../../../rholang/src/rust/interpreter/accounting/mod.rs)
- [Binary policies](../../../rholang/src/rust/interpreter/accounting/resource_logic.rs)
- [Admission and replay allocation](../../../casper/src/rust/util/rholang/acceptance.rs)
- [Authority allocation and custody](../../../rholang/src/rust/interpreter/accounting/authority.rs)
- [Byte measurements](../../../rholang/src/rust/interpreter/accounting/byte_accounting.rs)
- [State-bound execution](../../../casper/src/rust/rholang/runtime.rs)
- [Runtime admission entry points](../../../casper/src/rust/util/rholang/runtime_manager.rs)
- [Vault settlement](../../../casper/src/rust/util/rholang/costacc/vault_cost_deploy.rs)
- [Deploy wire format](../../../models/src/main/protobuf/CasperMessage.proto)
- [Deploy information wire format](../../../models/src/main/protobuf/DeployServiceCommon.proto)
- [Deploy data and signing](../../../models/src/rust/casper/protocol/casper_message.rs)

## Historical findings before the September 10 fee repair

This section preserves the original findings and their supporting evidence.
The September 10 refresh controls statements about the current implementation.
In particular, NPA-3 records the earlier multiplied fee, not the current one-total-unit fee rule.

### NPA-1: The binary restriction is path-specific

`nary_nested_compound_absent_inner_pool_rejected` funds three individual signer pools but no intermediate compound pool.
The test expects the structural gate to reject the three-signer deploy.
This records a limitation of that helper, not a complete test of current state-bound execution.

A workspace Rust-source search found no external production caller of the named structural helpers.
The observed calls are wrappers and tests within `acceptance.rs`.
Comments elsewhere still describe those helpers as the production path.
This textual search does not prove that external consumers or generated code never use their public interfaces.

The inspected runtime entry point calls `certify_state_bound_admission`.
The runtime then uses `prepare_state_bound_authority_reservation_with_host_work` for each candidate.
Therefore, changing the binary helper alone would not repair the inspected state-bound monetary allocation path.

The physical allocator has a separate four-authority example.
`physical_presentation_accepts_an_arbitrary_join_partition` supplies two compound pools, each with two authorities.
Both the verifier and the allocator accept the presentation.
That test passed in this audit.

The replacement audit must distinguish obsolete helper assumptions from necessary authority constraints.
Removing the complete authority check would violate the papers rather than generalize wallet funding.

### NPA-2: Equal authority consumption is not an equal monetary split

Let the logical cost be $`k`$, the combined balance be $`J`$, and the component balances be $`L`$ and $`R`$.
For funded binary inputs, the default policy computes:

```math
j = \min(k,J), \qquad p = \min(k-j,L,R).
```

The combined pool pays $`j`$ and each component pays $`p`$.
The physical debit total is $`j+2p`$, not generally $`k`$.
For a logical cost of five with no joint balance, two funded components each pay five.

This can conserve the required authority multiset.
It does not meet a separate requirement that all wallets together pay five monetary units.
The existing source description of this policy as balanced across all wallets is insufficient evidence for that requirement.

The monetary solver must therefore sit behind an explicit authority-to-money contract.
The September 10 decision selects lexicographic minimax for restricted funding, with the capped allocator retained for certified all-to-all scopes.
For a monetary obligation of five and three sufficiently funded payers, an even integer split has amounts two, two, and one.
The residual policy determines which payer receives the smaller share.
This example describes the planned monetary behavior, not the current implementation or an approved activation rule.

### NPA-3: The two fee paths do not establish one common monetary fee rule

The structural path uses `FlatFeeApportionment`.
The state-bound path instead creates a compound authority event and allocates that event through the custody allocator.
With distinct leaf custody and no funded compound pool, the leaf option contains one unit for each authority atom.
The runtime passes those fee amounts to vault settlement without an intervening arithmetic cost split.

The approved runtime regression reproduces this mismatch through actual user and recipient balances.
With `Nil`, two selected signers transfer two monetary units and three selected signers transfer three.
The fixed monetary fee requirement is one total unit.
The paper's `eq:fee-extract` transfers one token, whose authority and monetary interpretation must remain explicit.
The regression uses actual state-bound execution and replay, not only the structural helper that already enforces a flat fee.

### NPA-4: Byte charging needs the same economic separation

The byte allocator multiplies each selected authority-option component by the event amount.
Its current two-component test expects seven units from each component for an amount of seven.
That test passed and confirms the existing rule, not monetary cost sharing.

`comm_charge` measures transferred payload bytes and trace bytes.
Introduction charging measures installed data or continuation representations.
These functions alone do not establish a time-based storage rent policy.
The resource contract must separately identify storage lifetime, price units, and liability before claiming complete storage pricing.

### NPA-5: Existing proofs do not establish the new economic contract

[`CompoundSettlement.tla`](../../../formal/tlaplus/cost_accounted_rho/CompoundSettlement.tla) explicitly models three pools and a second binary group sharing one component.
It checks the current compound settlement rule.
It does not model arbitrary payer lists, capped max-min fairness, or a residual cursor.

[`CASettlement.v`](../../../formal/rocq/cost_accounted_rho/theories/CASettlement.v) connects funded reductions to scalar reservation and refund arithmetic.
Its inspected native settlement theorem instantiates byte cost and fee with zero.
It does not establish multi-payer monetary allocation or conversion pricing.

These observations do not invalidate the narrow theorems.
They prevent those theorems from serving as completion evidence for the new solver.

### NPA-6: Vault settlement supports lists but does not calculate shares

`VaultAllocation` and `VaultSettlement` contain an address, a custody role, and integer amounts.
`ApplyCostDeploy::new` accepts vectors of these records.
It coalesces duplicate address-role pairs and rejects overflow, mismatched entries, and charges above their reservations.
The two custody roles are `General` and `ValidatorFuel`.
These roles are not interchangeable assets or user-selectable conversion rates.

[`SystemVault.rho`](../../../casper/src/main/resources/SystemVault.rho) processes the allocation list through `reserveAll`.
`refundReserved` restores earlier reservations when a later reservation fails.
`settleReserved` sends each unused amount to its source address and role.
It sends each specified fee to the fee recipient.
The contract does not derive an equal split from an overall obligation.

The existing `system_vault_atomic_cost_application_is_conservative_and_rolls_back` test supplies two explicit reservation and settlement entries.
Its successful example reserves 100 and 200, burns 60 and 120, and pays fees of 10 and 20.
The test checks payer losses of 70 and 140, recipient credit of 30, and rollback after a later reservation failure.
It does not construct a jointly signed deploy or test allocation fairness.

The existing `atomic_cost_request_is_permutation_invariant` property also generates exactly two entries.
This is a test-coverage bound, not a demonstrated two-entry limit in `ApplyCostDeploy`.
The list implementation needs generated arity, alias, role, permutation, and failure-position coverage under the planned payer cap.

### NPA-7: Carrier exchange is not priced vault conversion

[`Exchange.rhox`](../../../casper/src/main/resources/Exchange.rhox) consumes one datum from each supplied carrier channel.
It sends each payload to the opposite carrier and acknowledges the swap.
The payloads are opaque to the contract.
The contract has no vault withdrawal, asset identifier, quoted rate, slippage limit, or refund provenance field.

The existing `exchange_resolves_and_swap_conserves_per_channel` test checks that payloads seven and eleven exchange places.
Other tests check complete stack transport and the requirement for both inputs.
These tests do not establish authorized conversion between wallet assets.
Swapping one datum for another does not establish equal monetary values for their contents.

The planned conversion contract must distinguish native same-asset funding from an authorized cross-asset trade.
The current `VaultAllocation` record cannot identify an arbitrary asset or conversion route.
Adding rate arithmetic to the carrier swap alone would not establish authenticated vault settlement.

### NPA-8: Byte installation charges are not a storage lifetime tariff

`reserve_introduction_identity` records introduction bytes against the declared authority.
For a persistent introduction, it uses the identity and billable kind to avoid duplicate charging within its tracked lifecycle.
This rule concerns installation and replay identity.
It does not multiply retained bytes by elapsed time.

The arbitrary-payer resource contract must state which storage liabilities exist before assigning monetary prices to them.
It must preserve the distinction between installation bytes, delivery bytes, trace bytes, and any separately approved retention policy.
The user did not authorize treating an unspecified rent policy as an implemented part of the papers.

### NPA-9: Atomic settlement proves transfer conservation, not fee selection

[`AtomicVaultSettlementRefinement.tla`](../../../formal/tlaplus/cost_accounted_rho/AtomicVaultSettlementRefinement.tla) accepts `RealizedFee` as a deployment-to-payer map.
The model parameterizes the payer set rather than limiting the definition to two wallets.
Its bound assumption requires each supplied fee, compute burn, and byte burn to fit the supplied reservation.
It does not derive the fee map from a separate total monetary obligation.

`FeeCreditIsAConservingTransfer` equates recipient credit with the sum of the supplied fee map.
It therefore checks transfer conservation even when the supplied map contains an excessive total fee.
`ReplayMatchesFinalizedState` checks equality against that same supplied map.
These invariants cannot distinguish the observed per-signer fee from the required one-total-fee policy.

The [model-checking fixture](../../../formal/tlaplus/cost_accounted_rho/MCAtomicVaultSettlementRefinement.tla) supplies one fee unit from Alice for each deployment.
That fixture does not construct authenticated compound funding or exercise its fee allocator.
This is a model-to-implementation coverage gap, not evidence that the conservation invariant is false.
The initial inspection did not rerun the model checker.
The subsequent formal boundary control below tests this gap directly.

[`AtomicVaultSettlementRefinement.v`](../../../formal/rocq/cost_accounted_rho/theories/AtomicVaultSettlementRefinement.v) likewise accepts `fee` as a natural-number input.
`native_apply_refines_reserve_then_settle` proves agreement with reservation and settlement arithmetic for that input.
`native_apply_success_is_visible_and_conserving` proves the supplied fee is accounted for without changing canonical value.
Neither theorem derives that input from signer membership, a price schedule, or an independent fee obligation.

The repair must add fee-selection correspondence before applying these existing settlement results.
It must not replace authority conservation with monetary conservation or remove either check.

#### Checked fee-obligation boundary control

[`FeeObligationBoundary.tla`](../../../formal/tlaplus/cost_accounted_rho/FeeObligationBoundary.tla) extends the existing atomic settlement model without changing its transitions.
The wrapper supplies fee maps and an independent monetary obligation.
It does not implement a new allocator or select the future residual policy.
The valid example assigns the supplied fee to one eligible payer solely to test the independent invariant.

The checked instance contains four wallet identities, three selected funders, a separate recipient, and two deployments.
Each wallet starts with two units.
Each selected payer has a one-unit reservation bound per deployment.
The unsigned wallet has no debit.
Application transfers, compute burn, and byte burn are zero to isolate fee selection.
The independent fee obligation is one unit per deployment.

| Configuration | Supplied fee map | Checked result |
| --- | --- | --- |
| `FeeObligationBoundary.cfg` | One unit from the example payer per deployment | All inherited invariants and `FeeMatchesIndependentObligation` hold. |
| `FeeObligationBoundaryConservationOnly.cfg` | One unit from each selected payer per deployment | All inherited invariants hold. `PerSignerExcessRemainsConserved` also holds. |
| `FeeObligationBoundaryMultipliedUnsafe.cfg` | The same per-signer map | `FeeMatchesIndependentObligation` fails at aggregate settlement. |

The counterexample credits six units to the recipient instead of two.
Each selected wallet falls from two units to zero.
The unsigned wallet retains two units.
Total value remains eight units, so conservation alone does not reject the excessive fee.
The counterexample contains four states and five distinct states were explored before TLC stopped.

The wrapper's `Finalize` action commits an abstract settlement batch.
It does not represent Casper voting or a finality round.
The base model explores both deployment-selection orders but applies the aggregate settlement atomically.
It does not model native lock interleavings, distributed validators, signature verification, conversion, or authority-token regrouping.
The finite checked instance does not prove correctness for every payer count.

The formal gate now registers both positive checks and the exact expected refutation:

```bash
TLC_HEAP=512m TLC_RSS=1G TLC_WORKERS=1 \
  bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter FeeObligationBoundary
```

The focused gate passed all three checks on September 9, 2026.
The full counterexample run returned TLC exit code 12 for the expected named invariant violation.
Logs are `fee-boundary-gate.log` and `fee-boundary-counterexample.log` under the audit log directory.
TLC used a 512 MiB heap, one worker, a 1 GiB process-group memory limit, and disabled swap.
The state files used the on-disk audit directory and were removed after each run.

### Required invariant and test mappings

These requirements belong to the existing cost-accounting tasks.
They do not authorize a new Casper protocol or change a deferred task's priority.
The table identifies known proof boundaries rather than claiming an exhaustive completed inventory.

| Obligation | Required invariant | Native test oracle |
| --- | --- | --- |
| Authenticated initial capacity | Initial discovery includes eligible custody backed by verified signers. Absent members and candidate-created balances contribute nothing. | Vary selected signer subsets, compound-tree shape, and missing compound pools. Test failure before the first authority event. |
| Separate monetary fee | The total fee allocation equals the independently specified monetary obligation. Logical authority multiplicity remains intact. | Query each payer and the recipient. Compare total credit with the signed or configured obligation, not the certificate sum. |
| Physical capacity | Every physical custody balance bounds the sum of all its logical debits and reservations. | Generate aliases across compute, bytes, and fees. Check no overdraw and unchanged state on rejected reservation. |
| Resource units | Compute, introduction bytes, transferred bytes, trace bytes, and storage liability retain distinct measurement and price rules. | Vary one resource dimension at a time. Check measured usage, conversion, and maximum authorized monetary exposure separately. |
| Integer residual | The selected residual rule preserves the total and uses certified pre-state rather than deploy-controlled order or entropy. | Permute payer input order. Compare canonical plans, charges, and residual-state transitions. Include totals smaller than the payer count. |
| Failure and refund | Only approved charges survive user failure. Platform faults cannot debit user funds. Refunds retain original custody and asset provenance. | Inject failures before reservation, during execution, during settlement, and before publication. Check all wallet and application state. |
| Parallel reservation | Accepted overlapping reservations admit a valid serial order without duplicate use of physical funds. Independent custody does not require global serialization. | Model competing reservations, top-ups, and settlement with shared custody. Compare outcomes with the pure reference ledger. |
| Execution and replay | Both paths derive the same authorized plan and satisfy the independent monetary obligation. | Replay from the same pre-state. Alter fee, custody, signer selection, or schedule evidence independently and require rejection. |

Generate payer arity and custody structure independently.
Two-payer examples alone cannot establish the arbitrary-payer contract.
Check the configured maximum and its rejection boundary without treating the default test cap as a mathematical limit.
Use Loom for production synchronization boundaries, not as a substitute for independent-validator message-order models.

## Historical regression boundary for NPA-3

This section records the regression design and results before the monetary fee repair.
The current test retains the independent one-total-fee oracle.

The inspected state-bound fee tests construct single-signer envelopes.
For example, `state_bound_settlement_charges_the_realized_branch_and_replays_identically` asserts one fee unit after `Cosigned::from_single_signer`.
That fixture cannot detect multiplication by the number of authenticated funders.
The direct vault test supplies its fee amounts, so it cannot detect the same error either.

An end-to-end regression must use the following independent observations:

| Requirement | Required observation |
| --- | --- |
| Actual deploy authentication | Use distinct signing keys and a valid jointly signed envelope. Reject altered or unsigned payer membership. |
| Distinct physical custody | Query each signer vault and the recipient vault before execution. Do not pre-fund a compound pool in the leaf-only case. |
| Constant fee obligation | Compare the total fee with the specified fee, not a sum copied from the certificate. Vary payer arity. |
| Separate compute cost | Measure realized compute and byte costs separately. A change in these costs must not silently change the fixed fee. |
| Native settlement | Execute state-bound admission and the actual vault operation. Check each payer loss and recipient gain. |
| Replay | Replay from the same pre-state. Check the resulting root, individual balances, and fee allocation. |
| Threshold selection | Exclude placeholder signers. Test different valid selected signer sets without charging absent members. |
| Physical aliases | Preserve logical authority multiplicity while preventing duplicate use of the same physical balance. |
| Failure | Verify no committed debit for rejected funding or a platform fault. Check the approved charge rule for deterministic user failure separately. |

The non-Casper allocator tests can establish arithmetic and authority counterexamples first.
The approved test-only change under `casper/tests/` now demonstrates the fee discrepancy through actual vault balances.
This audit does not authorize a production Casper change or select a new activation boundary.

### Approved test implementation

The user subsequently approved the test-only change.
[`multi_payer_fee.rs`](../../../casper/tests/util/rholang/multi_payer_fee.rs) now constructs authenticated protocol-v6 envelopes from genesis-funded keys.
It checks one-of-one, two-of-two, three-of-three, and two-of-three signer sets.
It does not fund a compound vault for the multi-signer cases.

The test ran in the working tree based on the commit identified above, not a clean checkout.
Earlier OSLF repairs and test-module additions remained present.
The Casper production files and `authority.rs` had no working-tree changes.

Each case starts with state-bound admission from the same initial state.
Admitted cases then execute native vault settlement and replay.
It compares each wallet loss with the recorded compute, byte, and fee debit.
It separately requires recipient credit of one monetary unit per admitted deploy.
The fixed-fee assertion does not derive its expected value from the certificate.
The test rejects a tampered signature and checks zero debit for an unsigned threshold member.

The expanded test runs both `Nil` and a process with one COMM for all four signer configurations.
It records unexpected admission failures and fee mismatches, then reports them together.
An unexpected rejection remains a failing assertion, not a skipped test.
Other correctness assertions remain immediate failures.
The test does not modify production allocation or suppress an observed defect.

The first run compiled and failed at two-of-two admission before reaching its fee assertion.
The single-signer execution and replay completed first.
A second run with targeted runtime logging reproduced the rejection.
The log reports `state-bound exhaustion exposed no new authenticated authority` with capacity zero.
Both runs ended with the expected test failure, not an out-of-memory termination.

The capacity path inserts the compound funding signature in `state_bound_capacity_signatures`.
It does not initially expand that signature into its signer leaf purses.
Leaf discovery depends on later frontier events or explicit presentations.
For the plain process, the zero-capacity execution stops without producing that frontier.
This makes initial capacity discovery a separate failure for the COMM cases.
It is not the structural helper's binary `GroupShape` rejection.

The expanded run completed with exit code 101 after 134.29 seconds of test execution.
It demonstrates both failures without changing production code:

| Process | Selected signers / policy members | Admission | Actual recipient credit | Required recipient credit |
| --- | --- | --- | ---: | ---: |
| `Nil` | 1 / 1 | Admitted and replayed | 1 | 1 |
| `Nil` | 2 / 2 | Admitted and replayed | 2 | 1 |
| `Nil` | 3 / 3 | Admitted and replayed | 3 | 1 |
| `Nil` | 2 / 3 | Admitted and replayed | 2 | 1 |
| One COMM | 1 / 1 | Admitted and replayed | 1 | 1 |
| One COMM | 2 / 2 | Rejected with zero initial capacity | Not settled | 1 after successful admission |
| One COMM | 3 / 3 | Rejected with zero initial capacity | Not settled | 1 after successful admission |
| One COMM | 2 / 3 | Rejected with zero initial capacity | Not settled | 1 after successful admission |

The admitted cases preserve replay roots and account for observed wallet debits.
They also preserve zero debit for the unsigned threshold member.
Those checks pass despite the incorrect total monetary fee.
Thus replay agreement and conservation against the recorded allocation do not establish compliance with an independent fee obligation.

The COMM rejections do not establish the fee that successful COMM settlement would charge.
The test does not claim that rejected cases reached settlement or replay.
Logs are `npayer-vault-fee-before.log`, `npayer-vault-fee-rejection.log`, and `npayer-vault-fee-expanded-before.log` under the audit log directory.

The expanded run has these SHA-256 evidence hashes:

| Artifact | SHA-256 |
| --- | --- |
| `casper/tests/util/rholang/multi_payer_fee.rs` | `1fc860cb30e882efa9e564bc201e2860dd3fe8cfcaf5056a96ee459bc4f06c60` |
| `casper/src/rust/util/rholang/acceptance.rs` | `d3747fdbaebee5181e1ef84410e81ee72ca02462a556120dd087df3e254807c5` |
| `rholang/src/rust/interpreter/accounting/authority.rs` | `558d52cad942d9a54ca6a50f608396a0b003fa0d592332e348cebce2b501dba8` |
| `rholang/src/rust/interpreter/accounting/oslf.rs` | `fdb85340dbd0bbfdcab02c3e195a591706e5e1532805b2581f60326c8e46b366` |
| `npayer-vault-fee-expanded-before.log` | `1e28ce048ad30e963617434bbdc6d9bd433511e3c87c2a6bfa0276482eeb8aa0` |

The existing frontier models do not cover this initial condition.
[`StateBoundFrontierExpansion.tla`](../../../formal/tlaplus/cost_accounted_rho/StateBoundFrontierExpansion.tla) assumes `Fee < Backing[1]` and initializes capacity from that first backing entry.
Thus its initial capacity is positive.
Its `Expand` action directly exposes the next backing entry when the attempt needs more capacity.
It does not execute the native reducer steps needed to discover that entry.

[`StateBoundFrontierExpansion.v`](../../../formal/rocq/cost_accounted_rho/theories/StateBoundFrontierExpansion.v) takes the remaining frontier list as input.
Its arithmetic and termination results do not prove that native execution can produce that list with zero initial capacity.
The correction requires a model-to-runtime obligation for authenticated initial signer custody and for failures before frontier discovery.
Changing the fee rule alone would not discharge that obligation.

This test covers fixed examples, not every signer subset, payer cap, failure position, or thread schedule.
It uses the actual runtime and replay within one process, not independent network validators.
The complete generated and distributed conformance requirements remain open.

## Historical verification before the monetary fee repair

Six existing pure-library tests passed with no ignored tests:

| Test filter | Passed | Other tests filtered |
| --- | ---: | ---: |
| `physical_presentation` | 3 | 527 |
| `quantitative_debit` | 2 | 528 |
| `compute_byte_and_fee_allocations_share_physical_capacity` | 1 | 529 |

Each command used `systemd-run`, a 4 GiB memory cap, disabled swap, one build job, and one test thread.
Logs are under `target/verification/claims-audit-20260909/` with the prefix `npayer-`.
These tests do not establish arbitrary-payer vault settlement, multi-validator conformance, or complete campaign qualification.
The new native-vault regression fails for the two documented defects.
Strict `cargo clippy --locked -p casper --test mod -- -D warnings` passed under the same resource limits.
The targeted formatting check and `git diff --check` also passed.
The initial audit and fee control changed no production source or consensus behavior.
The subsequent approved capacity repair is described below.
The fee-boundary model preserves the existing settlement model's transitions.

## Historical initial-capacity repair before the monetary fee repair

The user approved the two cost-accounting repairs after the source comparison with `dev`.
The plan agent confirmed that initial discovery can be repaired independently of the monetary fee policy.
Voting, fork choice, finality, and pruning remain outside this repair.

`state_bound_capacity_signatures` now adds the full deploy funding event before calling the existing authority-discovery helper.
The helper discovers its authenticated signer leaves as well as the compound signature.
It does not add unsigned policy members or enumerate arbitrary compound partitions.
The existing pre-state reader, physical-custody aggregation, duplicate-stack rejection, and checked capacity arithmetic remain unchanged.

The runtime passes its existing host-work budget through a new budget-aware capacity wrapper.
The previous public wrapper remains available.
Discovery charges authority nodes and depth through the existing helper.
An exhausted host budget rejects the attempt rather than permitting unbounded discovery.
The execution capacity remains a ceiling, not proof that every authority obligation is funded.
Exact authority settlement remains mandatory.

### Formal and native correspondence checks

[`InitialFundingDiscovery.tla`](../../../formal/tlaplus/cost_accounted_rho/InitialFundingDiscovery.tla) checks selected-principal discovery and distinct pre-state custody accounting.
The checked instance covers every nonempty subset of four principals.
Two principals share physical custody, another has separate funded custody, and the fourth has zero-balance custody.
The safe instance checks leaf inclusion, absent-leaf exclusion, exact capacity, and funded setup progress.
Three negative controls respectively omit selected leaves, include absent leaves, and count aliases twice.
All four gate checks passed before the production repair.

[`StateBoundFrontierExpansion.v`](../../../formal/rocq/cost_accounted_rho/theories/StateBoundFrontierExpansion.v) also defines an initial physical inventory over arbitrary finite principal lists and a custody mapping.
Four new theorems establish selected-custody inclusion, exclusion of unexplained custody, unique physical entries, and membership preservation when the two input lists exchange positions.
The fourth theorem does not prove invariance under arbitrary list permutations.
Fresh compilation and an independent kernel check passed.
All four theorems are closed under the global context.
These abstract inventory results do not prove native parsing, cryptography, the full allocator, or distributed execution.

The native generated property uses authenticated threshold envelopes and 64 cases across selected subsets of four members.
It compares actual discovery membership with verified signature presence.
It checks repeated-frontier idempotence and compares computed capacity with an independent selected-balance sum minus the fixed fee.
Unsigned members receive maximum signed-integer balances in the fixture and must still contribute no capacity.
Separate examples check alias coalescing, capacity overflow rejection, and zero-host-budget rejection.
All four focused Rust tests passed.

The unchanged eight-case vault regression completed after the repair in 207.81 seconds.
All eight cases now pass admission, replay-root equality, individual balance checks, and conservation checks.
The previous three multi-signer COMM admission failures no longer occur.
The test still fails its independent one-total-fee assertion in six cases.
For both sources, the two selected signers pay two units, and the three selected signers pay three units.
The two-of-three case also pays two units rather than one.
The fee allocator remains unchanged, and the failing assertions remain enabled.

The final four focused Rust tests passed after the alias test used the actual discovery entry point.
Strict Clippy checks passed for the Casper library and test targets with warnings denied.
The result logs are `initial-discovery-native-after.log`, `initial-discovery-reviewed-properties.log`, and `initial-discovery-reviewed-clippy.log` under `target/verification/claims-audit-20260909/`.
This permanent record preserves the results if build cleanup removes those logs.

The new formal discovery model abstracts setup admission, not native thread schedules or Casper consensus.
The generated Rust property binds the model's membership and capacity obligations to the actual discovery entry point.
It does not establish all payer counts or all thread interleavings.

## Remaining task evidence as of September 10

The user selected the planned rotating residual policy for the fee repair.
The [numeric allocation and integration record](cost-accounting-impl/rotating-monetary-allocation.md) describes the implemented kernel, formal checks, and remaining native integration requirements.
The native fee allocator now plans one total unit and execution attaches its cursor transition to vault settlement.
Replay recomputes the fee evidence from captured state.
This audit refresh inspected that integration but did not rerun the current Casper integration tests.

The source inventory does not establish implementation completion.
The following work remains within the existing arbitrary-payer and phlo task plan:

1. Resolve each audited structural and state-bound allocation finding through its tracked implementation or verified non-applicability task.
2. Complete the specified storage, operator, conversion, failure, and refund contracts without inventing an unspecified rent policy.
3. Extend the demonstrated fee and initial-capacity regressions to generated signer sets, custody aliases, and failure cases.
4. Specify separate authority conservation and monetary conservation obligations.
5. Map each obligation to the solver contract, formal proof, negative control, and generated regression test.
6. Establish migration rules before changing wire fields or removing historical helper behavior.

Signed phlo restoration also requires authenticated wire encoding and reservation/refund verification.
This audit does not establish implementation completion or bypass approval requirements.
