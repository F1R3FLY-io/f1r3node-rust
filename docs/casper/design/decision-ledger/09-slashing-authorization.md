# D-09 Slashing Authorization, Evidence Identity, and Neglect

**Status.** Proposed. Pending maintainer ratification.

**Kind.** Protocol.

**Sources.**

- dev [Consensus Protocol](../../CONSENSUS_PROTOCOL.md) section 9, [slashing specification](../../theory/slashing/slashing-specification.md) sections 4, 8, and 9, [`formal/rocq/slashing/`](../../../../formal/rocq/slashing), [`formal/tlaplus/slashing/`](../../../../formal/tlaplus/slashing).
- PR #216 DR-3, DR-7, DR-8, DR-18, `slashing-specification.md` sections 8, 9, and 15, `slashing_authorization.rs`, rules R-ADMISSION-CLOSURE, R-EVIDENCE-TRAVERSAL, R-EVIDENCE-CANONICAL, models `ObjectiveEquivocation.tla`, `ObjectiveEvidenceAuthorization.tla`, `CertifiedRejectionDependency.tla`, `TwoLevelSlashing.tla`, `SlashFlowProofs.tla`.

## 1. Question

What authorizes a slash deploy, what identifies the offender, and does a validator that neglects known equivocation lose stake?

## 2. Position on dev

- Honest validators emit a `SlashDeploy` from their `invalid_latest_messages`. Only bonded validators with non-zero stake can be slashed.
- **Rejected-slash recovery.** A slash issued in one parent can lose the merge. The proposer runs `compute_parents_post_state`, deduplicates rejected slashes by invalid block hash, and re-issues each survivor under its own identity. PoS idempotency makes the re-issue safe.
- The slash RNG seed is `(validator, sequence, invalid_block_hash)`.
- **Two-level slashing.** Level 1 loses the entire stake. Level 2, the neglect case, rejects the block. The stake penalty is inactive. The neglect verdict is view-relative, so it is demoted from evidence minting until shown admission-order-free. This is the slashing-specification amendment of 2026-08-24.
- The headline correctness claim is a Rust to Scala bisimilarity theorem. `main_bisimilarity_theorem` and `main_bisimilarity_strong` are in the CI assumption check. `MC_SlashFlow` is in the TLA+ gate.

## 3. Position on PR #216

- **Canonical reconstruction.** Before assembly the proposer computes the canonical pre-state from its parents. `prepare_slashing_deploys` scans durable objective sibling pairs first, then the unary invalid-block evidence index, and authorizes candidates against the bond map of that pre-state. At most one candidate per `(offender, activation epoch)`. A merge-rejection record is diagnostic evidence, never authorization.
- **Objective evidence.** An objective sibling pair is two distinct blocks with one sender and sequence number in one validator lifetime. It is independent of local invalid flags. Both hashes are dependencies. The pair seed sorts both hashes so arrival order cannot change it.
- **Validator lifetime.** A validator incarnation is `(key, bond generation)`. Generation changes only after a completed withdrawal and a fresh bond. Evidence for one generation cannot slash another. Evidence outside the current activation epoch cannot be relabeled current.
- **R-ADMISSION-CLOSURE.** A block waits until metadata exists for every evidence hash in its causal closure. A mutable receiver-local tracker cannot satisfy a dependency.
- **Neglect.** Section 8 of the slashing specification becomes a counterfactual analysis. The current protocol rejects a neglecting block and mints no economic evidence. `TwoLevelSlashing.tla` keeps an `EconomicNeglectSlashing` constant set to FALSE in the normative configuration. The PR #216 protocol document nonetheless says a neglecting validator also loses stake.
- **DR-8.** The Rust to Scala bisimilarity theorems are removed. `Bisimulation.v` is deleted. The assumption check names `main_slashing_algorithm_correct`. `MC_SlashFlow` moves to an exhaustive tier. `MC_SlashFlowRedeem` and TLAPS proofs gate instead.
- **DR-3, DR-7, DR-18.** Two-effect slashing with quarantine and redemption, redemption authority by PoS multisig, and Burned as a terminal state. These are economic layer decisions and stay out of this ledger's scope.

## 4. Divergence

| Aspect | dev | PR #216 |
|---|---|---|
| Authorization source | Own invalid latest messages plus merge-rejected slash hints | Canonical evidence scan against the merged pre-state |
| Merge-rejected slash | Re-issued as new authority | Diagnostic only. Persisted evidence reconstructs the candidate. |
| Offender identity | Validator key | `(key, bond generation)` plus activation epoch |
| Evidence shape | Unary invalid block hash | Objective sibling pair first, unary fallback |
| Level-2 penalty | Inactive, demoted pending re-promotion | Inactive by constant. Protocol document text contradicts this. |
| Correctness anchor | Bisimilarity with Scala | `main_slashing_algorithm_correct` |
| Gate | `MC_SlashFlow` | `MC_SlashFlowRedeem` plus TLAPS |

## 5. Options

- **A. Adopt canonical reconstruction, lifetime identity, and objective evidence. Keep level 2 inactive. Accept the new correctness anchor.**
- **B. Adopt canonical reconstruction and keep the bisimilarity anchor until a replacement statement is accepted.**
- **C. Keep dev.**

## 6. Unification proposal

Adopt option A for authorization, identity, and neglect. Treat the correctness anchor as a governance sub-decision under entry D-11.

**9.1, authorization from canonical evidence.** Ratify. A merge-rejection record is not consensus authority. Deriving the slash from persisted evidence and the merged pre-state follows principle P2 and removes the dependence on a proposer's own tracker.

**9.2, validator lifetime.** Ratify the identity principle. Binding evidence to a bond generation prevents stale evidence from slashing a rebond. The wire form of pair evidence depends on D-01.

**9.3, neglect penalty.** Ratify that level 2 stays inactive. Both branches agree in substance. The PR #216 protocol document must be corrected before the row flips.

**9.4, correctness anchor.** Deferred to D-11. Removing a gated proof needs a named replacement and a maintainer decision on whether the Scala reference remains the specification anchor.

## 7. Ratification checklist

- 9.1: `ObjectiveEvidenceAuthorization.tla` and `CertifiedRejectionDependency.tla` in the gate list. A regression where a parent slash loses the merge and the canonical scan reconstructs it.
- 9.2: the epoch and generation filtering examples from the slashing specification section 15.
- 9.3: the corrected protocol document text on PR #216.
- After ratification, replace the protocol section 9 flow and recovery text, and the slashing specification sections 3 and 8.

## 8. Open questions

1. Does the canonical scan reproduce every slash that the rejected-slash loop reproduces today? The sources claim equivalence in intent, not by test.
2. Does the pair evidence change the `SlashDeploy` wire shape, and does that change wait for protocol 6?
