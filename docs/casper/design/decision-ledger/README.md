# Casper Decision Ledger: Cost-Accounting Unification

**Status.** Decided. Every entry received a decision on 2026-09-16. See section 2.2.

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

A ratification meeting record that a ratifier posts as a review comment, and that names the PR #216 author as a participant, records both approvals.

### 2.2 Ratification record of 2026-09-16

A ratification meeting on 2026-09-16 decided every entry. `jeffrey-l-turner` posted the [meeting record](https://github.com/F1R3FLY-io/f1r3node-rust/pull/390#pullrequestreview-5227717933) as a review comment on PR #390. The participants were `jeffrey-l-turner`, `dylon`, and `spreston8`. The record is the linked proof for each entry and for each table row.

The meeting used current `dev` as the default Casper authority. PR #216 supplies a rule only for a demonstrated defect, a required accounting invariant, or an approved FIP.

Three terms come from the record.

- **Protocol 7.** The next Casper protocol version. It activates through a fresh genesis after FIP approval. The entries name protocol 6, which was the PR #216 value at comparison time. Protocol 7 supersedes it.
- **Accounting authority version 8.** The version of the node-level accounting module. It is independent of the Casper protocol version.
- **FIP.** A F1R3FLY Improvement Proposal, filed in the [FIPS repository](https://github.com/F1R3FLY-io/FIPS). FIP approval gates a protocol-7 activation and a token validity layer.

Conformance and soak harness changes go to a separate branch and PR. They stay separate from this documentation PR. The harness must retain deterministic seeds, run identifiers, metrics, and artifacts.

Each entry has a section named Decision (2026-09-16) after its sources. That section records the ratified position, its effect on the options and sub-decisions, and the edits that follow. Sections 1 to 8 of each entry stay as the comparison record.

## 3. Status vocabulary

| Status | Meaning |
|---|---|
| Proposed | Written in this ledger. No maintainer decision yet. |
| Ratified | A maintainer approved the unification proposal. Specification edits may follow. |
| Rejected | A maintainer rejected the proposal. The entry records the reason and any replacement. |
| Deferred | A maintainer postponed the decision to a named event, for example a protocol boundary or a soak result. |

The statuses `accepted and implemented`, `superseded`, and `user-ratified` in the PR #216 decision-records file map to none of these. They describe implementation state on that branch.

## 4. Entries

| ID | Entry | Kind | Proposed conflict with a ratified dev position | Decision (2026-09-16) |
|---|---|---|---|---|
| [D-01](./01-protocol-version-authority.md) | Protocol-version authority and activation | Protocol | No. Dev has no normative rule. | Ratified with modifications. Protocol 7, one authority chain, fresh genesis after FIP approval. |
| [D-02](./02-certified-floor-authority.md) | Certified finalized floor and authority committee | Protocol | Amends ground truth 1 and rule R-COMM. | Ratified with modifications. The `dev` committee stays. No floor certificates. |
| [D-03](./03-fork-choice-certified-context.md) | Fork choice over a certified context | Protocol | Replaces R-FILTER, R-LCA depth filter, and R-COUNT truncation. | Ratified with modifications. The `dev` fork choice stays. The finalized floor bounds the LCA walk. |
| [D-04](./04-state-preserving-finality.md) | State-preserving finality and effect provenance | Protocol | Generalizes the containment gate. Removes hold states and budgets. | Ratified with modifications. The `dev` containment gate stays. The threshold is inclusive. |
| [D-05](./05-finalization-publication.md) | Durable finalization publication and concurrency | Node-local | No. Architecture change. | Ratified with modifications. Five invariants. Single-flight stays the default. |
| [D-06](./06-heartbeat-recovery-leadership.md) | Heartbeat intents and recovery leadership | Node-local policy | Yes. Dev says recovery is never leader-gated. | Ratified with deferrals. Intents, coalescing, and permits ratified. Rotation and frontier-follow removal deferred. |
| [D-07](./07-deploy-recovery-custody.md) | Deploy recovery, custody, and retry packaging | Mixed | Amends the ratified B1 packaging predicate. | Ratified with a deferred experiment. One-parent coverage stays. Collective coverage deferred. |
| [D-08](./08-merge-algebra-and-rejection-records.md) | Merge algebra, rejection records, and mergeable evidence | Protocol | Yes. Dev rule N-SEMANTICS forbids the PR #216 fold. | Ratified with an activation condition. Additive composition at protocol 7. |
| [D-09](./09-slashing-authorization.md) | Slashing authorization, evidence identity, and neglect | Protocol | Replaces the rejected-slash recovery loop. Removes a gated proof. | Ratified with modifications. The `dev` slashing rules stay. The bisimilarity anchor stays. |
| [D-10](./10-repeat-deploy-carrier-index.md) | Repeat-deploy carrier index | Protocol refinement | Amends the pending 2026-09-01 row. | Ratified with conditions. The index stays. Protocol 7 identity. The claim stays pending. |
| [D-11](./11-cbc-fv-governance.md) | CbC and FV governance | Governance | Yes. Rewrites two ratified rows in prose. | Ratified. Tiers of 2,000, 10,000, and 100,000 cases. |
| [D-12](./12-deploy-cost-limits.md) | Deploy cost limits, removal of `phloLimit` and `phloPrice` | Protocol and economics boundary | Removes the minimum-price validation rule. | Rejected. Both fields stay. |

## 5. Merge notes for `feature/cost-accounted-rho`

This branch merges into PR #216's branch as well as into `dev`. Two files conflict textually with that branch.

- `docs/casper/CONSENSUS_PHILOSOPHY.md`. Both branches edit the 2026-09-01 row. Keep the wording on this branch. It records the D-10 decision. Keep every 2026-09-05 row and the 2026-09-03 row from this branch.
- `docs/formal-verification.md`. Both branches add a carrier-index row to the verified-areas table. Keep both rows. Entry D-10 says both models gate.

The meeting of 2026-09-16 preserved the `dev` position on most entries. The PR #216 protocol and theory documents hold positions that the decisions reject or defer. The ledger does not edit them. A follow-on change on `feature/cost-accounted-rho` must edit those documents to the ratified positions before that branch merges into `dev`. The Decision section of each entry lists the edits.

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
