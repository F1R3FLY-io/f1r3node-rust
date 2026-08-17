# Recovery Leader Model

This model checks recovery-leader agreement across local validator views.

| Model element | Rust element |
| --- | --- |
| `Validators` | Finalized bonded validator set |
| `StableLeader` | Minimum validator key |
| `lfbA`, `lfbB` | Node-local last finalized block views |
| `Inv_CrossViewLeader` | Cross-view leader agreement |

`MC_RecoveryLeader.cfg` must pass.

`MC_RecoveryLeader_view_dependent_pre_fix.cfg` must violate `Inv_CrossViewLeader`.
