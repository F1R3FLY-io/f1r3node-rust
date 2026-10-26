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

The review fixes preserve the current reward-forfeiture policy. Slashing transfers forfeited principal and accrued rewards to the coop vault.
A permanent slashed-key set prevents a removed validator from bonding again.
Delegated principal uses the same quarantine horizon as validator withdrawals: the next epoch boundary plus `quarantineLength`.
Epoch processing updates cumulative reward indexes and liability totals per validator. Each stake change or claim settles the affected delegator positions.
Each validator admits up to 1600 active delegators. The bound is independent of the validator count.

PoS state now includes:

- `allBonds : Map[ValidatorPk, BigInt]`
  - Validator self-bond only.
- `delegations : Map[DelegatorPk, Map[ValidatorPk, BigInt]]`
  - Ownership ledger of delegated stake.
- `validatorDelegators : Map[ValidatorPk, Map[DelegatorPk, Nil]]`
  - Reverse index used to bound and localize validator delegation operations.
- `delegatedTotals : Map[ValidatorPk, BigInt]`
  - Aggregated delegated stake per validator.
- `delegatorRewards : Map[DelegatorPk, Map[ValidatorPk, BigInt]]`
  - Settled whole rewards, stored by delegator and validator.
  - `getDelegatorRewards` returns the flattened `Map[DelegatorPk, BigInt]` view.
- `pendingUndelegations : Map[DelegatorPk, Map[ValidatorPk, (BigInt, Int)]]`
  - Pending undelegation principal by delegator/validator as `(amount, unlockBlock)`.
  - Unlocking principal is excluded from future effective stake, but remains slashable escrow until `completeUndelegate`.

Effective stake is computed as:

- `effectiveBonds[validator] = allBonds[validator] + delegatedTotals.getOrElse(validator, 0n)`

Slashing deletes the validator from `allBonds`, so slashed validators do not leave permanent zero-bond entries. `computeEffectiveBonds` also ignores stale `delegatedTotals` entries for validators missing from positive `allBonds`.

## Lazy Reward Accounting

**Implemented in:** `PoS.rhox`

**Value Stream:** Consensus Correctness

As a delegator, I need accurate rewards and a predictable quarantine period without a reward calculation for every delegator at each epoch.

The fresh-chain implementation uses these additional state fields:

| Field | Purpose |
| --- | --- |
| `slashedValidators` | Permanently rejects previously slashed keys at bonding. |
| `rewardIndexes[v]` | Cumulative scaled reward per unit of delegated stake. |
| `rewardCheckpoints[d][v]` | Last settled index and fractional reward remainder. |
| `validatorRewardLiabilities[v]` | Reserved delegator rewards that have not been paid or forfeited. |
| `delegatorRewardLiability` | Sum of all validator reward liabilities. |
| `pendingUndelegationTotal` | Sum of all pending undelegation principal. |

Let `S = maximumBond * 10^18`, `B` be self-bond, `D` delegated stake, and `R` the validator epoch reward.
The validator receives `floor(R * B / (B + D))` in committed rewards.
Delegators reserve `Q = floor(R * D / (B + D))`.
For positive `D`, the cumulative index increases by `floor(Q * S / D)`.
The per-validator and global reward liabilities increase by `Q`.

Settlement computes `scaled = stake * (currentIndex - lastIndex) + remainder`.
It adds `floor(scaled / S)` to the position reward and retains `scaled % S` as the remainder.
A new position starts at the current index and receives no earlier rewards.
Stake additions, undelegations, and validator withdrawals settle rewards before changing the position stake.
Bonding rejects validators whose earlier bond remains in withdrawal quarantine.
Claims settle all positions of the caller and debit liabilities only after a successful transfer.
A failed transfer preserves the original state.

This changes rounding from separate integer truncation each epoch to fractional carry across settlements.
Index truncation leaves a small reserve that cannot be redistributed as surplus.
The reserve remains in the PoS vault until slashing transfers the validator liability to the coop vault.
Claims cannot exceed the total reserved rewards. Repeated claims without new rewards cannot pay twice.
Slashing also transfers committed validator rewards and invalidates all reward checkpoints for the slashed key.

Epoch reward calculation traverses validators and uses cached reward and pending-principal liabilities.
It does not traverse delegator positions. The full rewards query and validator withdrawal still traverse affected delegators.
State serialization costs can still depend on total state size. This change does not claim constant epoch execution cost.

The legacy gRPC `BondInfo.stake` field cannot represent a value above signed 64-bit range.
Such clients must read `stakeBigInt`. REST already exposes the exact decimal string as `stake`.

The regression tests cover stake changes, fractional carry, rounding reserves, repeated claims, quarantine, and claims across slashed and live validators.
The capacity test admits seventeen delegators with one active validator. It does not measure performance at the 1600-delegator limit.


## Public Contract API

Stake amounts, vault balances, and accrued rewards use `BigInt` in native base units.
The `bond`, `delegate`, and `undelegate` methods require BigInt amounts, such as `40n`, and reject `Int` amounts.
The validator and delegation REST endpoints return monetary values as decimal strings.
Block heights, including `unlockBlock`, remain JSON numbers.
Clients must preserve monetary precision and must not convert these strings to JavaScript `number` values.

This proposal changes execution, consensus encoding, and metadata storage without changing the protocol version.
Existing or mixed-version networks require a coordinated activation and state migration before deployment.
See the [economic pipeline design](../designs/pr54-bigint-economic-pipeline.md) for compatibility limits.

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
      - `allBonds[validatorPk] + delegatedTotals.getOrElse(validatorPk, 0n) + amount <= maximumBond`
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
    - creates `pendingUndelegations[delegatorPk][validatorPk] = (amount, quarantineLength + epochLength * (1 + blockNumber / epochLength))`
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
    - the caller has a positive whole reward after settlement
  - Effects:
    - transfers accumulated delegator rewards to delegator vault
    - clears delegator reward entry

- `PoS("withdraw", deployerId, returnCh)`
  - Preconditions:
    - deployer is a bonded validator
  - Effects:
    - converts every active delegation to this validator into pending undelegation with unlock block `quarantineLength + epochLength * (1 + blockNumber / epochLength)`
    - merges with an existing pending undelegation for the same `(delegator, validator)` by adding the amount and keeping the later unlock block
    - refuses with `Delegated total mismatch.` if the moved active delegation sum does not match `delegatedTotals[validatorPk]`
    - clears `delegatedTotals[validatorPk]`
    - records the validator in `pendingWithdrawers`
  - Consequence:
    - if a delegator already has an unlocked but unclaimed pending undelegation for this validator, validator withdrawal can extend the merged claim to the new quarantine horizon, and the merged amount remains slashable until completion

## Wallet SDK Integration (Reference)

This section shows one practical integration pattern for a wallet SDK that can:

- submit signed deploys
- wait for finalization
- run exploratory deploys for read-only state checks

### 1) Build deploy payloads (delegate/undelegate/claim)

```ts
export function buildDelegateRho(validatorPkHex: string, amount: bigint): string {
  return `
new retCh, PoSCh, rl(\`rho:registry:lookup\`) in {
  rl!(\`rho:system:pos\`, *PoSCh) |
  for(@(_, PoS) <- PoSCh) {
    new deployerId(\`rho:system:deployerId\`) in {
      @PoS!("delegate", *deployerId, "${validatorPkHex}".hexToBytes(), ${amount}n, *retCh)
    }
  }
}
`;
}

export function buildUndelegateRho(validatorPkHex: string, amount: bigint): string {
  return `
new retCh, PoSCh, rl(\`rho:registry:lookup\`) in {
  rl!(\`rho:system:pos\`, *PoSCh) |
  for(@(_, PoS) <- PoSCh) {
    new deployerId(\`rho:system:deployerId\`) in {
      @PoS!("undelegate", *deployerId, "${validatorPkHex}".hexToBytes(), ${amount}n, *retCh)
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
  amount: bigint
): Promise<string> {
  const rho = buildDelegateRho(validatorPkHex, amount);
  const { deployId } = await sdk.deploy(rho, { phloLimit: 500_000, phloPrice: 1 });
  await sdk.waitForFinalization(deployId);
  return deployId;
}

export async function undelegateWithCooldown(
  sdk: WalletSdk,
  validatorPkHex: string,
  amount: bigint
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
        b.getOrElse("${validatorPkHex}".hexToBytes(), 0n),
        e.getOrElse("${validatorPkHex}".hexToBytes(), 0n),
        d.getOrElse("${delegatorPkHex}".hexToBytes(), {}).getOrElse("${validatorPkHex}".hexToBytes(), 0n),
        t.getOrElse("${validatorPkHex}".hexToBytes(), 0n),
        r.getOrElse("${delegatorPkHex}".hexToBytes(), 0n),
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
- `Delegation amount must be a BigInt.`
- `Delegation is less than minimum.`
- `Validator is not bonded.`
- `Validator has no active bond.`
- `Validator is pending withdrawal.`
- `Delegated total mismatch.`
- `Undelegation amount must be positive.`
- `Undelegation amount must be a BigInt.`
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
- reserved delegator reward liabilities, including fractional rounding reserves
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
- pending undelegations use the same quarantine horizon as delegator-requested undelegation
- validator self-bond still follows the existing pending-withdrawer and quarantine flow

### Slash

`PoS("slash", ...)` now:

- transfers `selfBond + quarantinedWithdrawerBond + activeDelegatedTotal + pendingUndelegationTotalForValidator + committedRewards + validatorRewardLiability` to Coop vault
- removes slashed validator from every delegator mapping
- deletes the validator's reverse delegator index
- deletes `delegatedTotals[validatorPk]`
- removes pending undelegation claims tied to the slashed validator
- removes pending/quarantined withdrawal records tied to the slashed validator
- invalidates reward checkpoints for the slashed validator and preserves rewards from other validators
- transfers reserved rewards, including rounding reserves, to the coop vault
- permanently prevents the slashed public key from bonding again
- deletes `allBonds[validatorPk]`, removes the validator from the active set, and clears rewards

## Mechanism Invariants

Expected invariants for valid state transitions:

1. `delegatedTotals[v] == sum(delegations[d].getOrElse(v, 0n) for all d)`
2. Delegation can only target validators with active self-bond (`allBonds[v] > 0`).
3. Delegation amount is at least `minimumBond`. A validator can admit at most 1600 distinct delegators at a time; an existing delegator can add stake without consuming another slot.
4. Validator withdrawal converts positive delegated totals into pending undelegations instead of permanently blocking validator exit.
5. A validator cannot increase self-bond through `bond` after it is already bonded; effective stake cap growth is only admitted through `delegate` and checked against `maximumBond`.
6. On slash, validator self-bond, active delegated stake, and pending undelegation principal tied to that validator are slashed.
7. Reward weighting uses effective stake (`self + active delegated`), excluding pending undelegations. Active-validator selection currently caps the bonded-key list and is not stake-ranked by effective amount.
8. Undelegation request removes stake from effective bonds immediately and records unlocking principal in `pendingUndelegations`.
9. Pending undelegation principal remains slashable until `completeUndelegate` transfers it out.
10. Reward indexes and position checkpoints preserve rewards until payment or slashing.

## Current Limitations

1. Delegated principal uses the validator quarantine horizon. There is no separate undelegation duration setting.
2. Only one pending undelegation per `(delegator, validator)` pair is allowed at a time.
3. `getDelegatorRewards` intentionally remains a flattened compatibility view; validator attribution is internal slash accounting state.
4. `maximumBond` is enforced as an effective stake cap (`self + active delegated`), not just self-bond.
5. Delegation, undelegation, and validator withdrawal change effective stake immediately. TestNet v1 should either accept this immediate-activation economic model explicitly or gate delegation rollout on an epoch-snapshot design.
6. Epoch reward accounting uses validator indexes. Full reward queries, validator withdrawals, and state serialization still depend on the number of delegators.
7. Pending undelegation slashability is explicit current behavior. Operators and wallets must show that unlocking stake is still slashable until completion.
8. Concurrent delegation deploys contend on the single PoS state cell. Merge-rejected delegation deploys are expected to recover through canonical deploy recovery and finalize without double application, but a deploy that loses the merge can see additional inclusion latency.
9. REST `/validators` and `/validator/{pubkey}` expose effective stake plus self-bond and delegated components. Wallets that need delegation ownership, claimable rewards, or pending undelegation state must still use the PoS query methods through exploratory deploys until dedicated REST endpoints are added.
10. Slashing confiscates delegated principal, pending undelegation principal, and credited delegator rewards tied to the validator being slashed. Rewards attributed to other validators remain claimable.

## Minimal Usage Example (Rholang)

```rho
new return, rl(`rho:registry:lookup`), posCh in {
  rl!(`rho:system:pos`, *posCh) |
  for (@(_, PoS) <- posCh) {
    @PoS!("delegate", delegatorDeployerId, validatorPk, 40n, *return)
  }
}
```

```rho
new return, rl(`rho:registry:lookup`), posCh in {
  rl!(`rho:system:pos`, *posCh) |
  for (@(_, PoS) <- posCh) {
    @PoS!("undelegate", delegatorDeployerId, validatorPk, 25n, *return)
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
