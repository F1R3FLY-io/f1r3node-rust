# Deploy-scope cache read semantics

```yaml
claim_id: CLAIM-DEPLOY-SCOPE-CACHE-001
artifacts:
  - casper/src/rust/engine/multi_parent_casper/snapshot.rs
  - shared/src/rust/store/lmdb_key_value_store.rs
  - block-storage/src/rust/key_value_block_store.rs
status: pending
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/1177
  - https://github.com/F1R3FLY-io/f1r3node-rust/pull/1178
```

For the same ancestor sequence, a rebuilt snapshot must return the same deploy signatures, seed holders and retained rejection signatures as the reference body lookup. Duplicate-flagged rejections contribute no retained signature. A present body can have an empty retained list.

After body pruning, previously cached deploy identities remain available for the lifetime of that entry. The pruned body contributes no retained rejection. A cold lookup with no identities and no body still reports the missing ancestor. A successful replacement invalidates the previous facts, including a read in flight before replacement. A missing body or storage error does not consume a completed cache slot.

The LMDB presence probe must distinguish readable values, missing keys and invalid bincode values in the same way as the typed read. A read error must propagate before a valid pending copy is removed. This check covers the stored byte-vector encoding. Protobuf decoding and consensus validation are separate operations.

The binding evidence covers finite fixtures and the tested read, prune, replace and error schedules. Formal coverage and maintainer acceptance remain pending. The claim makes no recovery guarantee for a validator missing historical state.
