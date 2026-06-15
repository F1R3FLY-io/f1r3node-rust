# 06 · Evidence visibility & timing models

## 1 · Family motivation

In a partially-synchronous distributed system, not every validator
sees every block at the same time. **Visibility** — which validators
have observed which equivocations — and **timing** — when reports
are submitted that close accountability — are first-class adversary
levers. This family searches the visibility × timing space for
boundary cases.

## 2 · Models in this family

| Model                                                                                                          | Searches                                                                                |
|----------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------|
| [`evidence_propagation_model.sage`](../../../../../formal/sage/slashing/evidence_propagation_model.sage)       | Visibility/report stages over time; accountability shrinkage as reports propagate       |
| [`evidence_timing_attack_search.sage`](../../../../../formal/sage/slashing/evidence_timing_attack_search.sage) | Local-view divergence, report-time closure shrinkage, delayed-evidence slash timing     |
| [`evidence_visibility_model.sage`](../../../../../formal/sage/slashing/evidence_visibility_model.sage)         | Partial evidence visibility, induced neglect edges, full-visibility accountability gaps |

## 3 · Representative witness

```json
{
  "kind": "accountability_gap_witness",
  "n": 4,
  "equivocators": [0],
  "visibility": {"0": [0], "1": [], "2": [], "3": [0]},
  "reports": [],
  "partial_visibility_closure": [0, 1, 2],
  "full_visibility_closure": [0, 1, 2, 3],
  "gap": [3],
  "gap_stake": 25,
  "covered_by_invariant": "Inv_VisibilityReportedNeglectActive"
}
```

Reading: validator 0 equivocates; only validators 0 and 3 see the
equivocation (validators 1 and 2 did not). Under partial visibility,
the closure includes 0, 1, 2 (1 and 2 fail to acknowledge what they
*could* see if they polled 0); validator 3 is *not* in the closure
because it *did* acknowledge. The gap `{3}` is what full-visibility
analysis would catch but partial visibility does not — an
**accountability gap** of stake 25.

## 4 · Promotion targets

| Witness shape                 | Defense / theorem                                     | Rust regression                                             |
|-------------------------------|-------------------------------------------------------|-------------------------------------------------------------|
| Accountability gap            | Visibility / report invariant in `TwoLevelSlashing.v` | `evidence_visibility_gap.rs`, `evidence_view_divergence.rs` |
| Report-time closure shrinkage | `closure_monotonicity_with_reports` (informal)        | `report_time_closure_shrinkage.rs`                          |
| Local-view divergence         | `view_consistency_under_gossip` (design §4)           | `divergence_class.rs`                                       |
| Delayed-evidence slash timing | (informal; documented in threat model §5.A.5)         | `evidence_visibility_gap.rs`                                |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#6** Evidence withholding creates an accountability gap.
- **#14** Evidence propagation over time is not monotone once
  reports are modeled.
- **#15** (corrected invariant) Active neglect edges must be
  *visible and unreported*, not just *visible*.

## 6 · Methodology note

This family encodes the **synchrony hypothesis** (Dwork *et al.*
[DLS88]) as a search parameter rather than as a fixed assumption.
By varying the visibility maps and report timings, the search
exposes which invariants depend on full synchrony and which survive
partial synchrony. The output informs the threat model's
documentation of synchrony as a load-bearing assumption.
