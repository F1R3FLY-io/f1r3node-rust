# Deploy storage bound

| Artifact | Purpose |
| --- | --- |
| `DeployStorageBound.tla` | One deploy under phlo accounting: storage effects are charged per encoded byte before they are retained, and execution halts when the limit is exhausted. |
| `MC_DeployStorageBound.cfg` | Requires `RetainedWithinPhlo`: retained bytes times the storage rate never exceed the phlo limit. 237 states. |
| `MC_DeployStorageBound_unmetered_pre_fix.cfg` | Storage effects are not charged. It must violate `RetainedWithinPhlo`. It checks no liveness property, because an unmetered deploy is not claimed to terminate. |

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

## Fairness and termination

`Spec` uses weak fairness on the whole `Next` action, not on `Finish` alone. That is enough for `Terminates` in the metered configuration for one reason. Every `StorageEffect` and every `ComputeStep` strictly decreases `phloRemaining`, so a run can take only finitely many of them. After that, `Finish` and the out-of-phlo transition are the only enabled steps, and either one leaves the running phase. Fairness on `Finish` alone would make termination hold without metering, so it would prove less.

The unmetered control does not have this property. `StorageEffect` stays enabled forever there, so a run can stutter on it under weak fairness. The control therefore checks only the invariant that the gate registers it for.

## Promotion decision

The interpreter is a mandatory subsystem under
[CbC verification tiers](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2388a8eedf33d07018f0630bced51a6e054ba439/docs/cbc-verification-tiers.md), and the
claim here is unbounded: it holds for every deploy and every phlo limit. The
refutation tier above does not close it. Promotion is pending. The Rocq
theorem belongs on the execution side after the consensus component moves to
its own repository. Its name goes in the table above when it lands.
Until then the cycle record for this area carries `construction: pending`.

The tiers link above is a permalink into the staging branch of the soak disk hygiene stack. The document itself lands on `dev` with the last PR of that stack, and the link becomes the relative path `docs/cbc-verification-tiers.md` then.

The model does not bound how many deploys a block admits. It also does not
bound how the tuple space stores a byte on disk, or the history trie's growth
per checkpoint. Those are consumer-side quantities.
