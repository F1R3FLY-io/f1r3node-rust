# FX-5 decisions: unnormalizable terms and variable seals over stacks

Fix FX-5 (A-6 findings F12 and F19), Decisions 62 and 66, rev-stacks patch
plan r8.5, lane C. Branch `patch/FX-5`, cut from `patch/FX-6` (a978756ca).

## Decisions

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 1 | The executor classifies each envelope as `Unnormalized`, `Sealed` or `Named`. An unnormalized term names nothing and seals nothing, so the deployer alone pays (F12). | Refuse it at the gate. | Decision 66. The run fails before any region runs, so only the deployer's charge moves. | Yes |
| 2 | The gate keeps its kind assumption for a seal over a stack body that it cannot resolve statically. Such a seal is not named, so a kind name received at run time stays usable. | Name an unresolved variable seal as unresolved and refuse it (A-6 F19). | Decision 62: kind seals remain usable from received names (F19 option 1, for kinds). | Yes |
| 3 | When the variable resolves to a wallet at run time, the region check of FX-2 and FX-6 decides. The wallet counts only if the owner's envelope admitted the code (the origin's signers), or the code is stored under the owner's record, and never inside data received at run time. | A run-time refusal of every wallet seal over a stack body. | Decision 62. A wallet that the gate could not see behind a variable passes only where its owner wrote the seal. | Yes |
| 4 | For a stack body whose signature is a compound (the nested kind form `{% {% cells %}[k] %}[A]`), the gate splits the elements. Elements that it cannot resolve, or that are kind names, are kinds. The other elements are the payer, and the gate names them as one owner (a compound when there are several). With no payer element, the stack is a kind stack, as decision 2. With no kind element, the whole signature is named, as before. | Name the whole compound. | The gate names the same payer that the reducer draws on (FX-2 decision 11). Before this, the nested form was refused at the gate (FX-2 open item 1). | Yes |

## Tests

- `rholang/tests/signed_region_spec.rs`, `the_gate_sees_the_deployer_as_the_only_name_of_an_own_signed_term`: the nested form names the deployer, the nested form with a received payer names nothing, and a mint names nothing.
- `casper/tests/v6/region_spec.rs`:
  - `a_term_that_fails_to_normalize_is_charged_to_the_deployer_alone` (the A-6 F12 plan): a cosigned envelope whose term does not normalize. Only Alice's wallet is debited, Bob's wallet does not change, no transfer settles, and the block replays to the same settled root.
  - `the_nested_kind_form_passes_the_gate_and_draws_only_on_its_sealer` (the A-6 F15 plan, end to end): Alice mints 3 units of a kind, then moves 1 with the nested form in her own envelope. Bob's envelope uses the nested form with Alice's id received at run time. The gate admits it, and the run fails at the region check. Alice holds 2, the slot holds 1, Bob holds none, and each block replays to the same settled root.
