# PoS Stake Delegation Mechanism

## Scope

This document describes the delegation extension in:

- `casper/src/main/resources/PoS.rhox`
- `casper/src/test/resources/PoSTest.rho`

This revision includes:

- on-chain delegator reward accrual + claiming
- undelegation cooldown (`undelegate` request + `completeUndelegate` transfer)

## Goal

Allow an external account to stake to an already bonded validator without becoming a validator itself.

Delegated stake contributes to validator effective stake for reward weight and slashing exposure.

## State Model

PoS state now includes:

- `allBonds : Map[ValidatorPk, Int]`
  - Validator self-bond only.
- `delegations : Map[DelegatorPk, Map[ValidatorPk, Int]]`
  - Ownership ledger of delegated stake.
- `validatorDelegators : Map[ValidatorPk, Map[DelegatorPk, Nil]]`
  - Reverse index used to bound and localize validator delegation operations.
- `delegatedTotals : Map[ValidatorPk, Int]`
  - Aggregated delegated stake per validator.
- `delegatorRewards : Map[DelegatorPk, Map[ValidatorPk, Int]]`
  - Accrued rewards claimable by delegators, bucketed by validator internally.
  - `getDelegatorRewards` returns the legacy flattened `Map[DelegatorPk, Int]` view.
- `pendingUndelegations : Map[DelegatorPk, Map[ValidatorPk, (Int, Int)]]`
  - Pending undelegation principal by delegator/validator as `(amount, unlockBlock)`.
  - Unlocking principal is excluded from future effective stake, but remains slashable escrow until `completeUndelegate`.

Effective stake is computed as:

- `effectiveBonds[validator] = allBonds[validator] + delegatedTotals.getOrElse(validator, 0)`

Slashing deletes the validator from `allBonds`, so slashed validators do not leave permanent zero-bond entries. `computeEffectiveBonds` also ignores stale `delegatedTotals` entries for validators missing from positive `allBonds`.

## Public Contract API

### Existing method with changed semantics

- `PoS("getBonds", returnCh)`
  - Returns **raw self-bonds** (`allBonds`), matching legacy semantics.

### New read methods

- `PoS("getEffectiveBonds", returnCh)`
- `PoS("getDelegations", returnCh)`
- `PoS("getDelegatedTotals", returnCh)`
- `PoS("getDelegatorRewards", returnCh)`
- `PoS("getPendingUndelegations", returnCh)`
- `PoS("getMinimumBond", returnCh)`

### New write methods

- `PoS("delegate", deployerId, validatorPk, amount, returnCh)`
  - Preconditions:
    - `amount > 0`
    - `amount >= minimumBond`
    - validator exists in `allBonds`
    - validator self-bond is `> 0`
    - validator is not in `pendingWithdrawers`
    - validator effective stake cap is respected:
      - `allBonds[validatorPk] + delegatedTotals.getOrElse(validatorPk, 0) + amount <= maximumBond`
  - Effects:
    - transfers `amount` from delegator vault to PoS vault
    - increments `delegations[delegatorPk][validatorPk]`
    - increments `delegatedTotals[validatorPk]`

- `PoS("undelegate", deployerId, validatorPk, amount, returnCh)`
  - Preconditions:
    - `amount > 0`
    - delegator has at least `amount` delegated to `validatorPk`
    - no active pending undelegation for `(delegatorPk, validatorPk)`
    - validator is not pending withdrawal
  - Effects:
    - removes `amount` from active delegation immediately
    - decrements `delegations[delegatorPk][validatorPk]`
    - decrements `delegatedTotals[validatorPk]`
    - creates `pendingUndelegations[delegatorPk][validatorPk] = (amount, blockNumber + epochLength)`
    - pending principal stops contributing to future effective stake, but remains slashable until completion

- `PoS("completeUndelegate", deployerId, validatorPk, returnCh)`
  - Preconditions:
    - pending undelegation exists for `(delegatorPk, validatorPk)`
    - `currentBlock >= unlockBlock`
    - no pending slash evidence targets `validatorPk`
  - Effects:
    - transfers cooled undelegation principal from PoS vault to delegator vault
    - removes pending undelegation entry

- `PoS("claimDelegatorRewards", deployerId, returnCh)`
  - Preconditions:
    - `delegatorRewards[delegatorPk] > 0`
  - Effects:
    - transfers accumulated delegator rewards to delegator vault
    - clears delegator reward entry

- `PoS("withdraw", deployerId, returnCh)`
  - Preconditions:
    - deployer is a bonded validator
  - Effects:
    - converts every active delegation to this validator into pending undelegation with unlock block `blockNumber + epochLength`
    - merges with an existing pending undelegation for the same `(delegator, validator)` by adding the amount and keeping the later unlock block
    - refuses with `Delegated total mismatch.` if the moved active delegation sum does not match `delegatedTotals[validatorPk]`
    - clears `delegatedTotals[validatorPk]`
    - records the validator in `pendingWithdrawers`
  - Consequence:
    - if a delegator already has an unlocked but unclaimed pending undelegation for this validator, validator withdrawal can extend the merged claim by up to `epochLength`, and the merged amount remains slashable until completion

## Wallet SDK Integration (Reference)

This section shows one practical integration pattern for a wallet SDK that can:

- submit signed deploys
- wait for finalization
- run exploratory deploys for read-only state checks

### 1) Build deploy payloads (delegate/undelegate/claim)

```ts
export function buildDelegateRho(validatorPkHex: string, amount: number): string {
  return `
new retCh, PoSCh, rl(\`rho:registry:lookup\`) in {
  rl!(\`rho:system:pos\`, *PoSCh) |
  for(@(_, PoS) <- PoSCh) {
    new deployerId(\`rho:system:deployerId\`) in {
      @PoS!("delegate", *deployerId, "${validatorPkHex}".hexToBytes(), ${amount}, *retCh)
    }
  }
}
`;
}

export function buildUndelegateRho(validatorPkHex: string, amount: number): string {
  return `
new retCh, PoSCh, rl(\`rho:registry:lookup\`) in {
  rl!(\`rho:system:pos\`, *PoSCh) |
  for(@(_, PoS) <- PoSCh) {
    new deployerId(\`rho:system:deployerId\`) in {
      @PoS!("undelegate", *deployerId, "${validatorPkHex}".hexToBytes(), ${amount}, *retCh)
    }
  }
}
`;
}

export function buildCompleteUndelegateRho(validatorPkHex: string): string {
  return `
new retCh, PoSCh, rl(\`rho:registry:lookup\`) in {
  rl!(\`rho:system:pos\`, *PoSCh) |
  for(@(_, PoS) <- PoSCh) {
    new deployerId(\`rho:system:deployerId\`) in {
      @PoS!("completeUndelegate", *deployerId, "${validatorPkHex}".hexToBytes(), *retCh)
    }
  }
}
`;
}

export function buildClaimDelegatorRewardsRho(): string {
  return `
new retCh, PoSCh, rl(\`rho:registry:lookup\`) in {
  rl!(\`rho:system:pos\`, *PoSCh) |
  for(@(_, PoS) <- PoSCh) {
    new deployerId(\`rho:system:deployerId\`) in {
      @PoS!("claimDelegatorRewards", *deployerId, *retCh)
    }
  }
}
`;
}
```

### 2) SDK-side transaction flow

```ts
type DeployResult = { deployId: string };

interface WalletSdk {
  deploy(rhoCode: string, opts?: { phloLimit?: number; phloPrice?: number }): Promise<DeployResult>;
  waitForFinalization(deployId: string): Promise<void>;
  exploratoryDeploy(rhoCode: string): Promise<unknown[]>;
}

export async function delegateStake(
  sdk: WalletSdk,
  validatorPkHex: string,
  amount: number
): Promise<string> {
  const rho = buildDelegateRho(validatorPkHex, amount);
  const { deployId } = await sdk.deploy(rho, { phloLimit: 500_000, phloPrice: 1 });
  await sdk.waitForFinalization(deployId);
  return deployId;
}

export async function undelegateWithCooldown(
  sdk: WalletSdk,
  validatorPkHex: string,
  amount: number
): Promise<{ requestDeployId: string; completeDeployId: string }> {
  const requestRho = buildUndelegateRho(validatorPkHex, amount);
  const request = await sdk.deploy(requestRho, { phloLimit: 500_000, phloPrice: 1 });
  await sdk.waitForFinalization(request.deployId);

  // SDK should poll pending undelegations + current block height off-chain until unlock.
  const completeRho = buildCompleteUndelegateRho(validatorPkHex);
  const complete = await sdk.deploy(completeRho, { phloLimit: 500_000, phloPrice: 1 });
  await sdk.waitForFinalization(complete.deployId);

  return { requestDeployId: request.deployId, completeDeployId: complete.deployId };
}
```

Finalization reliability note for SDKs:

- Prefer waiting against the same validator node that accepted the deploy.
- If finalization polling stalls, fallback to polling both deploy inclusion and `last-finalized-block` height until included block height is finalized.

### 3) Read/verify state from wallet UI

```ts
export function buildDelegationStateQueryRho(delegatorPkHex: string, validatorPkHex: string): string {
  return `
new ret, PoSCh, rl(\`rho:registry:lookup\`), bondsCh, effBondsCh, delCh, totalsCh, rewardsCh, pendingCh in {
  rl!(\`rho:system:pos\`, *PoSCh) |
  for(@(_, PoS) <- PoSCh) {
    @PoS!("getBonds", *bondsCh) |
    @PoS!("getEffectiveBonds", *effBondsCh) |
    @PoS!("getDelegations", *delCh) |
    @PoS!("getDelegatedTotals", *totalsCh) |
    @PoS!("getDelegatorRewards", *rewardsCh) |
    @PoS!("getPendingUndelegations", *pendingCh) |
    for (@b <- bondsCh & @e <- effBondsCh & @d <- delCh & @t <- totalsCh & @r <- rewardsCh & @p <- pendingCh) {
      ret!((
        b.getOrElse("${validatorPkHex}".hexToBytes(), 0),
        e.getOrElse("${validatorPkHex}".hexToBytes(), 0),
        d.getOrElse("${delegatorPkHex}".hexToBytes(), {}).getOrElse("${validatorPkHex}".hexToBytes(), 0),
        t.getOrElse("${validatorPkHex}".hexToBytes(), 0),
        r.getOrElse("${delegatorPkHex}".hexToBytes(), 0),
        p.getOrElse("${delegatorPkHex}".hexToBytes(), {}).getOrElse("${validatorPkHex}".hexToBytes(), Nil)
      ))
    }
  }
}
`;
}
```

Interpretation in wallet UI:

- first value = validator self-bond (`getBonds`)
- second value = validator effective stake (`getEffectiveBonds`)
- third value = this wallet's delegated amount to validator
- fourth value = total delegated amount on validator
- fifth value = this wallet's claimable delegator rewards
- sixth value = this wallet's pending undelegation tuple `(amount, unlockBlock)` for validator (or `Nil`)

### 4) Error handling mapping (recommended)

Map these contract errors to stable SDK/user-facing codes:

- `Delegation amount must be positive.`
- `Delegation is less than minimum.`
- `Validator is not bonded.`
- `Validator has no active bond.`
- `Validator is pending withdrawal.`
- `Delegated total mismatch.`
- `Undelegation amount must be positive.`
- `Undelegation amount exceeds delegated stake.`
- `Pending undelegation already exists for validator.`
- `No pending undelegation for validator.`
- `Undelegation cooldown not finished.`
- `Validator has pending slash evidence.`
- `No delegator rewards available.`
- `Delegation would exceed validator maximum effective bond.`

## Behavior Changes in Core Flows

### Rewards

`rewardsInfo` and `getCurrentEpochRewards` use effective bonds and split epoch rewards:

- `totalBond` is computed from `effectiveBonds`
- `activeBonds` is computed from active validators over `effectiveBonds`

This means delegation increases validator reward weight, and delegators receive on-chain rewards via `delegatorRewards`.

Reward accounting now treats these as liabilities before minting new epoch rewards:

- validator `committedRewards`
- delegator `delegatorRewards`
- `pendingUndelegations` principal

Rust runtime on-chain bond queries now call `getEffectiveBonds`, so consensus stake reads include delegated stake.

### Undelegation Cooldown

`PoS("undelegate", ...)` no longer transfers principal immediately.

- request step: removes active delegation stake and creates pending undelegation with unlock block
- pending step: principal is unlocking, excluded from future effective stake, and still slashable escrow
- completion step: `PoS("completeUndelegate", ...)` transfers principal only after cooldown
- completion refuses while slash evidence for the validator is pending, so a cooled pending undelegation cannot escape a known slash before the slash deploy runs

### Withdraw

`PoS("withdraw", ...)` no longer rejects only because delegated stake exists:

- active delegations to the withdrawing validator are converted to pending undelegations
- pending undelegations use the same `epochLength` cooldown as delegator-requested undelegation
- validator self-bond still follows the existing pending-withdrawer and quarantine flow

### Slash

`PoS("slash", ...)` now:

- transfers `selfBond + quarantinedWithdrawerBond + activeDelegatedTotal + pendingUndelegationTotalForValidator` to Coop vault
- removes slashed validator from every delegator mapping
- deletes the validator's reverse delegator index
- deletes `delegatedTotals[validatorPk]`
- removes pending undelegation claims tied to the slashed validator
- removes pending/quarantined withdrawal records tied to the slashed validator
- removes delegator reward buckets attributed to the slashed validator while preserving rewards from other validators
- deletes `allBonds[validatorPk]`, removes the validator from the active set, and clears rewards

## Mechanism Invariants

Expected invariants for valid state transitions:

1. `delegatedTotals[v] == sum(delegations[d].getOrElse(v, 0) for all d)`
2. Delegation can only target validators with active self-bond (`allBonds[v] > 0`).
3. Delegation amount is at least `minimumBond`. A validator can admit at most `numberOfActiveValidators * 16` distinct delegators at a time; an existing delegator can add stake without consuming another slot.
4. Validator withdrawal converts positive delegated totals into pending undelegations instead of permanently blocking validator exit.
5. A validator cannot increase self-bond through `bond` after it is already bonded; effective stake cap growth is only admitted through `delegate` and checked against `maximumBond`.
6. On slash, validator self-bond, active delegated stake, and pending undelegation principal tied to that validator are slashed.
7. Reward weighting uses effective stake (`self + active delegated`), excluding pending undelegations. Active-validator selection currently caps the bonded-key list and is not stake-ranked by effective amount.
8. Undelegation request removes stake from effective bonds immediately and records unlocking principal in `pendingUndelegations`.
9. Pending undelegation principal remains slashable until `completeUndelegate` transfers it out.
10. Delegator rewards are persisted on-chain by `(delegator, validator)` until claimed or until that validator is slashed.

## Current Limitations

1. Cooldown duration is fixed to `epochLength` (no separate undelegation-parameter yet).
2. Only one pending undelegation per `(delegator, validator)` pair is allowed at a time.
3. `getDelegatorRewards` intentionally remains a flattened compatibility view; validator attribution is internal slash accounting state.
4. `maximumBond` is enforced as an effective stake cap (`self + active delegated`), not just self-bond.
5. Delegation, undelegation, and validator withdrawal change effective stake immediately. TestNet v1 should either accept this immediate-activation economic model explicitly or gate delegation rollout on an epoch-snapshot design.
6. Epoch reward distribution is bounded by the reverse validator-delegator index and the per-validator delegator cap. A reward-per-share or equivalent lazy-accounting model is still the right path before broad open delegation.
7. Pending undelegation slashability is explicit current behavior. Operators and wallets must show that unlocking stake is still slashable until completion.
8. Concurrent delegation deploys contend on the single PoS state cell. Merge-rejected delegation deploys are expected to recover through canonical deploy recovery and finalize without double application, but a deploy that loses the merge can see additional inclusion latency.
9. REST `/validators` and `/validator/{pubkey}` expose effective stake plus self-bond and delegated components. Wallets that need delegation ownership, claimable rewards, or pending undelegation state must still use the PoS query methods through exploratory deploys until dedicated REST endpoints are added.
10. Slashing confiscates delegated principal, pending undelegation principal, and credited delegator rewards tied to the validator being slashed. Rewards attributed to other validators remain claimable.

## Minimal Usage Example (Rholang)

```rho
new return, rl(`rho:registry:lookup`), posCh in {
  rl!(`rho:system:pos`, *posCh) |
  for (@(_, PoS) <- posCh) {
    // Delegate 40 tokens to validatorPk.
    @PoS!("delegate", delegatorDeployerId, validatorPk, 40, *return)
  }
}
```

```rho
new return, rl(`rho:registry:lookup`), posCh in {
  rl!(`rho:system:pos`, *posCh) |
  for (@(_, PoS) <- posCh) {
    // Request undelegation of 25 tokens (starts cooldown).
    @PoS!("undelegate", delegatorDeployerId, validatorPk, 25, *return)
  }
}
```

```rho
new return, rl(`rho:registry:lookup`), posCh in {
  rl!(`rho:system:pos`, *posCh) |
  for (@(_, PoS) <- posCh) {
    // Complete undelegation after unlock block is reached.
    @PoS!("completeUndelegate", delegatorDeployerId, validatorPk, *return)
  }
}
```

```rho
new return, rl(`rho:registry:lookup`), posCh in {
  rl!(`rho:system:pos`, *posCh) |
  for (@(_, PoS) <- posCh) {
    // Claim accumulated delegator rewards.
    @PoS!("claimDelegatorRewards", delegatorDeployerId, *return)
  }
}
```
