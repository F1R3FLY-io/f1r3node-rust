# FX-4 decisions: audit A-6 findings F7, F16/F13, F18 and F20

Branch `patch/FX-4`, lane B, cut from `patch/P3-05` (685627c28), with `patch/FX-1` merged in (8886c8c9a). F18 and F20 are in P3-04's code. F7 relies on FX-1's filtering of genesis-only URNs in both `eval_new` and the registry lookup.

The fixes follow the proposals in A-6 (`docs/rev-stacks/audits/authority-P10.md` on `patch/A-6`).

## F18: one form for the `# P` check and the `# P` wallet

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 1 | The reducer substitutes the seal and the body before it checks a `# Q` component. `# Q` is admitted only when Q and the body are both closed and equal in sorted form. The wallet is keyed on `process_owner` of that closed process, as the deposit path already was. | Compare unsubstituted forms (P3-04 decision 2). | A-6 F18: the check and the wallet key must use one form. Each instance of a template now has its own wallet, and only that instance can certify itself. | Yes |
| 2 | The body is substituted without a phlo charge. | Charge the substitution. | The check must not change the cost of the work it admits. | Yes |
| 3 | The other seal components are substituted once, with the signature, not again per component. | — | Substituting twice is redundant. | Yes |

## F20: a `# P` body may not read the running deployment

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 4 | A `# P` seal is refused when P's encoding contains any of `rho:system:deployerId`, `rho:rchain:deployerId`, `rho:system:deployId`, `rho:system:cosignerId:` or `rho:deploy:data`. The reducer and the gate's self-sealed check both apply it. | Walk the term for `new` binders that name those URNs. | Binding one of those URNs puts its string in the encoding, so the search cannot miss a binding. Its only false positive is a P that contains one of those strings as a literal, and refusing that is safe. A party that P pays must be a name fixed in P (A-6 F20). | Yes |

## F16 and F13: compute spending and hook replacement

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 5 | `ComputeSpender` and its `InstalledHook` variant are removed. `spend_compute` and `replace_rev_hook` take the owner of the enclosing seal. | Keep a spender variant. | A-6 F16: `InstalledHook`'s authority was "the hook is running", which is execution context. | Yes |
| 6 | `seal_authorizes(store, seal)` admits a seal whose components satisfy the hook authority's k of n, or a seal equal to `# H` for the contract H that the hook channel holds. | The authority only; `# H` only. | A-6's proposal, with both cases. `# H` gives the authority to H's own body and to nothing H calls, by P3-04's rule. | Yes |
| 7 | Neither function is wired to a system process. When they are wired, the seal comes from the region record of lane C's FX-2. | Wire now with the deploy's signers. | That would rebuild F13's execution-context pattern. | — |

## F7: release and slashing behind a name only PoS holds

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 8 | `release` and `slash` move from `rho:rev:bond` to a new system process, `rho:rev:bondAdmin` (fixed channel 72). Neither takes the system auth token any longer. `rho:rev:bond` keeps `bond` and `balance`. | Keep the token check on the public process. | §6.9 and A-6 F7: holding the name is the authority. The token is a fieldless singleton that any system deploy holds. | Yes, before a v6 shard runs |
| 9 | `rho:rev:bondAdmin` is genesis-only: `eval_new` and the registry lookup refuse it outside genesis (FX-1). `is_genesis_only_urn` lists it beside `rho:rev:genesis`, and `urn_refusal`, which both `eval_new` and `registry_lookup` call, refuses every genesis-only URN while the genesis flag is off. `rev_bond_spec` checks both routes: binding it with `new` and asking the lookup for it are each refused. Unlike `rho:rev:genesis`, its handler runs after genesis, for the holder of the name. | Refuse after genesis. | PoS needs it at every close block. | Yes |
| 10 | `PoSV6.rhox` binds `rho:rev:bondAdmin` in its top-level `new`, which runs during genesis, so only PoS's code holds the name. Its `slash` and `closeBlock` contracts still check the system auth token for calls from system deploys. | — | A-6 open question 3: the token stays only for the system deploys' own calls into PoS. | Yes |

## Tests

- `hash_seal_spec`:
  - `each_instance_of_a_hash_sealed_template_has_its_own_wallet` (A-6 F18 plan: a template fired with x = 1 and x = 2, and a deposit naming the x = 1 instance lands only in its wallet);
  - `a_hash_seal_is_checked_on_the_substituted_process` (a seal `# {@"y"!(x)}` on the body `@"y"!(1)` is admitted for x = 1, which the unsubstituted comparison refused, and refused for x = 2);
  - `a_hash_sealed_body_may_not_read_the_running_deployment` (refused at run time and by the gate's self-sealed check).
- `hooks::tests`:
  - `replacing_the_rev_hook_needs_the_threshold_of_genesis_addresses` (now with compound seals);
  - `the_compute_wallet_starts_empty_and_spends_only_under_the_authority` (adds the F16 plan: `# H` is refused before H is the hook and admitted after, and a contract H calls cannot spend).
- `rev_bond_spec::only_a_holder_of_the_bond_admin_name_can_release_or_slash_a_bond`: a user deploy cannot slash through `rho:rev:bond`, cannot bind `rho:rev:bondAdmin`, and cannot obtain it through the registry lookup. A name bound during genesis and kept releases and then slashes.

## Open

- An end-to-end test of PoSV6's release and slash through `rho:rev:bondAdmin`. The v6 executor still refuses blocks with slashes (`require_no_slashes`), and release happens only in close-block withdrawals after quarantine. The test belongs with v6 slashing.
- Wiring `spend_compute` and `replace_rev_hook` to the FX-2 region record (decision 7).
