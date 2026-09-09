# PR 216 external review adjudication

## Purpose

This record evaluates the external review in `pr-216-review.zip` against the current branch and its governing design sources.

The review examined commit `1a60fa567` against `dev` commit `375933475`.

This adjudication examined committed branch head `3980ed402` and the later uncommitted repair set.

The review did not run any proof tool. Its formal-verification conclusions are source audits, not successful proof executions.

## Source authority

The following sources control this adjudication, in descending order:

1. `cost-accounted-rho.tex` and `continued-gslt-cost-v2.tex` control the semantics that they specify.
2. User-ratified decisions control native F1R3node integration choices that the papers do not specify.
3. [Cost-accounting decision records](../casper/theory/cost-accounting-decision-records.md) record those integration choices.
4. Current production code determines implemented behavior.
5. Formal artifacts establish only the properties in their checked statements.
6. Tests establish only the paths and assertions that they execute.

DR-27 records Greg Meredith's one-denomination clarification. REV and phlogiston are names for one native token denomination.

One denomination does not imply one custody role. General custody, locked stake, validator fuel, fees, quarantine, and burn remain distinct roles.

DR-36 maps paper resource roles to SystemVault and RSpace. It forbids duplicate economic ledgers and unbacked credits.

DR-38 refines abstract reserve, execute, settle, and refund phases into one atomic native vault transition.

DR-41 states that the mathematical `s0` collapse is an embedding. It is not a production accounting mode.

DR-47 adds deterministic byte accounting within the same reserved hard ceiling.

## Verdict terms

**Defect** means that current behavior violates a required safety, liveness, economic, or compatibility property.

**Gap** means that a claim, proof, test, or document exceeds its established evidence.

**Observation** means that the review found a real property that does not require a behavior change.

**False positive** means that the cited behavior follows an intentional and supported design decision.

**Addressed, unverified** means that the working tree contains a repair, but its required gates have not passed.

## Executive disposition

The review contains several important defects. It also recommends deleting intentional extension scaffolding and changing supported protocol decisions.

The repair preserves exact replay, validator concurrency, strict finality, and intentional linear-logic scaffolding.

The repair adds missing custody roles, deterministic resource bounds, retry ownership, executable refinement evidence, and performance gates.

## Finding-by-finding adjudication

### Wire format

| Finding | Verdict | Current disposition | Required evidence |
| --- | --- | --- | --- |
| `sig_algebra` is unreachable from production funding | Observation | Preserve the extension scaffold. Document its fail-closed funding boundary. | Extension-boundary tests and accurate conformance text |
| Witness size and verification scale with COMM count | Defect | Add protocol limits for events, bytes, authorities, and search work. | Boundary properties, encoded-size tests, TLA+ work bounds |
| New score constants collide across sorter vectors | False positive | Vector position supplies the domain. Do not change byte identities. | Existing golden vectors |
| `EZipper` collides with `DOUBLE` inside one vector | Pre-existing defect | Defer only until a coordinated protocol-version migration can preserve interoperability. | Cross-version golden vectors and migration tests |
| Five signature schemes are declared, but two are active | Documentation gap | Keep inactive schemes fail-closed. State active and reserved values. | Producer and consumer feature-symmetry tests |
| `BlockMetadataInternal` reserves the wrong old name | Addressed, unverified | The working tree now reserves `invalid`. | Protobuf descriptor and generated-code checks |
| Unknown rejection reasons decode as `Unspecified` | Addressed, unverified | Unknown discriminants now return an error. | Example and generated unknown-value tests |
| Unknown admission status decodes as `Executed` | Addressed, unverified | Unknown discriminants now return an error. | Example and generated unknown-value tests |

### Runtime metering

| Finding | Verdict | Current disposition | Required evidence |
| --- | --- | --- | --- |
| Pure computation has no deterministic work ceiling | Defect | Add a consensus-visible, non-economic work budget. Never use wall time. | Work-exhaustion refinement, properties, and replay equality |
| `MeteredFrame` is not a production charge path | Observation | Preserve it as test and extension infrastructure. Correct its scope. | Economic-charge versus work-budget boundary test |
| Deep operator nesting can abort the process | Pre-existing defect | Add a safe depth guard here. Coordinate the later pushdown-automata replacement. | Bounded-depth regression without duplicated PDA work |
| Unmatched input and output introductions have byte cost | Supported refinement | Keep the storage grade defined by DR-47. | Arrival-order equality, persistence, and replay tests |

Economic cost and host work serve different purposes. Economic cost implements the papers' resource semantics.

Host work limits protect every validator from finite but excessive structural computation.

Both budgets must use deterministic integer arithmetic. Both budgets must produce identical play and replay decisions.

### Static analysis and linear scaffolding

| Finding | Verdict | Current disposition | Required evidence |
| --- | --- | --- | --- |
| The static `Sigma >= Delta` gate is not the production gate | Observation | Preserve it as a closed-fragment and GSLT extension boundary. | Accurate scope documentation |
| Source documentation calls the static gate live | Gap | Correct every live-path claim. | Documentation audit |
| Static demand and runtime settlement use different units | Activation gap | Add byte and work refinement before activation. | Unit-refinement theorem and generated properties |
| Static demand is unknown for dereference and contracts | Supported limitation | The papers permit dependent proofs or conservative bounds. | Closed-fragment domain tests |
| Authority discovery reruns execution for each new frontier | Performance defect | Bound the frontier and work before optimization. | Adversarial frontier properties and work measurements |
| Physical allocation uses backtracking | Risk | Keep canonical search semantics. Bound the explored state space. | Permutation properties and search-bound models |
| The implementation completes the full static theorem | Overclaim | Narrow the claim until term-level refinement exists. | Conformance audit |

The repair does not delete `delta_sigma`, `MeteredFrame`, signature algebra, or GSLT category scaffolding.

These components provide defined extension boundaries. Documentation must distinguish proved scaffolding from active consensus behavior.

### Acceptance, custody, and economics

| Finding | Verdict | Current disposition | Required evidence |
| --- | --- | --- | --- |
| A fresh bond can increase transferable custody | Critical defect | Corrected. Initial issuance is genesis-only. Later bonds use normal epoch eligibility. | `BondIssuanceLifecycle.v`, nine TLA+ controls, lifecycle properties, and play/replay tests |
| Proposer execution burns transferable custody | High defect | Burn from dedicated validator-fuel custody within SystemVault. | General-custody and fuel-custody independence tests |
| Epoch validator fuel becomes stakeable | High defect | Credit the validator-fuel role, not general custody. | Custody-role noninterference proof |
| Inclusion capacity depends on proposer liquid balance | High defect | Make capacity depend on validator-fuel custody. | Low-general and low-fuel boundary tests |
| A rejection closes one signature group | Supported behavior | Preserve group atomicity and document the client result. | Shared-purse permutation and starvation properties |
| Epoch receipts grow without a bound | Uptime defect | Compact successful history without permitting a second mint. | Retention bound and replay-protection proof |
| A failed epoch mint can be skipped | High defect | Corrected. The first failure stops close, and runtime restores the exact pre-state. | DR-60, `EpochMintAtomicity.v`, six TLA+ controls, Loom, generated properties, and native every-position failure tests |

The paper constants remain unchanged. One admitted deployment uses three validator execution units and one client fee unit.

The validator-fuel role uses the native denomination. It does not create a second asset or duplicate ledger.

Formal conservation must include general custody, stake, validator fuel, quarantine, fees, and burn.

### Play and replay

| Finding | Verdict | Current disposition | Required evidence |
| --- | --- | --- | --- |
| Validators execute admission and then execute exact replay | Performance defect | Optimize only after an executable refinement proof. | Differential verdict properties and performance experiments |
| Empty rejection sets still trigger partition rederivation | Candidate optimization | Skip only if exact replay proves the accepted partition. | Forged-partition negative test and equivalence proof |
| Rejection records need independent reconstruction | Required behavior | Keep reconstruction until rejection records carry verified proofs. | Fabricated-rejection rejection test |
| Replay capacity exceeds realized cost | Supported behavior | Preserve conservative reservation and exact refund. | Randomized realized-cost bound properties |

Exact rigged replay remains authoritative. The proposer cannot authorize a validator to trust an unverified classification.

### Casper, finalization, and recovery

| Finding | Verdict | Current disposition | Required evidence |
| --- | --- | --- | --- |
| Failed LFS restore consumes retry ownership | Critical branch regression | Restore bounded retry ownership and explicit terminal failure. | Corrupt-first, correct-second, duplicate, restart, and Loom tests |
| Peer LFS validation can panic | Pre-existing robustness defect | Return a typed failure for untrusted input. | Malformed-peer and concurrent-restore tests |
| Engine truncated-DAG coverage was removed | Coverage gap | Restore public engine-boundary coverage. | LUCA and restored-node regressions |
| Finalization uses strict `>` instead of inclusive `>=` | False positive | Preserve strict fault-tolerance semantics. Correct stale tests and prose. | Boundary, adjacent-value, Rocq, and TLA+ evidence |
| Missing latest-message materialization aborts context construction | Supported fail-closed invariant | Return a typed fatal invariant result where possible. | Corruption test and operational telemetry |
| Protocol-v6 rejects legacy data directories | Intentional compatibility break | Preserve fail-fast startup. Document reset and export procedures. | Startup compatibility tests |
| The dirty signed-floor rule accepts one-parent ancestry | Proof blocker | Require a proved multi-parent state-preservation condition before landing. | Rocq, TLA+, negative controls, and replay properties |
| Consensus work is independent of cost accounting | Packaging observation | Keep only evidence-backed fixes. Preserve reviewable commit boundaries. | Decision records and per-subsystem gates |

The signed finalized floor is the replay anchor. A derived candidate certificate cannot replace that anchor before durable certification.

Proposal readiness cannot depend on an uncertified candidate floor. Multi-parent replay must preserve the committed signed-floor state.

### Storage, queues, and lifecycle ownership

| Finding | Verdict | Current disposition | Required evidence |
| --- | --- | --- | --- |
| Admission manifest omits rejection variants | Addressed, unverified | Generate the manifest from `AdmissionRejectionReason::ALL`. | Exhaustiveness property and golden digest |
| DAG lock documentation omits nested locks | Documentation gap | Record the complete lock and await order. | Loom model linked to production ownership |
| Checkpoint retry drops terminal gate rejections | Addressed, verification in progress | Recertify every complete canonical retry window. Publish only the successful generation. | Rocq, TLC, Apalache, Loom, partition properties, forced retry, terminal rejection, suffix retention, and peer replay |
| Retriever capacity protects old work and rejects new work | Policy observation | Keep bounded memory. Prove fairness and expose deferral metrics. | Sustained-arrival property |
| Retry quarantine is removed immediately | Addressed | Store quarantine with the bounded request owner. Clear attempt data without deleting dependency evidence. | TLC, Apalache, Rocq, expiry, re-entry, capacity, receipt, and pruning tests |
| Settlement tickets can be consumed before durable insertion | Addressed and verified | Retain durable citation evidence. Commit one claimed reservation only after certified DAG insertion. | TLC and Apalache safe models plus five controls, axiom-free Rocq, failure injection, properties, restart tests, and Loom schedules |
| Several counters exhaust for the process lifetime | Uptime defect | Use bounded, owner-scoped windows with fair expiry. | Long-horizon and wraparound properties |
| Bond generations and burn tombstones grow | Safety storage risk | Preserve anti-reuse state and define a priced storage envelope. | Identity-reuse proof and storage-bound analysis |

### Formal verification and CI

| Finding | Verdict | Current disposition | Required evidence |
| --- | --- | --- | --- |
| Signatures-as-names lacks a concrete theorem | Formal gap | Add encoding and injectivity over production authority names. | Rocq theorem and serialization properties |
| Modulus equality targets the legacy carrier | Formal gap | Prove the result for current signed terms and encoding. | Rocq proof and extraction tests |
| Join conservation proves algebra, not operational firing | Formal overclaim | Add the operational theorem or narrow the label. | COMM, partition, and replay theorem |
| `funding_decidable` proves natural-number comparison | Formal overclaim | Add term-level demand refinement or narrow the label. | Closed-fragment theorem and counter-domain tests |
| Category proofs use a reduced graded LTS | Intentional scaffold | Keep the scaffold and state omitted ciGSLT obligations. | Accurate theorem names and conformance table |
| Cross-witness models do not execute production code | Evidence gap | Add serialization and state-transition refinement bridges. | Production-adjacent properties and Loom tests |
| Runtime budget bridge omits nonzero event kinds | Formal gap | Add COMM, produce, consume, byte, and work vectors. | Rocq-to-Rust generated conformance tests |
| Cost and finalized-floor proofs do not run in required CI | High gate gap | Add tiered required gates. | Recorded successful CI jobs |
| Loom models compile but do not run in CI | Gate gap | Run bounded production-adjacent models on each pull request. | CI execution evidence |
| Parser pin guard is not a CI gate | Gate gap | Add the cheap pin and hash check to pull-request CI. | Mutation test and CI evidence |
| Citation auditors have no caller | Gate gap | Run citation and script-caller audits in maintained CI. | Zero unresolved citations |

Formal theorem names must match their statements. A partial proof cannot carry the name of the complete paper theorem.

Shadow models remain useful exploration tools. They do not establish production refinement without an executable bridge.

## Why earlier verification missed these defects

Earlier models proved local algebra and simplified state machines. They did not prove one executable end-to-end refinement.

The composed economic model omitted custody roles. It could prove arithmetic conservation while validator fuel remained transferable and stakeable.

Several models omitted failure transitions, retry ownership, restarts, lifetime exhaustion, unknown discriminants, and multi-parent ancestry.

Pure reductions had zero economic grade. No separate work variable represented host effort or exhaustion.

Some theorem names exceeded their actual statements. Required CI did not compile or check the complete proof corpus.

Several concurrency models used shadow locks or serial transitions. No bridge connected those models to the production asynchronous ownership graph.

Success and idempotence dominated existing tests. They omitted adversarial cross-phase sequences and failures between reservation and durable commit.

## Repair order

The pgmcp epic `pr216-principled-repair-closeout` controls execution. Scheduler revision 37 uses the critical-path policy.

The scheduler orders work as follows:

1. Complete review adjudication and freeze the branch-to-`dev` impact baseline.
2. Merge current remote `dev` and adjudicate every semantic overlap.
3. Repair fail-closed decoding, manifest coverage, and custody aliasing.
4. Repair LFS restore, quarantine, ticket ownership, and recovery lifetimes.
5. Repair validator custody, mint atomicity, epoch retention, and economic conservation.
6. Prove and implement checkpoint and signed-floor semantics.
7. Add deterministic work ceilings and remove proved duplicate work.
8. Bound runtime ownership, startup scans, queues, and soak telemetry.
9. Complete proof bridges, property tests, Loom tests, documentation, and CI gates.
10. Run targeted validation, multi-node integration, and the 24-hour merge-recovery soak.
11. Audit every requirement and evidence record before the pull-request update.

## Performance acceptance

Correctness does not excuse unbounded resource use. Performance defects remain release blockers for this branch.

The work budget must bound pure reduction, primitive work, substitution, authority discovery, physical search, witness decoding, and witness verification.

The validator optimization must preserve independent verification. It must not trust proposer classifications or weaken exact replay.

Runtime ownership must release obsolete roots, runtimes, caches, and anonymous allocations through proved lifecycle transitions.

Measurements must include CPU time, resident memory, witness bytes, block bytes, finalization latency, queue depth, replay count, and state growth.

The 24-hour soak must show forward finality, validator agreement, bounded growth, successful recovery, and no forbidden log condition.

## Preserved design constraints

- Preserve validator and shard parallelism.
- Preserve the Casper protocol unless a proved correctness repair requires a change.
- Preserve strict fault-tolerance threshold semantics.
- Preserve exact, independent replay.
- Preserve physical authority and logical linear ownership.
- Preserve the one native token denomination with distinct custody roles.
- Preserve fail-closed protocol decoding and storage corruption handling.
- Preserve intentional linear, lollipop, and GSLT extension scaffolding.
- Preserve bond-generation and burn tombstones until an equivalent anti-reuse proof exists.
- Preserve protocol-v6 fresh-state activation instead of unsafe implicit migration.
- Avoid duplicate work from the separate MeTTaIL optimization branch.

## Completion rule

A finding is complete only after implementation, formal evidence, generated properties, concurrency tests, documentation, and applicable CI gates pass.

The branch is not merge-ready until the canonical 24-hour soak passes on the final reviewed commit.
