# Cost-Accounting Branch: CbC and Formal-Verification Reconciliation

**Status:** Review record. This document decides nothing.
**Date:** 2026-09-05
**Compared:** `dev` at the PR #382 merge (`231067178`), PR #387 at `30c428335`, and PR #216 (`feature/cost-accounted-rho`) at `3980ed402`.
**Related:** [Consensus Philosophy](../CONSENSUS_PHILOSOPHY.md), [CbC repair plan](./cbc-repair-plan.md), [formal-verification.md](../../formal-verification.md).

## 1. Purpose and scope

This document records how PR #216 and PR #387 each change the Correct by Construction (CbC) ledger and the formal-verification (FV) practice of the Casper consensus. It separates decisions that `dev` has ratified from decisions that a branch introduces without a ratification record.

The scope is limited to CbC and FV artifacts. Economic semantics, protocol-version changes, and merge-conflict resolution are out of scope. A later change will use this record to bring the cost-accounting work into `dev`.

Paths under `formal/` and `docs/casper/theory/` that exist only on PR #216 appear as code spans, not links, because they are not present on this branch.

## 2. Baseline on dev

The `dev` branch holds three CbC and FV mechanisms.

- **Mandatory scope.** `.gitattributes` tags production artifacts with `cbc=mandatory`. Section 7.1 of the Consensus Philosophy names four ratified artifacts for phase 1.
- **Claim and evidence ledger.** `docs/claims/` holds six claim documents. `docs/cbc-evidence/` holds sixteen evidence records: two discharged and fourteen waived.
- **FV practice.** `docs/formal-verification.md` states five practice rules. Rule 1 requires violation configurations beside each gating configuration. Rule 3 says a refuted required claim blocks completion. Rule 4 requires cross-view claims for distributed decisions. Rule 5 says consensus liveness includes resource bounds.

The continuous-integration gate is `scripts/ci/check-formal-invariants.sh`. It runs `scripts/ci/check-tla-invariants.sh` over sixteen post-fix TLA+ configurations and rebuilds three Rocq projects, `slashing`, `fork_choice`, and `rspace_guards`, with assumption checks on named theorems. Expected-violation configurations stay outside the gating list.

## 3. PR #387 delta

PR #387 extends the existing mechanisms without changing them.

| Mechanism | Change |
|---|---|
| Mandatory scope | Adds `casper/src/rust/validate.rs`, `block-storage/src/rust/dag/carrier_index.rs`, and `block-storage/src/rust/dag/block_dag_key_value_storage.rs` to `.gitattributes`. Adds section 7.2 to the Consensus Philosophy. |
| Claim ledger | Adds [`CLAIM-FINALITY-002`](../../claims/repeat-deploy-carrier-index-equivalence.md) with status `pending` and seven claim statements C1 to C7. |
| Formal model | Adds `formal/tlaplus/carrier_index/CarrierIndex.tla` with two invariants, `IndexCompleteForWindow` and `AbsenceProofSound`, and two negative controls. |
| CI gate | Registers `carrier_index/MC_CarrierIndex` in the TLA+ gating list. |
| Umbrella doc | Adds one row to the verified-areas table. |
| Repair plan | Adds telemetry counters, a forced on and off differential, and the rule that a passing model for existing behavior is baseline evidence, not the RED test. |

The decision table row dated 2026-09-03 marks the scope extension as ratified for `CLAIM-FINALITY-002`. The PR author wrote that row. Maintainer acceptance is pending.

## 4. PR #216 delta

### 4.1 Governance model

PR #216 adds a second governance model beside the Consensus Philosophy.

- `docs/casper/theory/cost-accounting-decision-records.md` holds fifty-seven decision records, DR-1 to DR-57. Its preamble names the cost-accounting paper as the law of the implementation. Records use the statuses `accepted and implemented`, `superseded`, and `user-ratified`. The file does not use the Consensus Philosophy decision table.
- `docs/casper/theory/cost-accounting-executable-conformance-matrix.md` maps obligations to executable evidence. Every matched status cell reads `Verified complete`.
- `formal/README.md` defines a seven-item completion criterion for a formal area: a substantive source, a checked safe configuration, a checked unsafe control per defect, a model-to-code map, tests for the transition, an executing gate, and documentation of bounded assumptions.

The branch also edits the FV practice section of `docs/formal-verification.md`. It removes practice rules 3, 4, and 5. It removes the sentence that keeps expected-violation configurations outside the gating list. The cause can be a deliberate edit or a merge-resolution loss. The diff alone cannot decide.

### 4.2 CbC ledger

PR #216 does not change `.gitattributes`, `docs/cbc-evidence/`, or section 7.1 of the Consensus Philosophy. It edits one claim, [settled-effect-probe-equivalence](../../claims/settled-effect-probe-equivalence.md), to add a protocol scope note and to count a failed body with verified SystemVault settlement as an applied effect.

The branch adds thirty-six production Rust files. The `casper/src/rust/finality/**` glob covers two of them. The following new files in the `casper` and `block-storage` crates carry no CbC attribute and no claim:

- `block-storage/src/rust/dag/deploy_occurrence_store.rs`
- `block-storage/src/rust/dag/deploy_occurrence_types.rs`
- `block-storage/src/rust/deploy/pending_deploy.rs`
- `block-storage/src/rust/finality/finalization_ledger.rs`
- `block-storage/src/rust/finality/state_preservation.rs`
- `casper/src/rust/blocks/block_processing_queue.rs`
- `casper/src/rust/causal_equivocation.rs`
- `casper/src/rust/engine/finalization_certificate_retriever.rs`
- `casper/src/rust/util/rholang/acceptance.rs`
- `casper/src/rust/util/rholang/supply.rs`
- `casper/src/rust/util/rholang/costacc/redeem_deploy.rs`
- `casper/src/rust/util/rholang/costacc/vault_cost_deploy.rs`
- `casper/src/rust/util/rholang/costacc/vault_payer.rs`

The evidence for these files lives in conformance-matrix rows and theory dossiers, not in the CbC ledger.

### 4.3 Formal artifacts

PR #216 adds about 1,550 files under `formal/`. The table lists the areas that touch Casper consensus.

| Area | TLA+ files | Rocq files | Note |
|---|---|---|---|
| `tlaplus/finalized_floor` | 566 | 47 (`rocq/finalized_floor`) | Certified floor, finalization ledger, restore horizon, certificate carriers |
| `tlaplus/cost_accounted_rho` | 391 | 124 (`rocq/cost_accounted_rho`) | Cost authority, settlement, admission |
| `tlaplus/deploy_recovery` | 79 | none | Carrier index soundness, protocol lifecycle, rejection reasons |
| `tlaplus/slashing` | 62 | 22 (`rocq/slashing`) | Objective evidence, redemption |
| `tlaplus/block_admission` | 42 | none | Byte-bounded admission, transport residency |
| `tlaplus/deterministic_parallel_reduction` | 33 | included above | Intra-deploy reduction order |
| `tlaplus/fork_choice` | 24 | 15 (`rocq/fork_choice`) | Certified context, eligibility |
| `loom/cost_accounting` | 35 Rust files | none | Concurrency shadow models, added as a workspace crate |

Across `formal/tlaplus`, the branch adds 990 configuration files. About half of them, 497, are unsafe controls or pre-fix reproductions. The branch removes one Rocq file, `formal/rocq/slashing/theories/Bisimulation.v`, per DR-8. It adds eleven tool families: Lean, Isabelle, Iris, mCRL2, Storm, ProVerif, Tamarin, Verus, Why3, rewriting, and Loom.

The carrier index has two independent models. PR #387 adds `CarrierIndex.tla` with two invariants. PR #216 adds `deploy_recovery/CarrierIndexSoundness.tla` with thirteen invariants, five unsafe controls, two validators, and typed key domains. The models agree on write ordering, absence soundness, and read-failure refusal. They differ on the key, body signature against protocol-tagged deploy identity.

### 4.4 CI gates

PR #216 rewrites the TLA+ gating list in `scripts/ci/check-tla-invariants.sh`. It removes six `dev` entries and adds about twenty-six entries.

| Removed from the gate | Replacement on PR #216 |
|---|---|
| `slashing/MC_SlashFlow` | Moved to an exhaustive tier. `slashing/MC_SlashFlowRedeem` gates instead. |
| `deploy_lifecycle/MC_DeployLifecycle` | `deploy_occurrence/MC_DeployOccurrence` and `MC_DeployOccurrenceStorage` |
| `fork_choice/MC_ForkChoice` | None. The model remains on disk. |
| `fork_choice/MC_PromotionConvergence` | None. The model remains on disk. |
| `recovery_leader/MC_RecoveryLeader` | `finalized_floor/MC_RecoveryCommitteeTransition` |
| `replay_liveness/MC_ReplayHotLoop` | None. The model and its claim remain on disk. |

PR #387 registers `carrier_index/MC_CarrierIndex`. PR #216 registers `deploy_recovery/MC_CarrierIndexSoundness`. An integrated gate must decide whether to keep one or both.

The Rocq assumption check for `slashing` changes from two bisimilarity theorems to one theorem, `main_slashing_algorithm_correct`. The `fork_choice` and `rspace_guards` checks are unchanged. The 124 Rocq files under `rocq/cost_accounted_rho` and the 47 under `rocq/finalized_floor` are not in the CI gate. The branch adds more than forty `scripts/check-cost-accounted-rho-*.sh` and `scripts/check-*-ALL.sh` scripts. No workflow invokes them.

The property-test tiers change. The pull-request tier drops from 10,000 cases to 2,000 bulk cases. The nightly tier drops from 100,000 cases to 10,000 cases. The TLC launcher gains a bounded heap, worker count, and memory ceiling.

### 4.5 Umbrella document

The verified-areas table in `docs/formal-verification.md` gains ten cost-accounting rows. It drops three rows: recovery leader, replay liveness, and promotion convergence. The models remain on disk, and the replay-liveness claim remains in `docs/claims/`.

The theory index at `docs/casper/theory/README.md` is unchanged. It does not list the cost-accounting, deploy-occurrence, or uptime dossiers.

## 5. Decided versus unratified

The table classifies each CbC or FV position on PR #216 against `dev`.

| Item | dev status | PR #216 position | Classification |
|---|---|---|---|
| Phase-1 CbC scope, four artifacts | Ratified 2026-08-22 | Unchanged | Compatible |
| CbC attributes for new consensus files | Ratified mechanism | Thirteen new files untagged | Gap. Needs attributes and claims or a recorded waiver. |
| FV practice rules 3 to 5 | Stated on `dev` since 2026-08 | Removed | Conflict. Needs a decision or a restore. |
| Expected-violation configs outside the gate | Stated on `dev` | Statement removed. Unsafe controls remain outside `POST_FIX_CONFIGS`. | Text conflict, practice compatible |
| Decision-record governance with the paper as law | No record | Introduced | Unratified. Needs a Consensus Philosophy row that relates DR statuses to table statuses. |
| Formal-area completion criterion, seven items | No record | Introduced in `formal/README.md` | Unratified, compatible with rules 1 and 2 |
| Slashing bisimilarity theorems | Gated in CI | Removed per DR-8 | Unratified removal of a CI check |
| Six TLA+ gate entries | Gated in CI | Removed | Unratified removal. Three have no replacement. |
| Property-test case counts | 10,000 and 100,000 | 2,000 and 10,000 | Unratified reduction |
| Carrier-index model | None | `CarrierIndexSoundness.tla` gated | Compatible with PR #387. Two models need one decision. |
| Carrier-index key | Body signature, pending ratification 2026-09-01 | Protocol-tagged deploy identity | Conflict with the PR #387 claim text |
| Settled-effect claim `applied` predicate | Claim on `dev` | Widened to settled failed bodies | Unratified claim change |
| Proof-to-code obligation scripts | `scripts/ci` only | Forty scripts outside workflows | Unratified. Needs a wiring decision. |

## 6. Conflict points between PR #387 and PR #216

Only two files conflict textually between the branches: `docs/casper/CONSENSUS_PHILOSOPHY.md` and `docs/formal-verification.md`. Four points conflict in meaning.

1. Both branches edit the 2026-09-01 decision row. PR #387 marks the index implemented with ratification pending. PR #216 rewords the row for typed keys.
2. Both branches add a carrier-index row to the verified-areas table and a gate entry for a different model.
3. Both branches edit the carrier-index section of the [CbC repair plan](./cbc-repair-plan.md). PR #387 adds telemetry and differential requirements. PR #216 adds typed keys, atomic v6 admission, and the bounded decoded-identity cache.
4. `CLAIM-FINALITY-002` states its predicate over body signatures. PR #216 keys the index by deploy identity.

## 7. Open questions

The integration change must answer these questions before it edits any specification.

1. Which artifacts on PR #216 join the mandatory CbC scope, and which receive a recorded waiver?
2. Do practice rules 3 to 5 return to `docs/formal-verification.md`?
3. Which of the six removed TLA+ gate entries return, and does `MC_CarrierIndex` or `MC_CarrierIndexSoundness` gate the carrier index?
4. Do the cost-accounting Rocq projects join `check-formal-invariants.sh`, and does any workflow run the `check-cost-accounted-rho-*.sh` scripts?
5. Does the decision-records file become a Casper decision source, and how does a DR status map to a Consensus Philosophy table status?
6. Does the property-test case reduction stand?
7. Does `CLAIM-FINALITY-002` adopt the protocol-tagged key before or after integration?
