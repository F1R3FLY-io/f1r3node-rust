# FX-9 decisions: a `new` injects only the URNs it binds (Decision 69)

Branch `patch/FX-9`, lane B, cut from `patch/FX-7` (d1c1a5df0).

## Normalizer

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 1 | `normalize_p_new` puts into `New.injections` only the environment entries whose key is a URN that the `new` binds. Both paths do this: the static path and the wildcard-rewrite path (keyed on its static URNs). | Copy the whole environment, as before. Clear the injections and resolve URNs from a separate table. | Decision 69. A `# P` that contains `new` now has one encoding in every deploy, so it has one wallet, and funders in different deploys fund the same process. `eval_new` reads only the bound URNs from the map, so behavior does not change. | Yes, before M2. It changes the encoding of every stored `new`. |
| 2 | A `new` that binds an author-bound URN (for example `rho:system:deployerId`) still carries that URN's value. Such a `# P` stays specific to its deploy. | — | The value is part of the process. Two installers make two processes (FX-7 decision 6). | — |

## State roots

The state root changes for every deploy that stores a continuation containing a `new` that was normalized with a deploy environment. No golden root in the repository changes:

- `seal_record_spec::LEGACY_GOLDEN_ROOT`: its outer `new` is evaluated, not stored.
- `runtime_spec` bootstrap root and the genesis tests: genesis terms are normalized without a deploy environment, so their injections were already empty.

The full rholang, casper and models suites pass with no re-pin.

## Docs

- P3-03 decision 3 and its test text, and P3-02 decision 8, now point to FX-7 decisions 10 and 15. They show the joint spend inside `{% … %}[d (*) c]`.

## Tests

- `gated_executor_spec::a_process_with_new_hashes_the_same_in_every_deploy_and_is_funded_across_deploys`:
  - P is `new x in { x!(1) | for (_ <- x) { @"sink"!(1) } }`. Its `process_owner` is the same under Alice's environment, Carol's environment and no environment.
  - Alice funds 3,000 in one block. Carol funds 1,000 in the next block. The record is (3,000, 1,000, 4,000) and the wallet holds 4,000.
  - The second block replays to the same settled root.
