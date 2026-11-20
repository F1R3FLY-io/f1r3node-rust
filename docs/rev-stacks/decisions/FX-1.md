# FX-1 decisions: the genesis mint and the registry lookup (audit A-6, F1 and F8)

Branch `patch/FX-1`, lane B, cut from `patch/P2-integration` (3b5a7e90a).

## Finding confirmed

On 3b5a7e90a, with the URN filter on as for any deploy after genesis, a user deploy binds `rho:internal:registry_lookup`, asks it for `rho:rev:genesis`, and receives the handle.

| Case | Result before the fix |
|---|---|
| `"mint"` of 1,000,000 to the deployer's address | Succeeds. The balance and the recorded genesis supply both become 1,000,000, so the conservation checks still pass. |
| `"bonds"` with the deployer's key and stake 500 | Succeeds. The bond wallet holds 500 and the supply rises to 1,000,500. |
| The lookup asked for `rho:io:fs:native:1.0.0/removeDir` | The handle is served. |

No error is raised in any case. The tests below reproduced each case before the fix and fail on the old code.

## Decisions

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 1 | The reducer gets a separate flag, `genesis_phase`, default off. It is on only while genesis is played (`play_deploys_for_genesis`), replayed (`is_genesis`), or reported (a block with no parents, passed to reporting as `is_genesis`). | Keep the genesis-only URN on the fs-native flag. | F8: every place that lifted the fs-native filter also exposed the mint, and reporting keyed on cost accounting rather than on genesis. | Yes |
| 2 | One function, `urn_refusal(urn, fs_native_filtered, genesis)`, decides whether a URN may resolve. `eval_new` and `registry_lookup` both call it. The fs-native message is unchanged. | Filter in each place separately. | The lookup is a second way to bind a URN, and the two must not disagree. | Yes |
| 3 | The `rho:rev:genesis` handler refuses every call outside the genesis phase, with "rho:rev:genesis runs only during genesis". | Rely on the URN filters. | The handler must not depend on its own unreachability. A handle bound during genesis and kept in state is refused when used after it. | Yes |
| 4 | `SystemProcesses` and the reducer share the two flags through one `UrnPhase` value built in `setup_reducer`. `RhoRuntime::enter_genesis_phase` returns a guard that restores the previous value on drop. | — | One source for the phase, scoped by the caller. | Yes |
| 5 | The fs-native flag keeps its existing toggles and meaning. Lifting it no longer exposes the genesis URN. | — | F8. | Yes |

## Tests

`rholang/tests/rev_genesis_spec.rs`:
- `a_user_deploy_cannot_mint_through_the_registry_lookup`
- `a_user_deploy_cannot_mint_bonds_through_the_registry_lookup`
- `a_user_deploy_cannot_reach_an_fs_native_urn_through_the_registry_lookup`
- `lifting_the_fs_filter_alone_does_not_expose_the_genesis_mint` (F8)
- `a_genesis_handle_kept_past_genesis_is_refused_by_the_handler` (decision 3)

The genesis tests that used the fs-native flag to stand in for genesis now use `enter_genesis_phase`.

`casper/tests/v6/gated_executor_spec.rs`:
- `a_user_deploy_cannot_reach_the_genesis_mint_through_the_registry_lookup`: in a v6 shard after genesis, two user deploys try the mint and the bonds through the lookup. Both runs fail, the recorded supply and the deployer's balance less charges are unchanged, and the block replays to the same settled root.
