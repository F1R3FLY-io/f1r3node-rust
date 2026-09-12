# Casper Decision Ledger: Cost-Accounting Unification

**Status.** Open. Every entry starts as Proposed.

**Opened.** 2026-09-05

**Compared.** `dev` at the PR #382 merge (`231067178`), PR #387 at `30c428335`, and PR #216 (`feature/cost-accounted-rho`) at `3980ed402`.

**Related.** [Consensus Philosophy](../../CONSENSUS_PHILOSOPHY.md) section 8, [Consensus Protocol](../../CONSENSUS_PROTOCOL.md), [CbC and FV reconciliation](../cost-accounting-cbc-fv-reconciliation.md).

## 1. Purpose

This ledger exists to rectify and ratify the Casper design decisions that the cost-accounting work changes. Its goal is one Casper specification on `dev` that the cost-accounting changes are congruent with. It records each Casper consensus design decision on which `dev` and PR #216 differ. Each entry states both positions, the divergence, the options, and one unification proposal. Maintainers ratify or reject each decision here before any specification text changes.

The ledger covers Casper consensus decisions only. Cost-accounting economics, token semantics, signature algebra, and settlement stay out of scope. Entry D-12 is the one exception. It reviews the removal of the deploy cost fields because that removal changes a block-validity rule and the client deploy contract. An entry cites a PR #216 decision record (DR) as evidence, not as authority. A DR carries no ratification weight until its ledger entry is ratified.

## 2. Ratification workflow

1. An entry is **Proposed** when this ledger opens. Its row in the Consensus Philosophy decision table starts with `Proposed`, then names the entry kind and its conflict with `dev`, then reads `Pending maintainer ratification.`
2. A ratifier reviews the entry in the review pull request. The ratifier approves, rejects, or asks for a change in a review comment under the entry heading. This branch merges into `dev` and into `feature/cost-accounted-rho`. Either merge review can ratify an entry. An entry stays Proposed through both merges until a ratifier decides.
3. On approval, the entry status becomes **Ratified**. The status line records the date, the ratifier handle, and the URL of the approving review comment. The table row changes to `Ratified <date> by <handle>.` with the same URL.
4. On rejection, the entry status becomes **Rejected** with the reason. The row records the rejection. The rejected option stays in the entry as history.
5. After ratification, a separate change edits the protocol and theory specifications to the ratified position. This ledger never edits them.

An entry can hold several numbered sub-decisions. A sub-decision can be ratified alone. The table row flips only when every sub-decision in the entry has a final status.

### 2.1 Ratification authority and proof

A **ratifier** is a maintainer with merge rights on `dev`. For an entry that changes a PR #216 position, the author of PR #216 must also approve. No other approval counts.

The **proof** of a decision is the approving or rejecting review comment on the pull request. The entry status line and the table row both link to that comment. A decision with no linked comment is not ratified, whatever the text says.

This rule also answers the ownership question in entry D-11. The same ratifiers own sub-decisions 11.4 and 11.8.

## 3. Status vocabulary

| Status | Meaning |
|---|---|
| Proposed | Written in this ledger. No maintainer decision yet. |
| Ratified | A maintainer approved the unification proposal. Specification edits may follow. |
| Rejected | A maintainer rejected the proposal. The entry records the reason and any replacement. |
| Deferred | A maintainer postponed the decision to a named event, for example a protocol boundary or a soak result. |

The statuses `accepted and implemented`, `superseded`, and `user-ratified` in the PR #216 decision-records file map to none of these. They describe implementation state on that branch.

## 4. Entries

| ID | Entry | Kind | Conflict with a ratified dev position |
|---|---|---|---|
| [D-01](./01-protocol-version-authority.md) | Protocol-version authority and activation | Protocol | No. Dev has no normative rule. |
| [D-02](./02-certified-floor-authority.md) | Certified finalized floor and authority committee | Protocol | Amends ground truth 1 and rule R-COMM. |
| [D-03](./03-fork-choice-certified-context.md) | Fork choice over a certified context | Protocol | Replaces R-FILTER, R-LCA depth filter, and R-COUNT truncation. |
| [D-04](./04-state-preserving-finality.md) | State-preserving finality and effect provenance | Protocol | Generalizes the containment gate. Removes hold states and budgets. |
| [D-05](./05-finalization-publication.md) | Durable finalization publication and concurrency | Node-local | No. Architecture change. |
| [D-06](./06-heartbeat-recovery-leadership.md) | Heartbeat intents and recovery leadership | Node-local policy | Yes. Dev says recovery is never leader-gated. |
| [D-07](./07-deploy-recovery-custody.md) | Deploy recovery, custody, and retry packaging | Mixed | Amends the ratified B1 packaging predicate. |
| [D-08](./08-merge-algebra-and-rejection-records.md) | Merge algebra, rejection records, and mergeable evidence | Protocol | Yes. Dev rule N-SEMANTICS forbids the PR #216 fold. |
| [D-09](./09-slashing-authorization.md) | Slashing authorization, evidence identity, and neglect | Protocol | Replaces the rejected-slash recovery loop. Removes a gated proof. |
| [D-10](./10-repeat-deploy-carrier-index.md) | Repeat-deploy carrier index | Protocol refinement | Amends the pending 2026-09-01 row. |
| [D-11](./11-cbc-fv-governance.md) | CbC and FV governance | Governance | Yes. Rewrites two ratified rows in prose. |
| [D-12](./12-deploy-cost-limits.md) | Deploy cost limits, removal of `phloLimit` and `phloPrice` | Protocol and economics boundary | Removes the minimum-price validation rule. |

## 5. Merge notes for `feature/cost-accounted-rho`

This branch merges into PR #216's branch as well as into `dev`. Two files conflict textually with that branch.

- `docs/casper/CONSENSUS_PHILOSOPHY.md`. Both branches edit the 2026-09-01 row. Keep the PR #216 wording for the mechanism and the PR #387 status text. Keep every 2026-09-05 row. Entry D-10 records the intended final wording.
- `docs/formal-verification.md`. Both branches add a carrier-index row to the verified-areas table. Keep both rows. Entry D-10 says both models gate.

The PR #216 protocol and theory documents already hold the positions this ledger records for that branch. The ledger does not edit them. A ratified entry is applied to those documents in a later change on whichever branch carries the ratified position.

## 6. Entry template

Each entry uses the same sections so a reviewer can compare entries directly.

1. Question. One sentence that names the decision.
2. Position on dev. The ratified or documented rule, with its source.
3. Position on PR #387. Present only when PR #387 touches the decision.
4. Position on PR #216. The rule, the decision record, and the formal artifacts.
5. Divergence. A table of the concrete differences.
6. Options. Two to four options with their consequences.
7. Unification proposal. One recommended option and the principle it cites.
8. Ratification checklist. The evidence a maintainer needs and the edits that follow ratification.
9. Open questions. Facts the ledger author could not settle from the sources.

Paths that exist only on PR #216 appear as code spans. Paths that exist on this branch are links.
