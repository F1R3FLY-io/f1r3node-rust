# Vaults and Tokens

The vault system manages REV (the native token) balances. Built on top of the registry using MakeMint for currency operations and SystemVault for authenticated transfers.

## Architecture

```
SystemVault (rho:vault:system)
  |-- findOrCreate(address) -> vault instance
  |-- deployerAuthKey(deployerId) -> auth key
  |
  +-- vault instance
        |-- transfer(to, amount, authKey, ack)
        |-- balance(ret)
```

## Vault Addresses

Vault addresses are derived from public keys using `rho:vault:address`.

### Get Address from Public Key

```rho
new vaultAddr(`rho:vault:address`) in {
  new ret in {
    vaultAddr!("fromPublicKey", publicKeyBytes, *ret) |
    for (@address <- ret) {
      stdout!(address)
    }
  }
}
```

### Get Address from Deployer ID

```rho
new vaultAddr(`rho:vault:address`),
    deployerId(`rho:system:deployerId`) in {
  new ret in {
    vaultAddr!("fromDeployerId", *deployerId, *ret) |
    for (@address <- ret) {
      stdout!(address)
    }
  }
}
```

## SystemVault

The SystemVault contract is registered in the registry at `rho:vault:system`.

### Looking Up the SystemVault

```rho
new lookup(`rho:registry:lookup`), sysCh in {
  lookup!(`rho:vault:system`, *sysCh) |
  for (@(_, SystemVault) <- sysCh) {
    // SystemVault is now available
  }
}
```

### Finding or Creating a Vault

```rho
@SystemVault!("findOrCreate", address, *vaultCh) |
for (@(success, vault) <- vaultCh) {
  if (success) {
    // vault is the vault instance channel
  }
}
```

Returns `(true, vault)` on success or `(false, errorMessage)` on failure.

### Checking Balance

```rho
new lookup(`rho:registry:lookup`), sysCh, stdout(`rho:io:stdout`) in {
  lookup!(`rho:vault:system`, *sysCh) |
  for (@(_, SystemVault) <- sysCh) {
    new vaultCh in {
      @SystemVault!("findOrCreate", myAddress, *vaultCh) |
      for (@(true, vault) <- vaultCh) {
        new balCh in {
          @vault!("balance", *balCh) |
          for (@balance <- balCh) {
            stdout!(["Balance:", balance])
          }
        }
      }
    }
  }
}
```

### Transferring Funds

Transfers require an authentication key derived from the deployer's identity.

```rho
new lookup(`rho:registry:lookup`), sysCh,
    deployerId(`rho:system:deployerId`),
    stdout(`rho:io:stdout`) in {

  lookup!(`rho:vault:system`, *sysCh) |
  for (@(_, SystemVault) <- sysCh) {

    new vaultCh, targetCh, keyCh in {
      @SystemVault!("findOrCreate", fromAddress, *vaultCh) |
      @SystemVault!("findOrCreate", toAddress, *targetCh) |
      @SystemVault!("deployerAuthKey", *deployerId, *keyCh) |

      for (@(true, vault) <- vaultCh;
           key <- keyCh;
           @(true, _) <- targetCh) {

        new resultCh in {
          @vault!("transfer", toAddress, 100, *key, *resultCh) |
          for (@result <- resultCh) {
            stdout!(["Transfer result:", result])
          }
        }
      }
    }
  }
}
```

## Vault Registration

`SystemVault.findOrCreate(address)` does two things:

1. Computes the per-address vault instance channel.
2. Registers the per-vault contracts (including the internal `_deposit` receiver) in the tuplespace.

`_transferTemplate` calls `findOrCreate` on the destination before it sends `_deposit`, so every transfer registers its destination and moves the source balance inside the sender's own deploy. The source funds are not held in an intermediate purse and are not debited by a later deploy that another key signed.

`findOrCreate` is atomic per vault address: a `vaultMapLock` token held across the `TreeHashMap.get` and (when creation is needed) the `_create` + `set` makes concurrent callers for the same address agree on one vault instance and share its `_deposit` listener. Two parallel transfers to the same fresh address in one deploy therefore both land in the same purse.

Calling `findOrCreate` on the destination is not required for a transfer to complete. It remains a reasonable idiom when the deploy also wants to observe the destination vault directly — for example, to read the target balance in the same deploy.

## Mergeable-Tagged Channels

Some channels in F1R3FLY are tagged so that concurrent writes from sibling blocks merge instead of conflicting. Vault balances are one example (`IntegerAdd` — the deltas sum across chains); the registry's `TreeHashMap` interior-node bitmaps are another (`BitmaskOr` — the bitmaps are OR-merged). Channels of shape `@(*tag, ...)` where `tag` is bound to a known URI (`rho:system:bitmaskMergeableTag` for `BitmaskOr`) are detected by the merger at evaluation time.

A contract author can opt into mergeable semantics by binding the same URI:

```rho
new bitmaskTag(`rho:system:bitmaskMergeableTag`) in {
  // sends/consumes on @(*bitmaskTag, ..., ...) get BitmaskOr semantics
}
```

Mergeable channels rely on a contract-maintained invariant: at any observation point the channel holds zero or one Datum. Registry.rho upholds this with the lock pattern

```rho
for (@val <- @(*bitmaskTag, node, *storeToken)) {
  // compute newVal from val
  @(*bitmaskTag, node, *storeToken)!(newVal)
}
```

The linear consume `<-` removes the existing Datum; the contract publishes a fresh one. Always exactly one Datum.

Two situations silently break this invariant and corrupt the contract's own state. Neither breaks consensus or determinism, but a contract that hits either of them will diverge from its intended behavior on the very first lock cycle.

### Don't use `!!` (replicated send) on a mergeable-tagged channel

```rho
// WRONG — !! makes the Datum persistent, so the lock-acquire `<-` doesn't remove it
@(*bitmaskTag, node, *storeToken)!!(initVal) |
for (@val <- @(*bitmaskTag, node, *storeToken)) {
  @(*bitmaskTag, node, *storeToken)!(newVal)  // adds a SECOND Datum
}
```

The persistent Datum survives the consume, so the contract's release `!` adds a second Datum alongside the persistent one. Each lock cycle adds another. The merger's numeric multi-value reader OR-folds (or for `IntegerAdd` picks max of) every Datum on the channel on every read — so reads return a value that's the OR (or max) of every value the channel ever held, not the most recent write. Use `!` (linear send), not `!!`.

### Don't read with `<<-` and write with `!` without an intervening linear consume

```rho
// WRONG — <<- peeks without consuming; two concurrent peeks see the same val
for (@val <<- @(*bitmaskTag, node, *storeToken)) {
  @(*bitmaskTag, node, *storeToken)!(newVal)  // both branches publish
}
```

`<<-` reads without consuming, so two parallel readers can both observe the pre-state and both publish. The channel ends up with multiple Datums. Use `<<-` for read-only paths and the linear-consume `<-` form for read-modify-write.

The runtime can't tell an intended-singleton mergeable channel from an intended-multiset one — the persistence flag and `<<-` vs `<-` are valid Rholang in general. If you build on top of a mergeable-tagged channel, model the lifetime explicitly and prefer the Registry.rho lock pattern for any update step.

## MakeMint

MakeMint is a token factory that creates purses with controlled minting.

### Creating a Mint

```rho
// MakeMint is available via registry
new lookup(`rho:registry:lookup`), mintCh in {
  lookup!(`rho:id:makemint_uri`, *mintCh) |
  for (makeMint <- mintCh) {
    new purseCh in {
      makeMint!(*purseCh) |
      for (mint <- purseCh) {
        // mint is a new currency mint
      }
    }
  }
}
```

### Purse Operations

A purse holds a balance in a specific currency.

```rho
// Create a purse with initial balance
@mint!("makePurse", 1000, *purseCh) |
for (purse <- purseCh) {

  // Check balance
  new bal in {
    @purse!("getBalance", *bal) |
    for (@balance <- bal) {
      stdout!(balance)   // 1000
    }
  } |

  // Split: create new purse with some funds
  new splitCh in {
    @purse!("split", 300, *splitCh) |
    for (@result <- splitCh) {
      // result contains the new purse with 300
      // original purse now has 700
    }
  } |

  // Deposit: move funds from one purse to another
  new ack in {
    @targetPurse!("deposit", 200, *sourcePurse, *ack) |
    for (@result <- ack) {
      // funds moved
    }
  }
}
```

Key properties:
- Purses can only hold one currency (cross-currency deposits fail)
- Balances cannot go negative (NonNegativeNumber enforcement)
- Overflow protection on deposits

## AuthKey

Authentication keys provide deployer-identity-based authorization.

```rho
// Get an auth key from the deployer's identity
new deployerId(`rho:system:deployerId`) in {
  @SystemVault!("deployerAuthKey", *deployerId, *keyCh) |
  for (key <- keyCh) {
    // key authorizes operations for this deployer
  }
}
```

Auth keys are used for vault transfers and other privileged operations. They cannot be forged because they depend on unforgeable deployer identity channels.

## NonNegativeNumber

A wrapper ensuring a number never goes below zero. Used internally by MakeMint.

```rho
// Create
@NonNegativeNumber!(100, *nnCh) |
for (nn <- nnCh) {

  // Add
  new ret in {
    @nn!("add", 50, *ret) |
    for (@success <- ret) { ... }   // true, value is now 150
  } |

  // Subtract
  new ret in {
    @nn!("sub", 200, *ret) |
    for (@success <- ret) { ... }   // false, would go negative
  }
}
```

## MultiSigSystemVault

A variant of SystemVault requiring multiple signature approvals for transfers. Used for governance-controlled funds.

```rho
// Requires N-of-M signatures to authorize
@MultiSigVault!("transfer", toAddress, amount, signatures, *resultCh) |
for (@result <- resultCh) {
  // result indicates success/failure
}
```

See `casper/src/main/resources/MultiSigSystemVault.rho` and `casper/src/test/resources/MultiSigSystemVaultTest.rho` for the full implementation and tests.
