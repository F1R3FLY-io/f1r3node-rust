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
| Binding | The existing registry and vault specs check S3 | Recorded |
| Binding | S4 has no dedicated test yet | Pending |
| Refutation | Not modeled | Pending classification |
| Construction | Not modeled | Pending classification |

S1 to S3 are properties of a finite map. A named maintainer must decide whether `construction: not-applicable` applies, as `docs/cbc-verification-tiers.md` requires.
