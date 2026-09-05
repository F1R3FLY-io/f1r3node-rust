# D-11 CbC and FV Governance

**Status:** Proposed. Pending maintainer ratification.
**Kind:** Governance.
**Sources:** [CbC and FV reconciliation](../cost-accounting-cbc-fv-reconciliation.md), [formal-verification.md](../../../formal-verification.md), [Consensus Philosophy](../../CONSENSUS_PHILOSOPHY.md) sections 4.2, 4.3, 7.1, and 8, [`scripts/ci/check-tla-invariants.sh`](../../../../scripts/ci/check-tla-invariants.sh). PR #216 `cost-accounting-decision-records.md` preamble, `formal/README.md` completion criterion, `scripts/ci/check-tla-invariants.sh`, `scripts/ci/check-formal-invariants.sh`.

## 1. Question

Which verification practice, gate list, and decision record govern Casper after the cost-accounting work lands?

The reconciliation document holds the evidence. This entry states the decisions.

## 2. Sub-decisions

### 11.1 Practice rules 3 to 5

Dev states five practice rules. PR #216 removes rules 3, 4, and 5: a refuted claim blocks completion, distributed decisions need cross-view claims, and liveness includes resource bounds.

**Proposal.** Ratify that the five rules stay. The three removed rules are the rules that entries D-03, D-04, and D-06 rely on.

### 11.2 Expected-violation configurations

Dev keeps violation configurations outside the gate list and states it. PR #216 removes the sentence but keeps the practice.

**Proposal.** Ratify the practice and restore the sentence.

### 11.3 Gate entries

PR #216 removes six dev gate entries. Three have named replacements. `MC_ForkChoice`, `MC_PromotionConvergence`, and `MC_ReplayHotLoop` have none.

**Proposal.** Ratify that a gate entry leaves the list only with a named replacement or a ratified supersession. Restore the three entries without replacements. The replay-liveness claim on dev remains pending and its model must gate.

### 11.4 Property-test tiers

PR #216 lowers the pull-request tier from 10,000 cases to 2,000 and the nightly tier from 100,000 to 10,000.

**Proposal.** Deferred. A maintainer decides after the reason is stated. Wall-clock cost is a valid reason if it is recorded.

### 11.5 Decision-record governance

The PR #216 decision-records file names the cost-accounting paper as the law of the implementation. The Consensus Philosophy derives principles from failures and requires each remedy to preserve safety invariants.

**Proposal.** Ratify the mapping in this ledger's README. The philosophy table is the single Casper decision record. A DR is cited evidence. The DR statuses map to no table status. The paper-as-law preamble applies to cost-accounting economics, not to Casper consensus.

### 11.6 Formal-area completion criterion

PR #216 adds a seven-item criterion for a complete formal area.

**Proposal.** Ratify as an addition to the practice rules. It is compatible with rules 1 and 2.

### 11.7 Mandatory scope for new consensus files

Thirteen new files in the casper and block-storage crates carry no CbC attribute and no claim.

**Proposal.** Ratify that each file receives an attribute and either a claim or a recorded waiver before merge. Entry D-04, D-05, D-08, and D-09 name the claims.

### 11.8 Correctness anchor for slashing

DR-8 removes the Rust to Scala bisimilarity theorems from the gated assumption check.

**Proposal.** Deferred to a maintainer decision. The question is whether the Scala implementation remains the specification anchor for slashing. If it does not, `main_slashing_algorithm_correct` must be stated in the slashing specification as the headline claim, which PR #216 does in its section 9.

### 11.9 Verification scripts outside workflows

More than forty scripts on PR #216 run verification that no workflow invokes.

**Proposal.** Ratify that a script which discharges a ratified claim must run in a workflow, scheduled or on pull request. Scripts that explore stay local.

### 11.10 User Contract Concurrency

The 2026-08-22 row ratifies the waiver. PR #216 section 4.2 describes an enforced gate in dedicated jobs.

**Proposal.** Ratify the enforced gate as the follow-up the waiver row anticipated. Update the row on ratification.

### 11.11 Scan benchmark

The 2026-08-22 row ratifies the benchmark as a merge gate. PR #216 section 4.3 says no executable benchmark exists and calls it an open release criterion.

**Proposal.** Ratify the PR #216 statement as a correction of fact and keep the gate requirement. The row changes from "measurement remains a merge gate" to "benchmark pending. Gate requirement unchanged."

## 3. Ratification checklist

- 11.1 and 11.2: text restored on the integration branch.
- 11.3: gate list on the integration branch contains the three restored entries.
- 11.7: attributes and claims or waivers in the integration change.
- 11.10 and 11.11: row edits after the maintainer approves.

## 4. Open questions

1. Who owns the decision on 11.4 and 11.8? Both need a named maintainer.
2. Does the completion criterion apply retroactively to dev formal areas that lack an unsafe control?
