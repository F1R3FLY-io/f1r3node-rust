# D-04 State-Preserving Finality and Effect Provenance

**Status:** Proposed. Pending maintainer ratification.
**Kind:** Protocol.
**Sources:** dev [Consensus Protocol](../../CONSENSUS_PROTOCOL.md) section 7, [finalized-floor specification](../../theory/finalized-floor/finalized-floor-specification.md) R-FLOOR and R-SNAP, [settled-effect probe claim](../../../claims/settled-effect-probe-equivalence.md), [`formal/tlaplus/finalized_floor/`](../../../../formal/tlaplus/finalized_floor). PR #216 DR-43, DR-44, DR-45, DR-46, DR-54, DR-57, rules R-FLOOR, R-STATE-CERT, R-FLOOR-STATE, R-LFB-STATE, R-EFFECT-ID, R-EFFECT-WIRE, R-EFFECT-ACTIVE, R-STATE-PRESERVATION, R-UNIVERSAL-FRONTIER, R-COVERAGE-EQUIVALENCE, R-FINALIZER-SNAPSHOT to R-FINALIZER-YIELD, R-FINALIZATION-CLOSURE, R-SHARD-FINALITY, R-REBASE, R-VALIDITY-STABILITY, invariants S24 to S31 and S41, models `StateEffectProvenance.v`, `CertifiedFloorPromotion.tla`, `FinalizerFloorMaterialization.tla`, `LatestMessageCoverage`.

## 1. Question

What does a finalized block commit, how does the finalizer discover and certify candidates, and which node-local mechanisms may bound that search?

## 2. Position on dev

The protocol document states one finality clock. The LFB is the floor of the live view, derived by the same rule every block operation uses. The finalizer filters candidates with more than half the stake, runs the clique oracle, and decides with exact integer arithmetic. A **containment gate** advances the LFB only when the derived floor's state contains the current LFB's settled effects.

The settled-effect probe claim defines `applied(B, sig)` by signature. A deploy is applied when its processed record is not failed, or when its signature is in `applied_from_scope`. The clique rule states a two-sided disagreement test by height: at or above the target height, a message whose spine excludes the target disagrees. Below the target height, only a message off the target's own spine disagrees.

Two operator-visible hold states exist. **ContainmentHold** means the derived floor drops settled effects. A streak of rising refusals triggers the `DivergenceMonitor`, which logs FINALITY DIVERGENCE once and increments a counter. **AbsenceHold** means the floor walk needed a block the node does not hold. The finalizer runs under time budgets with a cooperative yield.

The floor specification finalizes an ancestor when `ft_witnessed(A, just(B)) >= θ`. The protocol document's FT table says finalization at threshold 0.33 with FT 0.33 is No, strict greater than. The two dev sources disagree on strictness.

## 3. Position on PR #216

- **Exact effect identity (DR-43, R-EFFECT-ID).** A committed transition has identity `(source block hash, execution index)`. A successful execution creates one. A failed body with verified settlement creates one (DR-57). Admission rejection creates none. A signature does not identify a state effect.
- **State parent and applied facts (R-EFFECT-WIRE, R-EFFECT-ACTIVE).** Each block commits its exact `merge_base` and a canonical `applied_state_effects` sequence. Active state is the state parent's active set, plus applied facts, plus own effects. Replay recomputes and requires exact equality.
- **Preservation (R-STATE-PRESERVATION).** `preserves(A, D)` requires DAG ancestry and active-set inclusion.
- **Second certificate (R-STATE-CERT).** State support contains exactly the validators whose latest messages causally include the candidate and preserve every effect active at it. The same hard-majority, maximum-clique, exact-threshold decision runs over that support. Causal support through a merge that rejected the candidate's effects does not count.
- **LFB admission (R-LFB-STATE, R-FLOOR-STATE).** A candidate becomes the LFB only when its causal certificate, its state certificate, and current-LFB preservation all hold over one frozen snapshot. Main-parent ancestry is not an admission condition.
- **Universal frontier (DR-45, R-UNIVERSAL-FRONTIER).** The floor rule gains a third source: the highest all-parent ancestor holding both certificates that preserves every inherited floor. It may be secondary to every parent.
- **Discovery (DR-46, DR-54, R-FINALIZER-CLOSURE).** Candidate discovery visits the complete all-parent closure above the LFB in descending order. A candidate-count cap, elapsed-time budget, or per-candidate timeout must not truncate the search. Coverage is computed once by transposing reachability, with a proof that the clique decision is unchanged.
- **Inconclusive, not negative (R-FINALIZER-ERROR).** Missing metadata makes the invocation inconclusive. It is never a negative vote.
- **Rebase and stability (R-REBASE, R-VALIDITY-STABILITY).** A child of a stale-state certified block recomputes from the certified floor. Learning that another block finalized never makes a valid block invalid.
- **Threshold.** Strict, `FT = (2q - S) / S > θ`, in every source.
- **Removed text.** The hold states, the divergence monitor, the work budgets, and the two-sided disagreement sentence do not appear in the PR #216 protocol document.

## 4. Divergence

| Aspect | dev | PR #216 |
|---|---|---|
| Effect identity | Signature | `(source block, execution index)` |
| Failed body with settlement | Not applied | Applied effect |
| Containment | One gate on settled effects | Second clique certificate over state-preserving support |
| Floor sources | Inheritance, main-chain advancement | Plus universal certified advancement |
| Candidate discovery | Main-parent agreement, live-view floor | Complete all-parent closure, descending, no caps |
| Search budgets | Time budgets, cooperative yield | Yield only. Budgets forbidden. |
| Missing block | AbsenceHold | Inconclusive error |
| Divergence detection | DivergenceMonitor with log and counter | Not present |
| Disagreement rule | Two-sided by height, stated | Sentence removed |
| Threshold | `>= θ` in the specification, `> θ` in the protocol table | `> θ` |
| Wire | None | `merge_base` and `applied_state_effects` in the block |

## 5. Options

- **A. Adopt the exact-provenance model as written.** Requires protocol 6 for the wire fields.
- **B. Adopt the principles now and the encoding at the protocol boundary.** State-preserving finality, complete discovery without budgets, inconclusive-not-negative, and validity stability ratify now. The exact identity and wire fields ratify with D-01.
- **C. Keep the signature-projection containment gate.** Reject the second certificate.

## 6. Unification proposal

Adopt option B, with three sub-decisions recorded separately.

**Sub-decision 4.1, state-preserving finality.** Ratify the principle that a causal certificate alone never promotes a state that drops a committed effect. This generalizes the dev containment gate. The state certificate is the exact form of that gate. This follows principle P6.

**Sub-decision 4.2, complete discovery without node-local budgets.** Ratify R-FINALIZER-CLOSURE and R-FINALIZER-ERROR. A time budget makes the candidate set depend on host speed, which principle P2 forbids. DR-46 supplies the work bound that makes this affordable.

**Sub-decision 4.3, threshold strictness.** This is a dev-internal repair. Ratify strict `> θ` and correct the floor specification. PR #216 already uses strict.

**Sub-decision 4.4, operator observability.** Do not ratify the removal of the divergence monitor. Keep a node-local divergence signal as telemetry. The PR #387 protocol text says such measurements never control consensus, which is compatible.

**Sub-decision 4.5, the two-sided disagreement rule.** Hold. The sources do not show whether the rule moved into a model or was dropped.

## 7. Ratification checklist

- 4.1: a differential test showing that every LFB promotion valid on dev is valid under the state certificate, and a case where the state certificate refuses a promotion the signature gate would allow.
- 4.2: the DR-46 timing evidence on the 132-block regression, and a bound statement in the specification.
- 4.3: none beyond the text fix.
- 4.4: a named metric and log target.
- 4.5: the location of the two-sided rule on PR #216, or a statement that it is superseded and why.
- After ratification, replace the protocol section 7 and the floor specification sections 2 and 2.2. Update the settled-effect probe claim to name its protocol scope.

## 8. Open questions

1. Where does the two-sided disagreement rule live on PR #216?
2. Does the second certificate change the FT value reported to clients, or only admission?
3. The dev claim `settled-effect-probe-equivalence` stays valid for legacy blocks. Does a protocol-6 node keep the legacy probe for blocks below its floor?
