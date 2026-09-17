# Numeric merge authority preservation

## Status and scope

The September 10, 2026 investigation demonstrates accounting metadata loss in numeric merge reconstruction.
The user approved the multiwriter authority rule on September 10, 2026.
The shared reconstruction repair is implemented locally.
The complete verification and activation gates are not yet satisfied.
The plan-agent review is complete.
The implementation gates below incorporate the independent source review.

This issue concerns the integration of cost accounting with numeric state merging.
The proposed repair does not change numeric arithmetic, writer selection, voting, finality, or fork choice.

## Demonstrated defect

Before the repair, [`calculate_number_channel_merge`](../../../../rholang/src/rust/interpreter/merging/rholang_merging_logic.rs) reconstructed a numeric datum after selecting its value and random state.
Its `create_datum_encoded` helper set `cost_authority` and `cost_stack` to `None`.
This behavior also applied when only one output survived.

The native monetary branch test first exposed this difference between direct execution and survivor-only merging.
The numeric balances agreed, but the merged datums lost their Unit-signed accounting regions.
Their Produce hashes therefore changed.
Unit denotes a cost-free authority signature, not permission to remove provenance.

The focused [regressions](../../../../rholang/src/rust/interpreter/merging/numeric_merge_accounting_tests.rs) initially called the unrepaired production merge function.
They test a base value of ten, a difference of five, and an output value of fifteen.
The fixtures use the production serializer, stable hash, authority demand, and COMM byte-charge functions.
COMM means a communication event that matches an output with a continuation.

| Regression | Before-repair result | Meaning |
| --- | --- | --- |
| Unit authority preservation | Failed | The output loses its region identity. |
| Non-Unit funding demand | Failed | One required authority unit becomes zero. |
| Following COMM byte charge | Failed | The Unit fixture falls from 587 transferred bytes to 545. |
| Unaccounted output control | Passed | The value, random state, and Produce hash remain unchanged without accounting metadata. |

The run completed with one passing test and three failing tests.
Its log is `target/verification/claims-audit-20260909/numeric-merge-accounting-before.log`.
Each test contains integer-add and bitmask-OR cases.
The failing tests stop at their first integer-add assertion, so this run does not establish execution of their bitmask cases.

The initial COMM regression invokes the actual byte-charge function with the reconstructed datum.
It does not execute a subsequent interpreter communication or prove complete replay equivalence.
The later native checks appear under [signed funding and weighted obligations](#signed-funding-and-weighted-obligations).
An all-Unit communication can remain cost-free under the existing budget rule.
Therefore, the measured byte footprint does not establish a billed token amount for every communication context.

## Reachability and upstream comparison

Normal signed programs can reach the numeric fold with non-Unit authority.
The runtime exposes both mergeable tag names through its URI map.
The reducer recognizes tuple channels that begin with a configured tag.
Its output evaluation attaches the active funding authority without restricting the sender to a system contract.

`NonNegativeNumber.rho` and `Registry.rho` are important consumers, but they do not define the complete reachable input domain.

Local `dev` revision `cdf447ac18710d9702a27379bce6c946f421be46` has the corresponding numeric reconstruction mechanism without the branch's accounting fields.
Therefore, this is an accounting integration gap, not evidence that upstream numeric arithmetic is incorrect.
Retaining upstream arithmetic does not justify erasing newly introduced accounting obligations.

## Specification constraints

The [cost-accounted rho paper](../../../../../publications/cost-accounting/cost-accounted-rho.tex) preserves signed provenance through communication in `rem:signed-subst`.
Its join-conservation discussion also preserves authority multiplicity across regrouping.

The [continued-GSLT cost paper](../../../../../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex) distinguishes cost-free Unit apparatus from erased apparatus.
Its cost-monad construction also distinguishes signature composition from ordered resource-stack concatenation.

Neither paper explicitly defines metadata aggregation for Rholang numeric DAG merging.
DAG means directed acyclic graph.
The papers constrain the repair, but they do not independently approve a specific numeric aggregation policy.

The [knotted-topoi paper](../../../../../publications/knotted-topoi/knotted-topoi.tex), `rem:fresh` and `prop:opcorr`, requires distinct occurrences and operational correspondence.
These constraints support occurrence-sensitive provenance checks.
The paper does not prescribe numeric aggregation or wallet pricing.

The existing Rocq `MergeableChannelAccounting.v` model represents numeric values, merge types, and an external accounting boundary.
Its numeric payload type does not represent `CostAuthority` or `CostStack`.
Its unchanged-boundary theorem therefore does not prove preservation of these datum fields.
This omitted correspondence requirement explains why that model did not detect this defect.

## Approved reconstruction rule

Preserve the canonical union of authorities on retained final numeric output datums.
Use the existing `merge_authorities` identity rules:

1. Retain every distinct region identity, including Unit regions.
2. Deduplicate identical region identities with identical signatures.
3. Reject conflicting signatures for the same region identity.
4. Exclude rejected effects and consumed base outputs.
5. Preserve absence when every retained output has no authority.
6. Preserve the existing numeric and random-state calculations.
7. Recompute the Produce hash from the complete reconstructed datum.

This rule retains multiplicity when different region identities have the same funding signature.
It does not deduplicate authorities by wallet or signature alone.
An output that explicitly retains an inherited region continues to contribute that region.
The merge must not restore a consumed base region merely because that region existed before execution.

The next COMM would require the authorities of all retained numeric outputs.
The user approved that economic consequence because numeric aggregation requires an explicit accounting rule.
The existing COMM authority combiner supports this representation.
The aggregation policy depends on the explicit user approval recorded above, not solely on that implementation mechanism.

Native resource-stack outputs have an empty `pars` list and a populated `cost_stack` field.
They are not numeric outputs.
Reject malformed numeric/resource-stack hybrids instead of deleting, sorting, or combining their linear cells.
Do not add a numeric merge ordering rule for resource stacks.

## Formal exploration and limits

[`NumericMergeAuthority.tla`](../../../../formal/tlaplus/cost_accounted_rho/NumericMergeAuthority.tla) explores the proposed authority rule before implementation.
It models two independently prepared writers and two validators that read their outputs in independent orders.
Each writer carries an arbitrary subset of two region identities.
Each identity has either a Unit or a non-Unit signature.
A separate base identity detects accidental restoration of consumed provenance.
Every nonempty accepted-writer subset is included.

The safe configuration and four negative controls passed before any production correction.
The log is `numeric-merge-authority-model.log` in the same verification directory.

| Invariant or control | Required implementation/test correspondence |
| --- | --- |
| `RetainedProvenance` | Reconstructed authority contains exactly the retained output regions. |
| `ExactFundingMultiplicity` | Distinct regions with equal signatures retain their separate obligations. |
| `UnitProvenance` | Cost-free regions retain their identities. |
| `NoConsumedBaseResurrection` | Consumed base regions do not return without explicit retention by an output. |
| `ParallelValidatorAgreement` | Independent input orders yield the same canonical authority. |
| Erasure control | Removing all metadata violates retained provenance. |
| Rejected-writer control | Including a rejected writer violates retained provenance. |
| Base-restoration control | Adding consumed base metadata violates the base invariant. |
| Signature-collapse control | Deduplicating by signature violates funding multiplicity. |

The model assumes agreed input effects, accepted writers, and consistent signature bindings for each region identity.
It does not prove agreement on writer selection, binary encoding, numerical arithmetic, malformed-binding rejection, or stack rejection.
It does not model the byte-charge function or complete interpreter execution.
Passing this model does not replace policy approval or implementation correspondence tests.

## Completion requirements

After rule approval, implement the metadata repair and pass the demonstrated regressions.
Add generated tests for both merge types, arbitrary authority sets, duplicate carriers, permutations, binding conflicts, and consumed-base exclusion.
Test zero numeric differences because unchanged numbers can still carry changed authority.
Reject malformed stack hybrids before numeric reconstruction.

Execute a subsequent accounted COMM against direct and merged states.
Compare authority events, byte charges, replay results, and complete retained state.
Retain the independent-branch conflict and monetary noninterference tests.
Update the formal correspondence records and run the affected strict lint gates.

No completion claim applies to the full verification plan, full CI, or soak test at this stage.

### Local repair evidence

The production helper now combines the authorities of retained base datums and retained added outputs.
Removed base datums do not contribute authority.
The helper preserves explicit empty authority presence and rejects malformed bindings and numeric/resource-stack hybrids.
Numeric arithmetic and random-state combination remain unchanged from `dev`.

The expanded before-repair run had one passing control and ten failures.
After the repair, all 27 targeted tests passed, including 128 generated authority cases.
The generated tests compare distinct region identities, same-signature multiplicity, reversed inputs, duplicate inputs, and complete output data.

The native follow-on test executes a communication and replays its event log.
It compares direct and reconstructed data for both numeric merge types.
Its contexts include Unit-only, Unit output with a funded consumer, and non-Unit output with a funded consumer.
Authority events, byte events, costs, and final roots agree in these cases.
The log is `numeric-merge-integration.log` in the verification directory.

This native test inserts the candidate datum into RSpace directly.
It does not yet establish ordinary signed multiwriter admission, public-tag execution, vault settlement, insufficient-supply rollback, or deposit-and-retry behavior.

The initial [Rocq module](../../../../formal/rocq/cost_accounted_rho/theories/NumericMergeAuthority.v) compiled before the production correction.
That revision contained fourteen proof terms, including its decidable region equality and two concrete examples.
Its nine printed theorem assumption reports were closed under the global context.
The log is `numeric-merge-rocq.log`.

The proofs establish exact retained membership, optional presence, conflicting-binding rejection, unique bindings, and a region-count bound.
They also cover permutation membership, repeated contributors, consumed-region exclusion, and distinct occurrences with equal signatures.
The model represents region identities and signatures as natural numbers.
Its normalization preserves membership, not the Rust map's concrete byte order.
It does not prove signature hashing, encoded-size bounds, numeric folding, funding sufficiency, or complete replay refinement.

The initial synthetic multiwriter fixtures reused one random state for different datums.
Those fixtures reached the pre-existing RNG requirement for at least two distinct merge inputs.
The ordinary independent-writer fixtures now derive distinct entropy from their values and metadata.
The repair does not change the upstream RNG algorithm.
The plan review found no demonstrated authenticated-node panic from these fixtures.
Version-six user entropy includes the envelope commitment, and term evaluation separates parallel positions.
The normal merge path also removes fully cancelled channel changes before reconstruction.
These facts narrow the reachable domain but do not prove that different datum bytes always imply different random states.
The adversarial helper checks reproduced panics for empty additions and distinct datums with only one random state.
The helper now returns a checked merge error for those unsupported inputs.
It does not invent entropy or change the upstream random-state result for valid inputs.
The signed input-domain checks below cover removal-only and zero-difference numeric paths.
Malformed random-state encodings remain outside that signed fixture.

[`NumericMergeRngBoundary.tla`](../../../../formal/tlaplus/cost_accounted_rho/NumericMergeRngBoundary.tla) checked this error boundary before its correction.
The safe model passed, and its negative control reproduced `NoBoundaryPanic` failure.
The model covers zero through three distinct datums and every compatible count of distinct random states.
It treats the upstream random-state operation as opaque and does not establish native reachability.
The logs are `numeric-merge-rng-model.log` and `numeric-merge-rng-before.log`.

The full monetary branch regression passed with complete survivor-root equality in 115.76 seconds.
It also retained its reversed-input, balance, cursor, and rejected-effect checks.
The log is `numeric-merge-cursor-root.log`.
This result verifies complete state preservation for the tested single-survivor monetary branches, not all possible multiwriter programs.

The final focused Rholang run passed all 30 tests in 1.86 seconds.
Strict Clippy passed for Rholang and Casper libraries and tests with warnings denied.
The logs are `numeric-merge-final-tests.log` and `numeric-merge-final-clippy.log`.
Targeted formatting and `git diff --check` also passed.
These results do not replace the remaining funding, public-tag, metadata-growth, activation, or CI gates.

| Rocq obligation | Current Rust correspondence |
| --- | --- |
| Exact retained membership and unique bindings | `numeric_merge_authority_matches_distinct_retained_identities` checks the distinct input-identity set and output count. |
| Optional presence | `numeric_merge_retains_explicit_empty_authority` distinguishes absence from explicit empty presence. |
| Conflicting binding rejection | `numeric_merge_rejects_conflicting_region_bindings` checks both merge types. |
| Output region-count bound | The generated identity test requires equality with the distinct retained input set. |
| Permutation and repeated-contributor membership | The generated identity test also compares complete reversed and duplicated output data. |
| Consumed authority exclusion | Zero-difference replacement and netted-intermediate tests exclude discharged provenance. |
| Distinct occurrences with equal signatures | `numeric_merge_retains_distinct_regions_for_the_same_signature` checks two regions and two logical units. |
| Unit-region preservation | The singleton Unit regression and native following-communication test retain Unit metadata. |

These tests do not establish arbitrary-domain byte-order or arithmetic proofs.

### Signed funding and weighted obligations

The [signed funding regression](../../../../casper/tests/util/rholang/numeric_merge_funding.rs) executes two authenticated writers from one immutable pre-state.
The writers use different validator identities and different funded signer purses.
They publish values one and two through each public numeric tag.
Each writer passes cold replay, which bypasses the replay cache.

The fixture builds exact user-effect indexes and invokes the production DAG merger.
Synthetic block carriers supply ancestry and cache lookup fields.
The fixture does not exercise network admission, voting, or finality.
Terminal system-close effects pass writer replay but do not enter this user-effect merge fixture.

The merged value is three for both merge strategies.
Its authority equals the canonical union of both signed outputs.
Reversed branch input order produces the same complete merge result.
A third signer then consumes the merged output without new signatures from the previous writers.

| Native boundary | Verified result |
| --- | --- |
| Retained output authority | Both original writer regions remain present. |
| Subsequent consumer | Both writers and the consumer pay nonzero computation and transferred-byte charges. |
| Purse settlement | Each observed purse decrement equals its witness computation, byte, and certificate fee allocation. |
| Consumer replay | Cold replay produces the same complete state root. |
| Exhausted inherited purse | Admission rejects the consumer despite other funded purses. |
| Rejection rollback | The complete root and system effects equal an empty candidate block with the same pre-state and context. |
| Authorized top-up | A signed wallet transfer deposits 100,000 tokens into the exhausted purse. |
| New funded attempt | The consumer succeeds from the deposited state and passes cold replay. |

The initial complete funding run passed in 234.45 seconds.
Its log is `numeric-merge-funding-independent.log` in the verification directory.
A protocol-authorized burn prepares the exhausted-purse fixture.
The later top-up uses an ordinary signed wallet transfer, not a protocol mint.
These checks demonstrate a valid funded retry, not guaranteed future funding under arbitrary concurrent spending.

The final native extension passed in 298.18 seconds.
Its log is `numeric-merge-funding-reviewed.log`.
It verifies that computation and COMM byte witnesses contain all inherited writer regions.
Writer, consumer, deposit, and retry contexts use increasing block heights and sequence numbers along each tested dependency path.

The extension prepares two sibling consumers and an unrelated signed effect through three independent validator contexts.
Both consumers race for the actual merged datum.
The native merger retains exactly one consumer and the unrelated effect.
Reversing the branch input order preserves the complete merge result.

The survivor-only root comparison establishes rejected-effect noninterference.
A separate oracle sums retained execution balance differences against the common pre-state.
Merged balances match that sum for every genesis client purse and validator general purse.
This oracle does not independently check validator-fuel purse differences or network finality.
Both consumer branches and the unrelated branch also pass cold replay before merging.

The plan-agent evidence review required the later retry context and the independent balance oracle.
Both corrections affect the test fixture, not production consensus rules.
Strict Clippy passed for this reviewed native fixture in 9.77 seconds.
Its log is `numeric-merge-funding-reviewed-clippy.log`.

The first fixture incorrectly used one validator identity for both sibling writers.
That fixture reached one shared Produce race and rejected a writer.
`SystemVault._costSplit` consumes and restores the validator's ordinary `validatorFuelState` cell.
The corrected fixture uses separate validators, as required for the independent-validator scenario.
No production conflict rule changed to make the fixture pass.
This experiment does not establish that concurrent sibling proposals from one validator must both survive.

The additional generated test checks one through sixteen retained regions across up to sixteen wallet identities.
Wallet identities can repeat, so the test includes multiple distinct obligations against one purse.
For both merge types, it checks exact computation and byte draws from the production allocators.
It reduces each required purse by one unit and requires allocation failure without input mutation.
The test uses byte amounts from one through 4,096.
The focused module passed sixteen tests, including both 128-case properties, in 4.45 seconds.
Its log is `numeric-merge-funding-properties.log`.

The Rocq extension contains twenty-one proof terms and fourteen closed assumption reports.
It proves that successful reconstruction is a permutation of distinct retained region bindings.
Therefore, any fixed nonnegative region weight has the same retained sum before and after reconstruction.
It also bounds that sum by the sum across all input occurrences.

Let $`n_p`$ be the number of retained non-Unit regions assigned to purse $`p`$.
Let $`b`$ be the byte charge for one communication.
For ground signatures, the combined computation and byte obligation is:

```math
(1+b)n_p.
```

`numeric_wallet_compute_and_byte_obligations` preserves this amount for arbitrary finite input lists, wallets, and natural-number byte charges.
The generated Rust test checks the corresponding allocator result, including repeated wallet identities.
The proof does not cover compound funding alternatives, conversions, fee allocation, or machine-integer overflow.
The weighted bound does not independently prove protobuf encoded-size or canonicalization-time bounds.
The weighted proof log is `numeric-merge-weighted-rocq.log`.
The later regrouping proof log is `numeric-merge-regroup-rocq.log`.
`numeric_compatible_regrouping_preserves_regions` proves a permutation between successful flat and grouped authority reconstruction.
This theorem assumes compatible successful inputs and does not establish whole-state or random-state associativity.
The corresponding generated test compares the exact canonical authorities from flat and grouped native reconstruction.
It also bounds reconstructed bincode authority bytes by the sum of encoded input authority bytes for its canonical fixtures.
The final focused module run passed sixteen tests in 4.67 seconds.
Strict Clippy passed afterward in 4.69 seconds.
The logs are `numeric-merge-regroup-properties.log` and `numeric-merge-regroup-clippy.log`.

### Concurrent-funding correspondence model

The plan agent reviewed this boundary after the signed funding test was added.
The review requires immutable roots and independent validator views.
It does not permit a new global snapshot-equality rule for validator acceptance.

| Existing model | Coverage | Missing correspondence |
| --- | --- | --- |
| `NumericMergeAuthority` | Retained regions and independent validator read orders. | Purse backing, later consumption, settlement, and replay. |
| `StateBoundFrontierExpansion` | Capacity growth and speculative rollback. | Zero backing, exact per-purse obligations, and independent immutable pre-states. |
| `FeeCandidateSettlement` | A local fee plan, conservation, and top-up. | Sibling validators and root-bound evidence. Its `Fresh` predicate is not a consensus commit rule. |

[`NumericMergeFundingCorrespondence.tla`](../../../../formal/tlaplus/cost_accounted_rho/NumericMergeFundingCorrespondence.tla) now connects retained regions to later funding and replay.
It imports the existing `MonetaryAllocation` operators.
It does not import the global settlement actions from `FeeCandidateSettlement`.

Each validator independently collects writer effects, captures a root, discovers funding, settles, and replays.
Validator views can advance after root capture without invalidating the captured balances.
A deposit creates a new root and does not change an earlier candidate's backing.
An unrelated effect also captures its own root and must have sufficient funds at that root.

Discovery records the consumer principal and all non-Unit principals from retained writer regions.
Capacity uses this recorded frontier and reserves one unit for the fee.
Settlement separately checks exact obligations against each purse.
Thus, sufficient total capacity cannot conceal an exhausted inherited purse.

The model retains at most one sibling consumer of the ordinary datum.
It retains the unrelated effect when the combined draws fit the common pre-state balances.
It checks merged balances against every retained draw, not only effect identifiers.
This model uses a fixed consumer-first selection order to isolate monetary compatibility.
It does not prescribe a new production selection order.

| Finite parameter | Checked domain or assumption |
| --- | --- |
| Validators and writers | Two independently scheduled validators and two writers, with every nonempty accepted-writer subset. |
| Purse identities | Three ground purses. Writer regions use distinct purses, one repeated purse, or one Unit signature. |
| Initial balances | Twelve units per purse, minus one fee per retained writer from that writer's purse. |
| Funding roots | The merged state, a state with purse zero exhausted, and a state after a six-unit top-up. |
| Deposit funding | The consumer purse pays six transfer units and one fee unit. The model records the prior burn separately. |
| Consumer charges | One compute unit and one byte unit per non-Unit region, plus the consumer region and one fee. |
| Fee allocation | The consumer is the sole fee payer. The imported allocator therefore has no residual choice in this instance. |
| Independent effect | One or ten units from purse zero, admitted against its captured root. |
| Attempts | At most two per validator. Successful attempts pass replay before the modeled merge. |
| Merge domain | All selected consumer roots must match the independent effect's root. This restricts the scenario, not production acceptance. |
| Scheduling | Arbitrary action interleavings, without fairness assumptions. |

The model checks nineteen safety invariants.
The following table maps their groups to implementation evidence.

| Invariant group | Implementation evidence |
| --- | --- |
| Retained writer authority and exact inherited demand | Generated region-identity and weighted-allocation properties, plus signed writer/consumer witnesses. |
| Immutable roots and root-bound replay | Cold replay and signed deposit/retry tests use explicit pre-states and increasing execution contexts. |
| Complete authenticated frontier and capacity | Native inherited-purse settlement and rejection checks, plus initial funding-discovery properties in `acceptance.rs`. |
| Per-purse sufficiency and settlement conservation | Generated exact-capacity and one-unit-short allocator cases, plus witness-to-balance comparisons. |
| No rejected candidate effects | The native rejection root and system effects equal the empty candidate result. |
| Whole-effect retention and single consumption | Native sibling consumers race for one datum, with survivor-only root equality. |
| Independent effect retention and merged conservation | The native balance oracle sums retained execution differences across client and validator general purses. |
| Deposit conservation | A signed wallet transfer funds the later attempt, without a protocol mint. |

The controls remove frontier completeness, substitute consumer or independent roots, invent authority, or replace per-purse checks with total-capacity checks.
Other controls introduce partial rejection effects, wrong replay roots, duplicate consumption, rejected charges, missing independent charges, aggregate overdraw, and unauthorized deposits.
The global-guard control drops a compatible effect after a validator view advances.
The safe model does not use this guard.

The final targeted run passed the safe configuration and all fourteen expected counterexample checks.
Thirteen checks are fault controls, and one checks recovery reachability.
The log is `numeric-merge-funding-model-final.log` in the verification directory.
The run used a three-gigabyte Java heap, two workers, a five-gigabyte checker memory limit, and disabled swap.
The outer command had a six-gigabyte memory limit.
The final plan-agent review found no further material mathematical issue in this model.

`RecoveryReachable` checks the negation of a recovery state and requires a counterexample.
That trace demonstrates rejection at the exhausted root, an authorized top-up, and successful second-attempt replay.
It is a reachability witness, not a deliberately faulty implementation or a liveness theorem.
Its `RecoverySpec` permits the deposit only after an initial exhausted-root rejection.
This witness configuration retains both writers with distinct funding purses, matching the native funding fixture.
The general safety configuration has no such restriction and includes deposits before, during, and after consumer attempts.
The initial witness allowed a deposit before rejection, followed by an attempt against the older root.
That trace established root-bound recovery but did not establish the requested post-rejection deposit order.
The separate recovery specification checks that order without narrowing the general safety model.
The two-writer witness appears in `numeric-merge-recovery-two-writers-trace.log`.
Validator one rejects its first attempt in state eleven and succeeds on its second attempt in state seventeen.
The trace publishes the top-up between those attempts and records successful replay in state eighteen.
The checker found this witness after 10,421 generated states and 2,908 distinct states.
It stopped at the witness with 520 queued states, so these counts do not describe an exhaustive recovery search.

The first model review found that recorded frontier membership did not determine capacity.
The review also found that the unrelated effect lacked monetary changes and its own captured root.
All three model gaps were corrected before accepting the correspondence evidence.
One control initially disabled its merge action because of Boolean operator precedence.
The missing expected counterexample detected that control error, which was corrected with explicit parentheses.
These findings concern verification models, not newly demonstrated production consensus defects.

The model abstracts numeric values and authority encodings, and it has no fee-cursor state.
It does not cover cross-root merging, Casper ranking, network finality, validator-fuel settlement, arbitrary pricing, or host-memory bounds.
It does not model entirely Unit-authorized execution or program-born supply.
Separate authority, fee-cursor, and native execution checks cover related boundaries without establishing a machine-checked refinement between every model.
Disabled deadlock checking and absent fairness assumptions preclude claims of inevitable recovery or completion.
The full verification and activation gates remain open.

### Live metadata bounds

`numeric_merge_replacement_history_does_not_grow_live_authority` checks one through thirty-two consecutive consume-and-replace rounds.
Each round has one through sixteen fresh regions and one through four copies of each carrier.
The property compares the exact result with that round's retained authority.
Consumed historical regions and duplicate carriers must not increase the live region count.
The test uses 128 generated cases across both numeric merge strategies.

`numeric_merge_wide_authority_preserves_live_region_bound` checks 256 and 1,024 distinct retained writer regions for both strategies.
It checks region count, same-signature multiplicity, and the encoded authority size against the sum of contributor authority sizes.
These fixtures use canonical ground and Unit signatures.
They do not establish bounds for arbitrary nested signatures or peak heap use.

The focused module passed all eighteen tests in 15.62 seconds.
Strict Clippy passed afterward in 29.18 seconds.
The logs are `numeric-merge-growth-properties.log` and `numeric-merge-growth-clippy.log` in the verification directory.
The command had a six-gigabyte memory limit, disabled swap, one build job, and one test thread.
These checks establish fixture-specific size bounds, not an asymptotic runtime proof or a month-long uptime guarantee.

`numeric_merge_preserves_compound_quote_and_name_metadata` extends constructor-level coverage to compound signatures, quoted strings, and named strings.
It uses 128 generated cases with one through sixteen contributors and one through 256 payload bytes per generated atom.
Each contributor adds an atom to the previous canonical compound signature.
The test compares reconstructed regions with an independent map of the original region records, sorted by identity.
It also checks encoded-size bounds and complete output equality after contributor reversal.
These fixtures test metadata preservation, not authenticated withdrawal rights or compound-purse settlement.

The expanded focused module passed nineteen tests in 17.34 seconds before the independent-oracle refinement.
The logs are `numeric-merge-structured-properties.log` and `numeric-merge-structured-clippy.log`.
The refined oracle passed its targeted generated test in 1.87 seconds.
Strict Rholang and Casper library and test Clippy checks then passed in 4.09 seconds.
The logs are `numeric-merge-structured-oracle.log` and `numeric-merge-structured-oracle-clippy.log`.

### Signed numeric input domains

`signed_numeric_zero_and_removal_effects_preserve_state_on_merge` seeds three numeric channels through each public mergeable tag.
Two signed programs then execute from that common state through independent validator identities.
Each execution passes cold replay before its user-effect index enters the production merger.

| Input case | Required result |
| --- | --- |
| Removal without replacement | The datum disappears through ordinary deletion, without a numeric contribution entry. |
| Same-value replacement | The numeric contribution is zero, but the replacement authority remains present. |
| Integer-add cancellation | Contributions of one and negative one retain value eight and both retained authorities. |
| Bitmask update without new bits | Contributions of one and zero produce value nine and both retained authorities. |
| Independent ordinary output | The removal-result output remains present after merging. |
| Reversed branch order | The complete merge result remains equal. |

The first native run passed in 104.09 seconds.
Its log is `numeric-merge-domain-native-exact.log` in the verification directory.
The plan review then requested explicit index assertions for the zero contributions and removal-only path.
These assertions distinguish a zero numeric difference from empty added and removed datum lists.
This fixture does not directly call the numeric override with empty additions.
The reviewed fixture passed in 105.60 seconds.
Its log is `numeric-merge-domain-reviewed.log`.
Strict Casper library and test Clippy checks passed in 8.80 seconds after removal of an unnecessary reference-drop statement.
The lint log is `numeric-merge-domain-clippy.log`.

### Activation evidence boundary

The existing fresh-genesis rule applies to the corrected numeric merge semantics.
Preserved metadata changes datum bytes, Produce hashes, and later costs, even when a numeric value remains equal.
All validators must use the same approved implementation and genesis state.
The repair is not an in-place migration for an existing accounted network.

[`DeployOccurrenceStore::activate_fresh`](../../../../block-storage/src/rust/dag/deploy_occurrence_store.rs) validates the occurrence schema and protocol marker.
That marker does not commit a numeric-merge algorithm or a node source revision.
Authority-accounting version nine identifies certificate evidence, not the full merge implementation.
Therefore, neither marker alone proves that mixed old and corrected validators are compatible.

The [protocol activation policy](deploy-envelope-v6-1.md#activation-and-migration) requires a new protocol number if a public network accepts another protocol-six meaning.
No claim here establishes that such a network upgrade has occurred or that historical state can be reused safely.
Release qualification must bind the node revisions, genesis identity, certificate format, and integration-test results together.
Existing-history migration and changes to Casper protocol activation require separate authorization.

## Reviewed implementation plan

### Gate A: Approve and record the complete rule

Approve retained-output canonical authority union, with no changes to upstream numeric arithmetic, writer selection, dependency handling, voting, or finality.
Do not add a lock or reject valid multiwriter merges to avoid metadata combination.

Keep `None` distinct from `Some(empty)` because their encodings differ.
Preserve a valid singleton's exact presence representation.
For multiple outputs, retain explicit authority presence if any retained contributor has that presence.
Check these rules against existing decoder requirements before implementation.
Do not introduce a new rejection condition without a demonstrated invalid input or an approved semantic rule.

| Alternative | Assessment |
| --- | --- |
| Canonical retained-output authority union | Recommended. It preserves provenance and distinct region obligations. |
| One selected output's authority | Incomplete. Other retained outputs lose their obligations. |
| Remove every Unit region | Unsupported. Region identity and encoded bytes remain observable. |
| Reject all accounted multiwriter merges | Unacceptable concurrency shortcut. |
| Separate provenance for each numeric contribution | A larger representation and consumption design. Reconsider only if union fails the funding or operational-correspondence gates. |
| Singleton-only repair | An intermediate repair, not completion of the known multiwriter defect. |

### Gate B: Complete the failing regression matrix

Retain the existing three failures and the passing unaccounted control.
Run integer-add and bitmask-OR cases independently so an early failure cannot hide the other strategy.

Add examples for multiple writers, equal signatures with distinct regions, repeated authenticated effects, conflicting bindings, and rejected writers.
Add dependent chains whose intermediate outputs cancel against removals.
Separate malformed constructor fixtures from states produced by normal signed programs.

Audit every empty or zero-change case:

- An unchanged channel.
- Empty added and removed lists after cancellation.
- A zero numerical difference with a replacement datum.
- A removal without a surviving addition.
- Several numerical differences that cancel.
- A bitmask update that changes metadata but introduces no new bits.

The trie-action builder calls the numeric override before its ordinary empty-change guard.
Therefore, arithmetic zero and an empty ordinary delta do not independently prove that reconstruction is unnecessary.
A proven no-op must preserve the original datum, including its random state and metadata.
If a case exposes an unrelated upstream defect, demonstrate that defect before proposing a separate correction.

### Gate C: Verify semantics before production correction

Extend the Rocq representation to include actual datum authority and optional-field presence.
Use arbitrary finite contributor lists and region identities for these proof obligations:

- Singleton preservation and legacy unaccounted identity.
- Metadata permutation invariance and compatible metadata-combination associativity.
- Duplicate-region invariance and distinct-region multiplicity.
- Conflicting-binding detection.
- Rejected-effect exclusion and consumed-base exclusion.
- Cancellation of dependent intermediate provenance.
- Preservation of the existing numeric result.
- Correspondence between retained authority and later funding demand.
- Bounds on output region count and encoded metadata size.
- Exclusion of resource-stack data from numeric reconstruction without resource loss.

Metadata associativity does not prove whole-state merge associativity.
Random-state combination and effect selection retain their separate specifications and tests.

Extend TLA+ with independent validator states, dependent writers, merge decisions, later consumption, funding changes, and replay.
Include exact negative controls for partial settlement, duplicated provenance growth, and acceptance without sufficient inherited funding.
Keep the existing erasure, rejected-writer, consumed-base, and signature-collapse controls.
Report each finite domain and each unmodeled boundary.

### Gate D: Implement the checked reconstruction path

Use one checked authority-reconstruction path for singleton and multiwriter cases.
Preserve the existing numerical and random-state algorithms.
Use canonical identity checks before publishing the new datum and its Produce hash.

Validate malformed metadata at a defined boundary.
Return structured errors instead of deleting data or panicking.
Do not sort, concatenate, or deduplicate linear resource-stack cells in a numeric fold.

### Gate E: Verify funding, replay, and resource use

Execute normal signed programs that use both public mergeable-tag names.
Include native vault and registry numeric channels.
Run a later accounted communication after merging independent writer states.

Check Unit-only communication, a Unit output with a funded continuation, and non-Unit outputs separately.
Compare region identities, logical demand, physical debits, encoded-byte charges, event witnesses, and state roots.
Test the source execution path and its replay path against the same authenticated inputs.

The existing state-bound admission path discovers additional authorities after an exhausted attempt.
It can restore the pre-state, expand the authority frontier, and retry with additional authenticated supply.
This mechanism does not prove that prior writers reserved funds for every future consumer.

The native funding matrix must establish these outcomes:

| State | Required result |
| --- | --- |
| Every inherited obligation has sufficient authorized supply | The consumer settles the exact verified costs. |
| An inherited obligation lacks sufficient supply | The consumer rejects without partial user effects, fees, or cursor advancement. |
| An authorized deposit follows that rejection | A new attempt can use the new authenticated pre-state and supply. |
| A region lacks required authorization | Metadata does not manufacture signatures, presentations, or withdrawal rights. |
| Branch inputs arrive in different orders | The same retained effects produce identical metadata, costs, and roots. |

Do not require fresh signatures from historical writers unless the existing authority contract requires them.
Do not relax authorization to make a union-funded consumer pass.

Generate property tests from each applicable formal invariant against the production helper.
Include configurable contributor counts, optional-field presence, binding conflicts, zero changes, and permutations.
Use Loom only when a changed Rust synchronization boundary requires a scheduler-interleaving check.
Do not substitute a test-only mutex model for independently executed validators.

For performance analysis, let $`R`$ denote the input region count and $`B`$ the total encoded signature bytes.
Target $`O(B + R\log R)`$ work and $`O(B + R)`$ auxiliary storage for canonical reconstruction.
Verify actual signature-validation costs before claiming those bounds.
The current path calls `canonical_cost_signature`, which sorts signatures and validates nested quoted terms.
Its total work cannot be inferred from region-map insertion alone.

Output provenance must depend on retained live contributions, not the complete historical writer set.
Duplicate carriers, rejected effects, and consumed intermediate outputs must not increase its size.
Measure repeated consume-and-replace cycles and wide merges under memory limits.
Audit merge-time resource limits separately from admission-time host-work limits.
Do not invent an arbitrary region cap or trim authority to control memory.

### Gate F: Activate and validate the corrected state semantics

Preserved metadata changes state bytes, Produce hashes, and some subsequent costs.
A funding-certificate version change alone does not establish safe activation.

Follow the approved fresh-genesis deployment model.
Document the compatible node set and the affected protocol checks.
Do not silently reinterpret existing accounted state or mix old and corrected merge semantics.
Any existing-history migration requires a separate explicit compatibility decision.

Run the targeted regressions, proof gates, formatting, strict Clippy, and relevant CI workflows.
Preserve the independent cursor-merge and rejected-effect noninterference tests.
Run heavy local commands with hard `systemd-run` memory limits and zero swap.
Use disk-backed scratch directories rather than `/tmp`.

Present any failed semantic or funding gate before expanding the repair into a new protocol or economic design.
