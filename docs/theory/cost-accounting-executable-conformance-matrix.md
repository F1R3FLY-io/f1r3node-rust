# Executable Conformance Matrix for Cost-Accounted Rho

## Scope

This matrix tracks the native rho instance of the cost construction described
by [Cost-Accounted Rho](../../../publications/cost-accounting/cost-accounted-rho.tex)
and [Continued Interactive GSLTs and the Cost Monad](../../../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex).
The second paper's other example calculi are not additional node languages.
Direct MeTTaIL integration is outside this epic, but the generic GSLT
presentation and OSLF proof-checker interfaces that it must implement are in
scope.

The matrix records executable refinement, not parser recognition alone. A row
is **verified complete** only when its wire representation, normalization,
runtime behavior, persistence, replay or settlement behavior, and focused
formal and Rust evidence agree. **Gate pending** means the implementation and
focused evidence exist, but the final aggregate workspace or multi-node run has
not yet completed on the current worktree.

## Language and wire representation

| ID | Obligation | Executable evidence | Status |
|---|---|---|---|
| E2E-001 | Preserve processes, names, signed terms, and token stacks as distinct sorts. | `Par.cost_signed_terms` and `Par.cost_stacks` in [`RhoTypes.proto`](../../models/src/main/protobuf/RhoTypes.proto); native normalizer and sorters; `signed_terms_and_token_stacks_preserve_their_send_payload_sorts`. | Verified complete |
| E2E-002 | Continuation and payload slots preserve signed terms instead of erasing them to bare processes. | `TaggedContinuation.cost_authority`, `ListParWithRandom.cost_authority`, `CostSignedTerm`; `signed_send_payload_retains_its_authority_when_received_and_run`; payload persistence TLA+/Rocq negative controls. | Verified complete |
| E2E-003 | A wrapper is semantic and contributes authority to every forced COMM in its execution scope. | `reduce.rs::eval_cost_signed_term`, `RhoCommObserver`, scoped `delta_sigma::demand_bound`; reference-counted accounting-scope ownership; overlapping-scope and structural-bound Rust regressions; `AccountingScopeLifetime.tla` and `StructuralAuthorityBound.tla`. | Verified complete |
| E2E-004 | Token stacks are first-class, finite, ordered resources whose consumed head exposes the exact tail. | `CostStack`, `eval_cost_stack`, `supply::read_purse_inventory`, `apply_stack_pops`; `token_stack_send_payload_retains_order_when_received_and_run`; physical-settlement property tests. | Verified complete |
| E2E-005 | Process and signed-term parallel composition retain their respective AC behavior without sort erasure. | Canonical `Par` sorting includes signed terms and stacks; `CAStructEquiv.v`, `SystemStructEquiv.v`; matcher and sorter round-trip tests. | Verified complete |
| E2E-006 | Quote, send, receive, substitution, dequotation, authenticated system bindings, and replay preserve complete cost provenance. | `substitute_cost_signed_term`, `substitute_cost_stack`, matcher payload fields, RSpace datum/continuation fields; certification and execution share `normalizer_env_from_cosigned_deploy`; checkpoint/reset, free-capture, and funded deployer-ID SystemVault replay tests. | Verified complete |
| E2E-007 | Ground, quote, compound, and bound signatures have distinct canonical encodings and domains. | `CostSignature` oneof, canonical sorter, `Sig` conversion, lane hashing, runtime-bound slot substitution, signed authority-presentation validation. | Verified complete |
| E2E-008 | Substitution never dynamically unwraps or rewraps a continuation. | Native substitution preserves wrappers; dispatch begins the forced continuation with its stored wrapper structure; `checkpoint_reset_preserves_payload_sorts_and_continuation_authority`; `WrappingSubjectReduction.v`. | Verified complete |
| E2E-009 | Copies sharing a signature still consume one resource unit per forced copy. | Runtime records each distinct COMM identity and preserves multiplicity even when regions share a signature; `ForcedRedexAccounting.tla`, `repeated_region_occurrences_preserve_multiplicity`, and RuntimeBudget tests. | Verified complete |
| E2E-009A | Unit authority is the zero-demand identity and cannot create a purse lane or physical draw. | Static demand, runtime event demand, and funding-signature discovery erase `Sig::Unit`; explicit paid fixtures install a non-unit payer; Unit neutrality Rust regressions and `RuntimeAuthorityScope.v`. | Verified complete |

## Gated COMM, joins, and linear implication

| ID | Obligation | Executable evidence | Status |
|---|---|---|---|
| E2E-010 | Rule 1 charges one cell for a whole binary redex under one region. | Authorities from both sides are canonically deduplicated by region identity at the pre-mutation observer; `whole_redex_deduplicates_one_shared_region`. | Verified complete |
| E2E-011 | Rule 2 can satisfy a compound region with split component cells atomically. | Exact physical allocation expands compound demand into atoms and searches canonical stack/balance partitions; all-or-none verification and property tests in `authority.rs`. | Verified complete |
| E2E-012 | Rule 3 can satisfy a compound region with one indivisible combined cell. | `CostStack` cells retain compound signatures; no-weakening verification; `combined_signature_remains_one_indivisible_purse_cell`. | Verified complete |
| E2E-013 | Rule 4 charges separately signed receiver and sender regions together. | RSpace merges continuation and datum authorities before reservation; `separately_signed_surfaces_charge_every_distinct_region_atomically`. | Verified complete |
| E2E-014 | Rule 5 accepts a complete combined presentation and rejects partial authority. | Canonical authority presentations are signed deploy data; `verify_physical_settlement` requires exact atom equality; omission, amplification, and partial-debit negative tests. | Verified complete |
| E2E-015 | A whole $N$-ary join under one region charges one region cell regardless of arity. | One RSpace COMM identity covers the selected join; authority regions deduplicate; `AtomicCommAccounting.tla` and join-arity tests. | Verified complete |
| E2E-016 | Separately signed join clauses conserve every receiver and sender authority. | Every signed bind contributes its own region; datum authorities are merged; `signed_join_collects_all_clause_authorities_in_one_comm`; `CAJoinConservation.v`. | Verified complete |
| E2E-017 | Join partitions regroup but neither weaken nor duplicate authority. | Arbitrary contiguous physical partitions are enumerated by property tests; canonical atom multisets and exact draws are witness-bound; `AuthorityPresentation.tla` negative controls. | Verified complete |
| E2E-018 | Split and Join are explicit authority-conserving operations. | `effective_supply_with`, physical allocation, and settlement refuse implicit compound-to-single weakening; `join_no_weakening` and exhaustive partition tests. | Verified complete |
| E2E-019 | $s_1 \multimap s_2$ attributes the rendezvous to $s_1$ and any forced continuation work to $s_2$; chains associate to the right, inert continuations are not charged, and a compound outer signature remains indivisible until an explicit Split. | `desugar::lollipop` creates distinct outer and continuation wrappers; the located-authority lollipop, inert-continuation, chain, and compound regressions; the normalizer right-association regression; `SyntacticSugar.v`, `CAJoinConservation.v`, and `LocatedAuthoritySettlement.tla`. | Verified complete |
| E2E-020 | Continuation authority is independent of producer-first or consumer-first arrival. | Authority is stored on the waiting continuation and dispatch evaluates its signed body; causal RSpace order and schedule-permutation tests; `LocatedAuthoritySettlement.tla`. | Verified complete |

## Located purses, persistent slots, and admission

| ID | Obligation | Executable evidence | Status |
|---|---|---|---|
| E2E-021 | Resource stacks are located at explicit signature surfaces. | `SignatureChannel::from_sig`, first-class `CostStack` data, authenticated purse inventory, and physical stack identifiers bind every draw to a concrete RSpace resource. | Verified complete |
| E2E-022 | Nearness is nominal matching; ambient or fallback authority cannot satisfy an explicit region. | Exact region signatures determine eligible cells; `explicit_region_authority_overrides_the_deploy_default` and `explicit_region_cannot_spend_an_unrelated_default_balance`; Rocq `explicit_regions_do_not_debit_ambient_purse`; TLA+ `NoAmbientAuthority` and its ambient-purse negative control. | Verified complete |
| E2E-023 | A `new`-created funding slot is the actual runtime unforgeable capability. | `BoundLevel` substitution resolves to the generated `GPrivate` value; `bound_slot_identity_is_the_runtime_unforgeable_name`; `wallet_funded_lollipop_slot_settles_across_deploys_and_replays` proves that the persisted continuation signature and public funding address resolve to the same native payer without publishing the capability; `WalletFundedLollipop.v` proves address/capability separation and canonical slot-address identity; runtime-bound TLA+/Rocq models. | Verified complete |
| E2E-024 | A slot and its stack or native vault custody persist across deployments with one replay-stable identity. | RSpace stores `CostStack`; SystemVault stores custody under the address derived from the same signature; `token_stack_persists_across_deploys_and_replays_before_consumption` and `wallet_funded_lollipop_slot_settles_across_deploys_and_replays`; checkpoint/reset and replay assertions; `CrossDeploySlotIdentityStable`; `LocatedStackConservation.tla`; `StackTransferConservation.v`; the composed `WalletFundedLollipop.tla` replay state machine. | Verified complete |
| E2E-025 | Capability possession governs draw and transfer-out while the public native address permits deposit and top-up without ambient access. | The grant contract retains the unforgeable slot behind a persistent public ingress that resolves `rho:system:deployerId` and admits only the configured gateway public key; funding publishes only the derived SystemVault address. An unauthorized call remains outside the located region, cannot consume the one-shot continuation, and is charged to its own envelope. Producing a non-empty stack performs an atomic, fresh-identity source-to-slot transfer before RSpace mutation; native wallet funding is an ordinary conserving SystemVault transfer. Settlement draws only authenticated pre-state resources and rejects `Unit`, duplicate, candidate-minted, underfunded, or unrelated cells. The wallet-to-lollipop regression attempts an unauthorized call before the authorized gateway, then proves slot-lane attribution, fee separation, retained balance, and replay equality. `WalletFundedLollipop.v` proves authentication gating and refinement to slot settlement. `WalletFundedLollipop.tla` composes those native effects and refutes gateway-authentication bypass, capability leakage, custody copying, slot-to-envelope payer collapse, missing outer authority, bound overcharge, and replay omission. | Verified complete |
| E2E-026 | $\Delta_s$ and $\Sigma_s$ are computed per native authority region. | Scoped structural demand follows every enclosing region; unsigned introductions use the envelope lane; authenticated state-bound admission loads balances and stack heads per signature. | Verified complete |
| E2E-027 | Static demand is conservative and state-dependent demand uses a checked dependent witness. | Structural alternatives use point-wise maxima; persistent, recursive, and runtime-bound cases are unprovable structurally; production retains exact bounded execution from the authenticated pre-state. | Verified complete |
| E2E-028 | Admission proves the complete linear plan before committed execution. | `StateBoundAuthoritySession` computes exact evidence, physical draws, certificate, and adjacent roots; canonical residual ledgers prevent shared-purse oversubscription. Exhaustion records the authenticated authority frontier before returning, reverts the speculative attempt, and retries with a strictly larger pre-state-backed capacity. The certificate binds the maximum, while native custody is changed only by the later lexical `SystemVault.applyCost`; `StateBoundFrontierExpansion.tla`, `StateBoundFrontierExpansion.v`, and `AtomicVaultSettlementRefinement.v` prove the refinement. | Verified complete |
| E2E-029 | Rejection changes neither RSpace nor purse state. | Observer reservation precedes tuple mutation; physical failure rolls back the soft checkpoint; `physical_rejection_rolls_back_before_later_state_bound_execution`; `AtomicCommRejection.tla`. | Verified complete |
| E2E-030 | Settlement charges realized authority and leaves the unused maximum available. | Witness event fold, exact physical draws, `stack_pops`, one authenticated `SystemVault.applyCost`, and replay verification. Maximum split, exact burn, conserving fee transfer, and refund are lexical phases of that call; no transient reservation survives in consensus state. The atomic branch-refund example and request property tests cover exactness and rollback. `WalletFundedLollipop.v` proves exact slot debit, distinct gateway fee debit, proposer credit, refund, conservation, and no mint across the composed wallet-funded continuation. | Verified complete |
| E2E-031 | The deployment is the financial-atomicity boundary. | Candidate-minted resources cannot self-fund; exhaustion cannot certify; scalar-capacity and per-lane allocation failures expose only authenticated authority and commit no event, debit, or RSpace mutation. Exact bounded play, located-stack pops, and `applyCost` share one node checkpoint and are either retained as a whole or rolled back; state-bound, located-transfer, and partial multi-purse failure regressions cover the boundary. | Verified complete |
| E2E-031A | An attempted state-bound funding rejection is terminal, consensus-visible, and effect-free. | `ProcessedDeploy.admission_status`; proposal/validation partition reconstruction from the block pre-state; `FundingAdmissionLifecycle.tla` and Rocq capstone; terminal-rejection serialization, forged-rejection, rollback, and duplicate-occurrence regressions. | Verified complete |

## Consensus, replay, merge, and activation

| ID | Obligation | Executable evidence | Status |
|---|---|---|---|
| E2E-032 | Authority metadata is consensus-visible and byte-canonical. | Versioned protobuf certificate, witness, resource, event, stack-reservation, and physical-draw messages in [`CasperMessage.proto`](../../models/src/main/protobuf/CasperMessage.proto); canonical conversions and hash domains. | Verified complete |
| E2E-033 | Datum and continuation storage preserve future COMM authority. | `ListParWithRandom.cost_authority` and `TaggedContinuation.cost_authority` flow through hot store, history, matching, checkpoint/reset, and replay; payload-sort tests. | Verified complete |
| E2E-034 | A match reserves one canonical authority plan before any tuple or log mutation. | `RhoCommObserver::observe` merges both sides, instantiates persistent occurrences, and calls `reserve_comm_authority_identity` at RSpace's atomic observer boundary. | Verified complete |
| E2E-035 | Certification, play, ordinary replay, and approved-state historical replay normalize the same authenticated deployment and consume identical cells to produce identical balances. | All phases derive deployer/cosigner bindings from the verified envelope; certificate and witness bind canonical program, pre/post roots, event authorities, stack IDs, and draws. Replay verifies the complete presentation before applying pops; `replay_block_from_consensus_data` derives historical context from each block rather than the joiner's current tip; deployer-ID SystemVault replay, late-checkpoint integration, `NormalizerEnvironmentRefinement`, and `ApprovedStateReplay` cover the boundary. | Verified complete |
| E2E-036 | Arrival, reducer, and parent order cannot change accepted cost state. | Causal event order, canonical deploy order, immutable state snapshots, widened simultaneous number-channel aggregation shared by selection/application, completed-branch merge over exact durable purse deltas, and least-fixed-point exact-effect rejection. `MergeAggregateAgreement.tla`, `AtomicVaultSettlementRefinement.tla`, `EffectCausalClosure.tla`, generated permutation tests, replay differential tests, the funded sibling regression, and full-DAG finality traversal cover the boundary. | Verified complete |
| E2E-037 | Merge neither duplicates nor drops settlement removals or residual cells. | Located-stack pops are recorded as RSpace removal events and indexed in state change; SystemVault settlement leaves only mergeable exact balance deltas, not a singleton reservation datum. Exact rejection propagates only through byte-identical ordinary datum/continuation dependencies and transitively closes; independent exact effects survive, while only legacy witnesses use conservative block-lineage expansion. Immutable copy-on-write merge, `SettlementMergeVisibility.tla`, `EffectCausalClosure.tla`, Rocq, Loom, and same-payer sibling tests cover visibility and aggregation. | Verified complete |
| E2E-038 | Mixed authority-accounting semantics cannot validate the same certificate. | Protocol version 7 and domain-separated certificate/witness IDs cover stack reservations and physical draws; replay rejects version, proof, root, or presentation mismatch. | Verified complete |
| E2E-039 | Genesis and exchange provision authority without double credit or unintended minting. | Canonical SystemVault allocations are embedded in the blessed genesis contracts and reconstructed during ceremony and replay; unit authority covers bootstrap execution; ordinary settlement cannot reapply genesis funding; `protocolMint` is authenticated and epoch-idempotent; the blessed Exchange conserves both sides and cannot credit SystemVault custody. | Verified complete |

## GSLT, OSLF, and refinement boundary

| ID | Obligation | Executable evidence | Status |
|---|---|---|---|
| E2E-040 | Cost acts on the concrete rho ciGSLT and preserves quote-faithful behavior. | `GsltPresentation`, `RhoGslt`, native signed wire sorts, force-by-unwrapping runtime, `CACostFunctorCI.v`, `CATranslationFaithfulness.v`, and runtime correspondence tests. | Verified complete |
| E2E-041 | Cost-monad unit and multiplication preserve ordered, non-idempotent stacks. | Nested wrappers flatten to canonical compound authority only at explicit multiplication; stack order is preserved; `CostMonad.v`, `CACostMonadCat.v`, normalizer and stack tests. | Verified complete |
| E2E-042 | Abstract categorical results, native refinement, and future integrations are not conflated. | [`cost-accounting-as-monad-correspondence.md`](cost-accounting-as-monad-correspondence.md) and the end-to-end design separate theorem, native implementation, and integration boundaries. | Verified complete |
| E2E-043 | The native OSLF boundary checks proof-bearing resource formulas from a generic GSLT presentation. | `OslfResourceLogic<G>::resource_observation/check_formula` operate over associated program/signature types; fake-GSLT conformance tests exercise exact evidence, while `DefaultResourceLogic` projects native Rho's conservative structural reservation. | Verified complete |
| E2E-044 | Local purse sufficiency composes into global sufficiency. | Point-wise multisets, `Formula::Located/Spatial`, exact event allocation, and canonical residual ledgers implement `CAOSLFSpatialModal.spatial_local_sufficiency_composes` and `CALocatedPurses.local_sufficiency_composes`; Rust property tests commute arbitrary disjoint local spends. | Verified complete |
| E2E-045 | Direct MeTTaIL integration later satisfies the same traits without changing node consensus semantics. | The traits and formula evaluator are representation-generic; `CAOSLFSpatialModal.v`, `OslfLocatedTyping.tla`, and the Rust conformance suite define the semantics a generated adapter must refine. No MeTTaIL dependency or representation assumption is present. | Scope boundary |
| E2E-046 | Linear, copyable, and relevant usage are opt-in checked disciplines rather than one hardwired operational policy. | `UsageDiscipline` builds exact-single-use-plus-spend, unconstrained-copyable, or spend-required-relevant formulas. Rust examples, Rocq no-contraction/no-weakening theorems, and TLC/Apalache contraction and weakening controls agree. | Verified complete |
| E2E-047 | A graded spend proves both availability and the exact post-state. | `Formula::Spend` consumes the grade from exact supply and demand before checking its continuation; `modal_poststate_is_exact`, `ModalPoststateExact`, and focused Rust tests cover the transition. | Verified complete |
| E2E-048 | Conservative demand proves safety but cannot be fabricated into evidence that an interaction occurred. | `DemandKnowledge::UpperBound` may satisfy `Sufficient`; positive `Required` and feasible `Spend` remain `Indeterminate`. `conservative_sufficiency_is_sound`, `upper_bound_cannot_assert_modal_spend`, and the Apalache upper-modal control cover the distinction. | Verified complete |
| E2E-049 | Candidate-created authority cannot fund the candidate's own OSLF judgment. | `rho_observation` uses authenticated pre-state supply and `external_reservation` without `guaranteed_program_supply`; the Rust regression, `authenticated_supply_excludes_candidate_credit`, and TLC/Apalache candidate-credit control reproduce the unsafe alternative. | Verified complete |

## Release evidence

| ID | Required evidence | Current evidence | Status |
|---|---|---|---|
| E2E-046 | Integrated TLA+ safety/liveness plus named unsafe controls. | State-bound admission, authenticated normalizer environment, authenticated frontier expansion, validator convergence, located settlement, stack-transfer conservation, stack-safe independent physical search, payload persistence, settlement visibility, per-deploy trace segmentation, widened merge aggregation, transitive exact-effect rejection closure, atomic native vault settlement, the composed native wallet-funded lollipop workflow, genesis execution/replay authority symmetry, structural authority bounds, overlapping accounting-scope lifetime, approved-state block-bound replay, deferred local-fault recovery, and terminal funding admission all have safe configs and expected refutations. The rejection-closure controls independently refute blanket block-lineage deletion and one-hop-only propagation; the wallet workflow's controls refute custody copying, capability leakage, payer collapse, missing outer authority, certified-bound overcharge, and replay omission. | Verified complete |
| E2E-047 | Axiom-free Rocq boundary theorems. | The proof tree includes end-to-end authority, authenticated normalizer-environment equality, recursive-tree/worklist physical-search equivalence, runtime-bound slots, located-stack transfer conservation, authenticated-frontier expansion, payload persistence, settlement visibility, structural-demand soundness, abstract reserve/settle-to-native-atomic refinement, and composed wallet-to-slot-to-lollipop conservation and payer separation; `rocqchk` and `Print Assumptions` are mandatory. | Verified complete |
| E2E-048 | Example-based Rust tests for every rule and lifecycle. | Normalizer, reducer, authority, stack-safe 4,096-event physical search, stack transfer, duplicate identity, allocation exhaustion, state-bound expansion, cross-deploy persistence, wallet-funded lollipop settlement, rollback, replay, merge, native atomic SystemVault conservation, partial multi-purse rollback, and funded same-payer sibling examples exist in `rholang` and `casper`. | Verified complete |
| E2E-049 | Property and concurrency tests. | Proptest covers arbitrary partitions, mixed-event worklist ordering and exact debit, witness bounds, scoped introduction counts, serialization, algebra, 512 generated merge-contribution permutations, indexed-versus-pairwise physical dependency equivalence, arbitrary transitive rejection depth, atomic-request permutation invariance, and realized-overdraw rejection. Loom covers budget ownership, duplicate stack transfer, frontier discovery, and settlement-removal races; Sage, TLC, and Apalache exhaustively search bounded stack-transfer, frontier-expansion, independent physical-worklist, atomic-vault, completed-branch merge, and exact-effect classification schedules. | Verified complete |
| E2E-050 | RSpace persistence and replay tests. | Hot-state reads, checkpoint/reset, payload sorting, cross-deploy stack persistence, stack-pop events, per-deploy soft-checkpoint trace segmentation, exact historical-context replay, late-checkpoint root reconstruction, and local-fault descendant gating are covered. Independent-validator and isolated-reporting regressions prove that a multi-deployment replay materializes each intermediate root before the next ordinary-RSpace purse read. Eager future-root reads, producer-history dependence, ReplayRSpace authority queries, cumulative traces, and current-context historical replay are expected TLA+ refutations. | Verified complete |
| E2E-051 | Multi-node consensus integration. | The system-integration pin contains convergence, DAG correctness, pause/recovery, heartbeat, deployment, readonly, and asymmetric-divergence suites; the current aggregate rerun remains required after the latest repair. | Gate pending |
| E2E-052 | Formatting, lint, unit, formal, dependency, and aggregate release gates. | Focused Rust, TLA+, and Rocq checks pass; full workspace and system-integration gates remain to be run on the final worktree. | Gate pending |
| E2E-053 | The downstream Python client exposes the exact node schema and typed cost-authority workflows needed by applications. | Generated protobufs must include authority presentations, funding certificates/witnesses, admission status, cost-bearing Rho sorts, and rejection provenance while preserving canonical SystemVault genesis funding without a separate supply payload. Typed slot, exchange, capability, and terminal-status helpers require client unit tests and a pinned system-integration exercise before Embers consumes them. | Gate pending |
| E2E-054 | Independent nodes replay paid multi-deployment blocks without relying on proposer-local RSpace history. | Production and reporting replay read each purse snapshot from an ordinary runtime at the current locally materialized root, replay one committed deployment, verify and checkpoint its recorded post-state, then continue. TLA+ proves materialization, runtime separation, agreement, and progress with three expected refutations; Rocq proves root-prefix and terminal-root equality; isolated Rust histories reproduce the producer/validator asymmetry and verify the repair. | Verified complete |

## Normative runtime invariants

For every authority lane $s$, structural admission establishes a finite bound
$B_s$ or returns `Unprovable`. A successful non-persistent COMM charged to $s$
has at least one distinct introduction whose enclosing scope contains $s$.
Consequently:

$`\kappa_s \leq B_s \leq \Sigma_s`$.

For native production admission, the authenticated pre-state may contain
persistent continuations or data that submitted syntax cannot bound. The node
therefore retains a capacity-bounded execution witness and uses its exact
realized multiset $\kappa$ as the certificate demand. Candidate-created stacks
are excluded from the pre-state inventory, and physical settlement must realize
each event's complete authority atom multiset. Settlement is:

$`\Sigma'_s = \Sigma_s - \kappa_s - \operatorname{fee}_s`$.

The unused reservation remains available; it is never injected back into the
runtime budget. Replay reconstructs the same certificate, causal events,
physical draws, stack pops, and adjacent roots. Any mismatch is objective block
invalidity. Local missing-history or storage failures remain recoverable local
faults and cannot create slash evidence.

The accounting scope is an execution-context ownership boundary, not a feature
flag or an A/B path. Every user deployment and its replay enter the same scoped
interpreter path. Direct reducer calls used for bootstrap construction and
low-level tests have no deploy payer; when no explicit signed surface exists,
their absent authority is the canonical wire erasure of the multiplicative
unit. An explicit signed surface is preserved in every context. Scope ownership
is reference-counted so overlapping evaluations cannot deactivate one another.

## Required final commands

```bash
scripts/check-cost-accounted-rho-ALL.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The aggregate formal runner is strict by default. It treats a missing tool,
advisory proof result, skipped model, failed safe model, or missing named
counterexample as a failure.
