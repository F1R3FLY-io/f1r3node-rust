# Claim: legacy vault and PoS URIs resolve to the canonical contracts

```yaml
claim_id: CLAIM-REGISTRY-URI-ALIASES-001
artifacts:
  - casper/src/main/resources/Registry.rho                    # registry shorthand map, deployed at genesis
  - models/src/rust/normalizer_env.rs                         # rho:rchain:deployId and rho:rchain:deployerId aliases
  - casper/src/test/resources/VaultUriAliasesTest.rho         # binding test
  - casper/tests/genesis/contracts/vault_uri_aliases_spec.rs  # binding test runner
status: pending
adapter: agentic
mechanization: none
refutation: not-applicable
construction: not-applicable
binding: recorded
applicability_review:
  classification: bounded-by-design
  accepted_by: jeffrey-l-turner
  accepted_at: 2026-10-16
  scope: S1, S2, S3, and S4. See "Tier classification" below
references:
  - F1R3FLY-io/f1r3node 4812fce0                    # upstream added rho:rchain:pos and rho:rchain:revVault for client compatibility
  - F1R3FLY-io/f1r3node 7fff0901 (PR #468)          # upstream lost both aliases in a Registry.rho sync with Scala
  - 1214adada                                       # this repository took the loss in the rust/staging sync at 6ee5c390
  - F1r3Faucet docs/designs/faucet-architecture.md  # design question Q16, Section 3.1
  - docs/ToDos.md TASK-014-10                       # test net faucet
```

## Context

The registry shorthand map in `Registry.rho` turns a readable URI into a `rho:id`. The wallets of record use older URIs. f1r3wallet uses `rho:rchain:revVault` and `rho:rchain:pos`. A lookup of a URI that the map does not hold never returns. These wallets therefore cannot read a balance or transfer on a current shard.

The deploy identity URIs `rho:rchain:deployId` and `rho:rchain:deployerId` already resolve through `normalizer_env.rs`. This claim records them so that a later change cannot drop them without review.

## Claim statements

**S1. Same contract.** Each alias maps to the `rho:id` of its canonical URI:

| Alias | Canonical URI |
|---|---|
| `rho:rchain:revVault` (deprecated) | `rho:vault:system` |
| `rho:f1r3fly:revVault` | `rho:vault:system` |
| `rho:rchain:pos` (deprecated) | `rho:system:pos` |
| `rho:f1r3fly:pos` | `rho:system:pos` |
| `rho:f1r3fly:multiSigRevVault` | `rho:vault:multiSig` |

**S2. Same behavior.** `findOrCreate` through `rho:rchain:revVault` or `rho:f1r3fly:revVault` returns the vault that `rho:vault:system` returns for the same address. A transfer through either alias moves the same balance as a transfer through the canonical URI.

**S3. Map otherwise unchanged.** The aliases add entries only. Every other shorthand keeps its `rho:id`.

**S4. Deploy identity aliases.** `rho:rchain:deployId` and `rho:rchain:deployerId` bind the same values as `rho:system:deployId` and `rho:system:deployerId`.

## Scope

`Registry.rho` is part of the genesis state. A change to it alters the genesis state of a new shard only. A running shard keeps the registry of its genesis.

## Evidence

| Tier | Evidence | State |
|---|---|---|
| Binding | `vault_uri_aliases_spec` checks S1 and S2. It fails on the registry without the aliases | Recorded |
| Binding | `vault_uri_aliases_spec` checks S3 for all 15 entries of the shorthand map: a lookup of each shorthand returns the value of its `rho:id`. A wrong `rho:id` for one entry fails the spec | Recorded |
| Binding | `normalizer_env.rs` tests `with_deployer_id_should_include_legacy_alias` and `normalizer_env_from_deploy_should_include_legacy_aliases` check S4 | Recorded |
| Refutation | None. See the tier classification | Not applicable |
| Construction | None. See the tier classification | Not applicable |

## Tier classification

The classification follows "Applicability per property" in `docs/cbc-verification-tiers.md`.

- **Domain.** S1 and S3 cover the shorthand map in `Registry.rho`, which has 15 entries. S4 covers the two deploy identity aliases in `normalizer_env.rs`. The source fixes both maps. A deploy cannot change the registry map of a running shard, because the map is part of genesis state.
- **Universal requirement.** "Each alias" and "every other shorthand" mean the 15 listed entries and the two deploy identity aliases. No other input, state, configuration, or execution history is in scope.
- **Specification bounds.** The bounds are the entries in the source. A new entry changes `Registry.rho` or `normalizer_env.rs`, and both files are `cbc=mandatory`, so the change returns this claim to review.
- **S2.** S2 follows from S1. The same `rho:id` gives the same contract object. The behavior of `findOrCreate` and `transfer` is a property of the vault contract, outside this claim. The binding test also checks S2 directly.
- **Refutation.** These properties have no state machine and no interleaving. A model checker would explore one fixed map. The binding tests cover the complete permitted domain.
- **Construction.** The properties are bounded by design. A kernel theorem would restate the 15-entry table and add no reduction in risk.
- **Trusted assumptions.** The RhoSpec framework and the registry `lookup` contract behave as their own specs check.
- **Review.** jeffrey-l-turner accepted the bounded-by-design classification and `construction: not-applicable` on 2026-10-16. This review does not accept the claim. The claim stays pending until the pull request review.
