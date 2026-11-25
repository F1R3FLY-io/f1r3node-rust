# Proposal eligibility after validator recovery

A validator that restores its last finalized state from peers (LFS) waits for an operator to confirm its signing history. It can receive and validate blocks while it waits. A bonded validator reports `HistoryIncomplete` before creating a block. Peer agreement and elapsed time cannot release it. An unbonded key still reports `NotBonded`.

## Confirm a restore

1. Stop the previous signer and ensure only this process can sign with the key. Identify the genesis hash and the validator's public key from your network and key records.
2. Attempt a proposal or wait for the heartbeat. The node logs `Proposals held pending operator confirmation of signing history` with the recovery ID, genesis hash, validator public key and a file path under its data directory. Verify the genesis and key against your records.
3. Write one of the following lines to a temporary file in that directory. Use lowercase hex without a prefix. Atomically rename the file to `validator-recovery-<recovery-id>.confirm` in the same directory. Restrict write access to trusted operators.
   - For a key that has signed blocks on this genesis, use `<genesis-hash> <public-key> <last-signed-sequence> <last-signed-block-hash>`. Obtain the exact last signature from a complete record of the old signer. Include blocks it signed but never published. A peer tip, finalized height or an old backup can omit such blocks.
   - Only for a key known to have never signed a proposal on this genesis, use `<genesis-hash> <public-key> first-use`. This includes new validators whose first startup uses LFS after the genesis ceremony.
4. On a subsequent proposal attempt the node checks the file against the same snapshot used to build the next block. For a signed checkpoint it requires the exact valid own latest message, its sequence, its justification and the stored block body. For first use it requires an empty signing history and an empty own slot or genesis placeholder. On success it persists the approval for this key and logs `Accepted operator confirmation of signing history`. The operator can then remove the file. Normal proposal constraints still apply.

If the last signature is unknown, leave proposals blocked. If the correct block is missing locally, leave the correct checkpoint in place while the node catches up. If the node cannot recover that block, the key remains blocked.

## Restarts and trust assumptions

The [startup engine](../../casper/src/rust/engine/initializing.rs) persists a new random recovery ID before installing the first block of each peer restore, including an observer restore. The [proposer gate](../../casper/src/rust/engine/multi_parent_casper/own_history_gate.rs) looks up the marker and the actual signer's approval in the local approved-block store. A restart from the stored DAG preserves a pending hold or an accepted approval. A later LFS requires a new confirmation. An ordinary restart from intact data with no recovery marker continues to propose.

The operator attests the completeness of the checkpoint and exclusive use of the key. This mechanism applies to peer restores performed by this version. It cannot detect a volume rolled back to an older intact state or retrospectively identify an LFS performed by an earlier version. Keep a validator with an uncertain signing history offline until it can undergo a new restore and its operator can supply a truthful confirmation. An independent signing journal and automatic recovery are separate work.

## Regression coverage

The [engine tests](../../casper/tests/engine/initializing_spec.rs) check an ordinary stored-DAG restart and a restart with a pending restore. The [production proposer tests](../../casper/tests/multi_node/restored_validator_spec.rs) cover a missing checkpoint, first-use refusal after signing, peer convergence without approval, acceptance of the exact checkpoint, stale first-use approval and unbonded refusal.

```sh
bash scripts/ci/run-tests.sh cargo test --locked -p casper --test mod stored_dag_resume_
bash scripts/ci/run-tests.sh cargo test --locked -p casper --test mod restored_validator_spec
```
