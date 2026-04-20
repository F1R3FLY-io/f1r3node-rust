# PoS Stake Delegation Mechanism

## Scope

This document describes the delegation extension in:

- `casper/src/main/resources/PoS.rhox`
- `casper/src/test/resources/PoSTest.rho`

Validation notes below are from runs on **April 20, 2026 (UTC)**.

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
- `delegatedTotals : Map[ValidatorPk, Int]`
  - Aggregated delegated stake per validator.
- `delegatorRewards : Map[DelegatorPk, Int]`
  - Accrued rewards claimable by delegators.
- `pendingUndelegations : Map[DelegatorPk, Map[ValidatorPk, (Int, Int)]]`
  - Pending undelegation principal by delegator/validator as `(amount, unlockBlock)`.

Effective stake is computed as:

- `effectiveBonds[validator] = allBonds[validator] + delegatedTotals.getOrElse(validator, 0)`

`computeEffectiveBonds` ignores stale `delegatedTotals` entries for validators missing from `allBonds`.

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

### New write methods

- `PoS("delegate", deployerId, validatorPk, amount, returnCh)`
  - Preconditions:
    - `amount > 0`
    - validator exists in `allBonds`
    - validator self-bond is `> 0`
    - validator is not in `pendingWithdrawers`
  - Effects:
    - transfers `amount` from delegator vault to PoS vault
    - increments `delegations[delegatorPk][validatorPk]`
    - increments `delegatedTotals[validatorPk]`

- `PoS("undelegate", deployerId, validatorPk, amount, returnCh)`
  - Preconditions:
    - `amount > 0`
    - delegator has at least `amount` delegated to `validatorPk`
    - no active pending undelegation for `(delegatorPk, validatorPk)`
  - Effects:
    - removes `amount` from active delegation immediately
    - decrements `delegations[delegatorPk][validatorPk]`
    - decrements `delegatedTotals[validatorPk]`
    - creates `pendingUndelegations[delegatorPk][validatorPk] = (amount, blockNumber + epochLength)`

- `PoS("completeUndelegate", deployerId, validatorPk, returnCh)`
  - Preconditions:
    - pending undelegation exists for `(delegatorPk, validatorPk)`
    - `currentBlock >= unlockBlock`
  - Effects:
    - transfers cooled undelegation principal from PoS vault to delegator vault
    - removes pending undelegation entry

- `PoS("claimDelegatorRewards", deployerId, returnCh)`
  - Preconditions:
    - `delegatorRewards[delegatorPk] > 0`
  - Effects:
    - transfers accumulated delegator rewards to delegator vault
    - clears delegator reward entry

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
- `Validator is not bonded.`
- `Validator has no active bond.`
- `Validator is pending withdrawal.`
- `Undelegation amount must be positive.`
- `Undelegation amount exceeds delegated stake.`
- `Pending undelegation already exists for validator.`
- `No pending undelegation for validator.`
- `Undelegation cooldown not finished.`
- `No delegator rewards available.`
- `Validator has active delegations.` (withdraw path)

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
- completion step: `PoS("completeUndelegate", ...)` transfers principal only after cooldown

### Withdraw

`PoS("withdraw", ...)` now rejects when validator has active delegated stake:

- condition: `delegatedTotals.getOrElse(validatorPk, 0) > 0`
- error: `"Validator has active delegations."`

### Slash

`PoS("slash", ...)` now:

- transfers `selfBond + delegatedTotal` to Coop vault
- removes slashed validator from every delegator mapping
- deletes `delegatedTotals[validatorPk]`
- keeps previous slash state transitions (`allBonds[validatorPk] = 0`, remove from active set, clear rewards)

## Mechanism Invariants

Expected invariants for valid state transitions:

1. `delegatedTotals[v] == sum(delegations[d].getOrElse(v, 0) for all d)`
2. Delegation can only target validators with active self-bond (`allBonds[v] > 0`).
3. Validator with positive delegated total cannot enter pending withdrawal.
4. On slash, both validator self-bond and delegated stake attached to that validator are slashed.
5. Reward weighting uses effective stake, but active-set selection remains based on `allBonds` path in `closeBlock`.
6. Undelegation request removes stake from effective bonds immediately and records claimable principal in `pendingUndelegations`.
7. Delegator rewards are persisted on-chain in `delegatorRewards` until claimed.

## Operational Validation (April 20, 2026 UTC)

### Passed

- Rust node compile check:
  - `cargo check -p node`
  - Result: **PASS**.

- Container shard startup and PoS init SLA:
  - `./scripts/ci/check-casper-init-sla.sh docker/shard.yml 240`
  - Result: **PASS**.

### Blocked / Inconclusive

- Targeted PoS Scala test execution:
  - `sbt "casper/testOnly coop.rchain.casper.genesis.contracts.PoSSpec"`
  - Blocked by unrelated compile errors in `rspace/ReplayRSpace.scala` (`MaybeConsumeResult`, `MaybeProduceResult` missing).

- Rust-client smoke and deploy finalization waits:
  - `/home/purplezky/work/asi/rust-client/scripts/smoke_test.sh localhost 40412 40413 40452`
  - `deploy` step passes, but `deploy-and-wait` hangs waiting for finalization in this environment.

- Direct `deploy-and-wait` with validator observer also timed out waiting for finalization:
  - `target/release/node_cli deploy-and-wait ... --observer-port 40413`

Observed last-finalized heights during this run were non-uniform across nodes (e.g. 43/41/38), indicating a current environment finalization issue unrelated to contract syntax/bootstrap.

## Current Limitations

1. Cooldown duration is fixed to `epochLength` (no separate undelegation-parameter yet).
2. Only one pending undelegation per `(delegator, validator)` pair is allowed at a time.
3. Delegator rewards are tracked per delegator total (not bucketed per validator).

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
