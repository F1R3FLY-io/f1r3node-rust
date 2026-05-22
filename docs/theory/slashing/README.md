# Slashing — Formal Specification and Verification

This directory contains the **complete formal specification and machine-checked
verification of the slashing logic** in the F1r3fly CBC Casper consensus
implementation. The work establishes observational bisimilarity at the process
level between the Rust slashing logic in this repository and the Scala original
from which it was migrated, with explicit, proven *bug-fix deltas* where the
Scala source is faulty.

> **Formal artifacts on this branch.** The normative specification, design
> docs, Rocq theories, TLA+ models, and Sage models are present in this
> repository under `docs/theory/slashing/` and `formal/{rocq,tlaplus,sage}/slashing/`.
> References to `analysis/slashing` identify preserved historical audit
> material, not the current source of the mechanized slashing models.

## Reading order

1. **`design/`** — *Pedagogical design document set* (17 files:
   15 numbered chapters + `14a-tier-architecture.md` + `README.md`,
   in intuitively-named subsections). Read first if you are an
   engineer or auditor approaching the slashing subsystem for the
   first time and want to understand *what each component is, what it
   does, how it does it, and why it was chosen*. Each section leads
   with intuition, then formal definition, then literate-pseudocode
   algorithm, then example, then rationale and references.
   See [`design/README.md`](./design/README.md) for the
   reading order within the design document set.
2. **`slashing-specification.md`** — *Normative contract.* Read second if you
   are an implementer, auditor, or validator operator. Defines components,
   message-passing semantics, the validator lifecycle, the bisimilarity
   statement, the bug-fix manifest, and the use-case catalog. Cites theorems
   by name and `file:line` anchor into the verification doc.
3. **`slashing-verification.md`** — *Proof artifact.* Read third if you are a
   formal-methods reviewer or certifier. A self-contained mathematical article
   stating every theorem with its prose proof, the TLA+ model summary, the
   Rocq↔TLA+ correspondence table, and the trust base. Citations to
   `formal/rocq/slashing/`, `formal/tlaplus/slashing/`, and
   `formal/sage/slashing/` point at the mechanized sources in this branch.
4. **`slashing-threat-model.md`** — *Defensive threat catalog.* Read alongside
   the verification doc to see every modeled attack surface and the protection
   mechanism that covers it. The cross-reference column points at Rust,
   Rocq, TLA+, and Sage artifacts in this branch unless it explicitly says
   it is citing historical audit material.
5. **`diagrams/`** — PlantUML sources and rendered SVGs for the 11 diagrams
   referenced in the spec. Each is embedded inline at its first relevant
   mention in the spec/verification docs; the visuals and the LTS rules
   are designed to stay 1:1.

   | #  | Title                                                                                                  | Embedded at                      | Scope                                   |
   |----|--------------------------------------------------------------------------------------------------------|----------------------------------|-----------------------------------------|
   | 01 | [Slashing subsystem topology](./diagrams/01-component-overview.svg)                                    | spec §3                          | **Exhaustive** — 11 components + 2 data artifacts (Bond map, Coop vault) |
   | 02 | [Admissible equivocation slash flow](./diagrams/02-seq-admissible-equivocation.svg)                    | spec §7                          | Sequence                                |
   | 03 | [Ignorable equivocation slash flow (post-fix)](./diagrams/03-seq-ignorable-equivocation-fixed.svg)     | spec §10.1                       | Sequence                                |
   | 04 | [Two-level slashing](./diagrams/04-seq-two-level-slashing.svg)                                         | spec §8                          | Sequence                                |
   | 05 | [Generic invalid-block dispatch (post-fix)](./diagrams/05-seq-invalid-block-dispatch-fixed.svg)        | spec §7.2, §10.3                 | Sequence                                |
   | 06 | [Validator lifecycle](./diagrams/06-state-validator-lifecycle.svg)                                     | spec §6                          | Statechart (6 observable + 1 doc state) |
   | 07 | [PoS.slash() Rholang activity flow](./diagrams/07-activity-pos-slash-contract.svg)                     | spec §5, §10.4                   | Activity                                |
   | 08 | [Justifications → neglect detection data-flow](./diagrams/08-dataflow-justifications-to-neglect.svg)   | spec §8                          | Data-flow                               |
   | 09 | [Tracker race and locking fix](./diagrams/09-seq-tracker-race-and-fix.svg)                             | spec §10.2; verification §10.8.1 | Sequence                                |
   | 10 | [Specification ↔ Rocq ↔ TLA+ ↔ Rust correspondence](./diagrams/10-component-formal-correspondence.svg) | spec §3; verification §11        | **Proof-bearing only** — components with Rocq/TLA+ artifacts; omits SystemDeployUtil, Bond map, Coop vault (no separate formal counterpart) |
   | 11 | [Withdrawal transfer-failure fix](./diagrams/11-seq-withdrawal-flow-fix.svg)                          | spec §10.10; design §11          | Sequence                                |

   Diagram 01 is the **exhaustive** component diagram; Diagram 10 is
   the **proof-bearing subset** (components with Rocq theorems and/or
   TLA+ models). Both are correct, just at different scopes.

## Scope

| In scope                                                      | Out of scope                                                           |
|---------------------------------------------------------------|------------------------------------------------------------------------|
| Equivocation detection (admissible, ignorable, neglected)     | Cordial Miners / RGB PSSM / Casanova consensus paths (Casper CBC only) |
| `EquivocationRecord` persistence and monotonicity             | Replay protocol details                                                |
| `SlashDeploy` system deploy and `@PoS!("slash", …)` Rholang   | Unconfirmed Rust implementation changes from model-only findings               |
| Two-level slashing (Level 1 + Level 2)                        | Rewriting `test_slash.py` (see `system-integration#51`)                |
| Fork-choice exclusion of slashed validators                   | Replacing PoS multi-sig keys (operations concern)                      |
| Bisimilarity between Rust and Scala (modulo proven bug fixes) | Graduated/proportional slashing penalties (future protocol design)     |
| Sixteen identified bug fixes with proofs of correctness       | End-to-end shard reproduction                                          |

## Source-of-truth correspondence

| Component                  | Specification reference           | Rust source                                                 | Scala reference                                               |
|----------------------------|-----------------------------------|-------------------------------------------------------------|---------------------------------------------------------------|
| Equivocation detection     | `slashing-specification.md` §4    | `casper/src/rust/equivocation_detector.rs`                  | `coop/rchain/casper/EquivocationDetector.scala`               |
| Block validation           | `slashing-specification.md` §3.2  | `casper/src/rust/validate.rs`                               | `coop/rchain/casper/Validate.scala`                           |
| Casper orchestration       | `slashing-specification.md` §3.3  | `casper/src/rust/engine/multi_parent_casper/mod.rs`               | `coop/rchain/casper/MultiParentCasperImpl.scala`              |
| DAG storage                | `slashing-specification.md` §3.4  | `block-storage/src/rust/dag/block_dag_key_value_storage.rs` | `coop/rchain/blockstorage/dag/BlockDagKeyValueStorage.scala`  |
| Equivocation tracker store | `slashing-specification.md` §3.5  | `block-storage/src/rust/dag/equivocation_tracker_store.rs`  | `coop/rchain/blockstorage/dag/EquivocationTrackerStore.scala` |
| Block proposer             | `slashing-specification.md` §3.6  | `casper/src/rust/blocks/proposer/block_creator.rs`          | `coop/rchain/casper/blocks/proposer/BlockCreator.scala`       |
| Slash deploy (system)      | `slashing-specification.md` §3.7  | `casper/src/rust/util/rholang/costacc/slash_deploy.rs`      | `coop/rchain/casper/util/rholang/costacc/SlashDeploy.scala`   |
| PoS Rholang contract       | `slashing-specification.md` §5    | `casper/src/main/resources/PoS.rhox:432-495` (shared)       | same                                                          |
| Fork-choice estimator      | `slashing-specification.md` §3.5.1 | `casper/src/rust/estimator.rs`                             | `coop/rchain/casper/Estimator.scala`                          |

## Headline claims (proved)

- **T-1 / T-2.** Equivocation detection is sound and complete with respect to the
  intended semantics.
- **T-7.** A successful `SlashDeploy` zeros the offender's bond and reward,
  removes them from the active validator set, and transfers the forfeited stake
  to the Coop vault.
- **T-11 / T-12.** Two-level slashing terminates, has exact
  reverse-reachability closure semantics, and preserves count-weighted and
  stake-weighted quorum under the stated closure bounds.
- **T-12C / T-12I / T-12F / T-12G / T-12A / T-12V.** Fixed-point
  closure certificates, active-quorum intersection, current-validator
  and epoch filtering, evidence visibility/report suppression,
  view-indexed closure, duplicate-edge idempotence, cycle edge cases,
  projection-risk witnesses, and safe arithmetic envelopes are stated
  and proved in `slashing-verification.md`; the underlying Rocq and TLA+
  mechanizations are in `formal/rocq/slashing/` and
  `formal/tlaplus/slashing/`.
- **T-12PF / T-5DF.** Hypothesis-backed Sage search results reduce proposer
  evidence-inclusion fairness and
  delimiter-free record-key collisions to deterministic witnesses, promoted
  to theorems in `slashing-verification.md` and use cases in the spec.
- **T-12HYP / deep Sage threat modeling.** Hypothesis frontier and Sage
  threat-ranking results explore
  partition/gossip schedules, objective-guided campaigns, production-shaped
  DAG traces, defensive adversarial vulnerability campaigns, precondition
  fuzzing, Rust replay fixtures, graph attack paths, stake damage,
  retention/pruning, epoch/churn, arithmetic envelopes, exact-vs-runtime
  projection matrices, differential-oracle rows, mutation/metamorphic
  variants, and objective-frontier fixture selection; all findings classify
  as documented boundary, projection-risk, or assumption-counterexample
  witnesses on the audit branch before promotion to this branch.
- **T-9.12–T-9.15.** Current-epoch slash authorization, unknown/stale slash
  evidence no-op behavior, checked sequence arithmetic, and duplicate
  justification rejection are stated and proved in `slashing-verification.md`.
- **T-15.** Under the documented bug fixes, the Rust implementation is
  observationally bisimilar to the Scala original — i.e., no observable
  divergence remains.

## Bug fixes proven correct

| # | Bug                                                          | Theorem |
|---|--------------------------------------------------------------|---------|
| 1 | `IgnorableEquivocation` non-slashable (DOS vector)           | T-9.1   |
| 2 | Lock-free tracker access (Rust regression)                   | T-9.2   |
| 3 | Generic slash dispatcher stub                                | T-9.3   |
| 4 | PoS transfer-failure FIXME                                   | T-9.4   |
| 5 | Stake-0 silent classification                                | T-9.5   |
| 6 | Self-regression slips through `justification_regressions`    | T-9.6   |
| 7 | Off-by-one seq-number density                                | T-9.7   |
| 8 | `prepare_slashing_deploys` did not check proposer is bonded  | T-9.8   |
| 9 | Scala rejects self-correcting blocks (Scala bug, Rust-fixed) | T-9.9   |
| 10 | PoS withdrawal transfer-failure FIXME (analog of #4)        | T-9.10  |
| 11 | Detector missing-pointer abort and duplicate-child over-count | T-9.11 |
| 12 | Unauthorized received slash deploys                         | T-9.13  |
| 13 | Same-key rebond stale-evidence slash                        | T-9.12  |
| 14 | Slash liveness depended on invalid-latest messages          | T-LivenessGap (`deploy_epoch_matches_target`) |
| 15 | Unchecked sequence arithmetic at fixed-width boundaries     | T-9.14  |
| 16 | Duplicate justification projection                          | T-9.15  |

See `slashing-specification.md` §10 for the full bug-fix manifest and
`slashing-verification.md` §9 for the proofs.

## Building and verifying

The mechanized Rocq proofs, TLA+ model-checking instances, and Sage models
live in `formal/{rocq,tlaplus,sage}/slashing/`. Build with:

```sh
# Rocq proofs (use systemd-run resource limits per CLAUDE.md)
systemd-run --user --scope -p MemoryMax=96G -p CPUQuota=1800% \
            -p IOWeight=30 -p TasksMax=200 \
            make -C formal/rocq/slashing -j1

# TLA+ model checking
cd formal/tlaplus/slashing
tlc -workers 12 MC_EquivocationDetector.tla
tlc -workers 12 MC_ConcurrentTracker.tla
tlc -workers 12 MC_SlashFlow.tla
tlc -workers 12 MC_TwoLevelSlashing.tla
tlc -workers 12 MC_AuthorizedSlashFlow.tla
```

On `feature/slashing` (this branch), only the diagram-rendering and
cross-link check are runnable:

```sh
# PlantUML rendering (SVGs are committed; this regenerates them)
for puml in docs/theory/slashing/diagrams/*.puml; do
  plantuml -tsvg "$puml"
done

# Cross-link sanity check
./scripts/check-doc-links.sh
```

## Related documents

- GitHub issue [`F1R3FLY-io/f1r3node-rust#25`](https://github.com/F1R3FLY-io/f1r3node-rust/issues/25)
  — original tracking issue for slashing documentation, test porting, and known gaps.
- `docs/casper/BYZANTINE_FAULT_TOLERANCE.md` — broader BFT model context.
- `docs/casper/CONSENSUS_PROTOCOL.md` — overall consensus protocol description.
- Cost-accounting precedent — methodologically modeled on
  `docs/theory/cost-accounted-rho-verification.md`.
