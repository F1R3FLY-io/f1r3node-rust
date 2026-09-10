# Deploy storage bound

| Artifact | Purpose |
| --- | --- |
| `DeployStorageBound.tla` | One deploy under phlo accounting: storage effects are charged per encoded byte before they are retained, and execution halts when the limit is exhausted. |
| `MC_DeployStorageBound.cfg` | Requires `RetainedWithinPhlo`: retained bytes times the storage rate never exceed the phlo limit. 237 states. |
| `MC_DeployStorageBound_unmetered_pre_fix.cfg` | Storage effects are not charged. It must violate `RetainedWithinPhlo`. |

This area belongs to the consensus and execution component. It cites only the
interpreter's cost table and exports one number to consumers: a deploy retains
at most `PhloLimit \div StorageCostPerByte` bytes. Nothing here refers to the
soak driver or to any host model. A host model that needs the bound names it
as a constant and cites this README.

| Model expression | Rust realization |
| --- | --- |
| `StorageEffect` with `b` bytes | `storage_cost_produce` and `storage_cost_consume` in `rholang/src/rust/interpreter/accounting/costs.rs`: the charge is the encoded length of the channel, data, patterns, and body |
| `StorageCostPerByte = 1` | `storage_cost` charges `encoded_len()` phlo per byte |
| `charge <= phloRemaining`, else `out-of-phlo` | the cost manager's charge before the effect is applied |
| `ComputeStep` | every non-storage cost in the same table |
| `PhloLimit` | the deploy's `phlo_limit` |

The model does not bound how many deploys a block admits. It also does not
bound how the tuple space stores a byte on disk, or the history trie's growth
per checkpoint. Those are consumer-side quantities.
