# FX-7 decisions: audit A-7 revisions 3 and 4 (F22 to F26, F30, F31) and the seal-record wiring

Branch `patch/FX-7`, lane B, cut from `patch/FX-4` (3c68f8b2d), with `patch/FX-2` (4aed4d852) and `patch/P3-05` (Decision 58) merged in.

The fixes follow the findings in `docs/rev-stacks/audits/authority-P10.md` on `patch/A-7` (cc89a5eb8).

## Merges and F30

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 1 | `check_region_signature` keeps FX-4's order. It substitutes the seal and the body once, checks each `# Q` component with `hash_seal_admissible`, and then applies FX-2's `region_signers` check with `held_or_signed` to the other components. It returns the owner and the `SealKind`. | Take FX-2's version of the function. | F30: FX-2's version dropped the `# Q` check that F18 needs. | Yes |
| 2 | The P3-05 spec conflict is resolved with the FX-4 side, and the Decision 58 test is appended to it. | — | The two sides added different tests. | Yes |

## F22 and F24: refusal by resolution, not by string

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 3 | `urn_refusal` receives the channel that the URN resolves to. `eval_new` and `registry_lookup` both pass it. | Keep the string lists. | F22 and F24: a URN built at run time has no fixed string. | Yes |
| 4 | Genesis-only names are refused outside genesis when the URN resolves to `rev_genesis` or `rev_bond_admin`. The prefix check on `is_genesis_only_urn` stays as a second test. `setup_maps_and_refs` asserts that each URN that resolves to a genesis-only channel is also matched by the prefix check. | Channel only. | The assertion keeps the two tests in step when a new URN is added. | Yes |
| 5 | Runner-context names (`get_block_data` and `deploy_data` channels) are refused when the current seal record has kind `Process`. | Refuse on the `# P` term's encoding (FX-4 decision 4). | F22: the record knows the region. The string search refused author-bound URNs such as `rho:system:deployerId` that a `# P` region may hold. | Yes |
| 6 | `hash_seal_admissible` no longer searches for runner-context strings. A `# P` that binds `deployerId` is admitted, and the bound name is the installer's, not the runner's (test below). | — | Author-bound URNs resolve at install time, so they name the installer. | Yes |

## F23: hook replacement

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 7 | `quorum_authorizes(store, seal)` is the quorum part of `seal_authorizes`. `replace_rev_hook` uses it, so `# H` cannot replace the hook. `spend_compute` keeps `seal_authorizes`. | — | F23. | Yes |

## F25: bondAdmin binders

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 8 | A genesis test builds every blessed term and checks that only the `PoSV6` term names `rho:rev:bondAdmin`, that three terms name `rho:rev:genesis`, and that no term asks the registry lookup. | Check the rholang sources by file. | The built terms are what genesis runs. Together with F24 (decision 4), no later system deploy can bind the name. | Yes |

## F31 (Decision 64)

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 9 | No code change. The existing draw path records a stack in a self-sealed `# P` region as a pro-rata draw on the funders' shares. A casper test now proves it in play and replay. | — | Decision 64 is the interim rule until region metering. | — |

## Wiring to the seal record (F5, F6, F13, F16, F26)

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 10 | `SystemProcesses::wallet_authority` gives the owner of the current seal record. A compound owner gives its components and the compound itself. With records off, it gives the deploy's signers, as before. `rho:rev:wallet` and `rho:rev:bond` use it. | Keep the deploy's signers. | F5, F6: the deploy's signers are execution context. Stored code that a deployment fires runs under its author's record, so it cannot draw on the firing deployment's wallet. | Yes |
| 11 | `bond` requires that the bonder is in the authority. | — | F5. | Yes |
| 12 | New `rho:rev:wallet` commands `spendCompute (payee, amount)` and `replaceHook hook`. Each takes the current seal. `spendCompute` uses `seal_authorizes`. `replaceHook` uses `quorum_authorizes`, takes `Nil` for no hook, and is refused while the journal is open. | A separate system process. | F13, F16. The journal carries only moves, so a hook change inside a gated run could not be settled or replayed. | Yes |
| 13 | `HookAuthority::authorizes` skips compound owners in the signer list. | Do not add the compound to the authority. | The compound is needed for joint wallets (decision 10). It is derived from its components, which are checked. | Yes |
| 14 | The gate adds the deployer to the paying wallets when the envelope has a stack outside every signed term (`has_bare_stack`). | — | F26 wiring: such a stack runs under the implicit deployment seal and draws on the deployer. | Yes |
| 15 | Behavior change: an unsealed, cosigned deploy no longer has its cosigners' authority. A joint-wallet spend or a funder refusal must be inside `{% … %}[d (*) c]`. The casper tests now do this, and one checks that the unsealed cosigned refusal moves nothing. | — | P10. | Yes |

## Tests

- `rholang/tests/seal_wiring_spec.rs` (through the interpreter, F26):
  - `a_transfer_draws_only_on_a_wallet_whose_owner_holds_the_enclosing_seal` (F6);
  - `a_bond_draws_only_on_the_holder_of_the_enclosing_seal` (F5);
  - `the_compute_wallet_spends_under_the_quorum_or_the_installed_hook_and_nothing_it_calls` (F16);
  - `a_hook_is_replaced_only_inside_a_region_the_quorum_sealed` (F13, F23).
- `hash_seal_spec::a_hash_sealed_region_may_not_read_the_running_deployment` (F22).
- `v6_genesis_wallets_spec::only_pos_v6_binds_the_bond_admin_name_and_only_the_rev_genesis_deploys_bind_the_mint` (F25).
- `gated_executor_spec::a_stack_in_a_self_sealed_region_draws_on_the_funders_shares_pro_rata` (F31).
- Test fixes: `rev_bond_spec` now sets the deploy data, because `bond` reads the authority (decision 11). In `hash_seal_spec`, the unsealed lookup gets its own return name. With one random seed for every run, its name was equal to the refused run's name, and the reply could go to the earlier listener.

## Open items (memo)

1. **Normalizer injections.** The normalizer copies the whole deploy environment (`deployerId`, `deployId` and others) into each `New.injections`. A `# P` that contains `new` therefore has a different hash in each deploy, so a funder cannot fund the same P from another deploy. A fix changes term encodings and so the state root. Recommendation: inject only the URNs that the `new` binds.
2. **Hook replacement in gated runs.** Decision 12 refuses it. Recommendation: keep the refusal until the journal can carry a hook change.
3. **PoSV6 release and slash.** No end-to-end v6 test exercises them, because `require_no_slashes` is set in the test shards.
