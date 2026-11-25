# Proposal eligibility after validator recovery

The [startup horizon probe](../../casper/src/rust/engine/initializing.rs) can resume from a stored DAG or restore state from peers. A validator that resumes from the stored DAG can propose with its existing history. It still requests fork-choice tips. An LFS restore arms the own-history gate before the engine enters Running.

The [proposer](../../casper/src/rust/blocks/proposer/proposer.rs) checks bonding first. A bonded validator returns `HistoryIncomplete` while it awaits its own history. This check runs before block creation. Once the gate opens, the usual proposal constraints apply.

## What the gate observes

At the end of LFS, the [gate](../../casper/src/rust/engine/multi_parent_casper/own_history_gate.rs) records the latest sequence it holds for each peer with its own block. On a proposal attempt, it checks active peers whose latest-message slot contains their own block. Each such block must be valid. If the peer had a recorded sequence, its latest sequence must exceed that value. Its citation for the recovering validator must match the local snapshot. Two absent citations also match.

The gate opens when every checked peer meets these conditions, including when there are no peers to check. It also opens on a proposal attempt after the 120-second deadline. The gate lives in memory. A second restart during recovery can therefore resume from a stored DAG that still lacks part of the validator's signing history. The startup horizon probe checks the height of that DAG.

## Limit of the safety claim

Resuming at the next sequence is safe with respect to reuse if the snapshot contains the validator's actual last signed block as its own latest message and no other process signs with that key. The peer checks establish agreement about the history the node can see. A later signature can still be missing from every peer tip it has received. Elapsed time supplies no evidence about that signature. Protection against reuse in this case remains open.

## Regression coverage

- [Stored DAG resume](../../casper/tests/engine/initializing_spec.rs) exercises the startup probe with a stored peer tip. It checks proposal eligibility before any new tip arrives.
- [Restored validator](../../casper/tests/multi_node/restored_validator_spec.rs) calls the production proposer. It checks that a stale citation holds proposals after the missing own block arrives. Once the peers cite that block, the validator proposes its successor and another node accepts it. A second test checks that an unbonded validator receives `NotBonded` while the gate is closed.

Run the two focused checks from the repository root.

```sh
bash scripts/ci/run-tests.sh cargo test --locked -p casper --test mod stored_dag_resume_allows_proposals_without_a_new_peer_tip
bash scripts/ci/run-tests.sh cargo test --locked -p casper --test mod restored_validator_spec
```
