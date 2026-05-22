# Staking UX Architecture (Bonding + Delegation)

> Last updated: 2026-05-22 (UTC)

## Audience and Goal

This document is for web-wallet product and UI designers building staking features for end users.

Goal: define the staking user experience and required UI properties based on the current PoS contract behavior in this repository.

## Why Staking Exists

Staking aligns user capital with network security:

- **Bonding** lets a user become a validator by locking self-stake.
- **Delegation** lets a user support an existing validator without running validator infrastructure.
- **Slashing** penalizes misbehavior by confiscating stake that was contributing to validator security.

For UI design, the key principle is that stake can be in different lifecycle states with different risk and liquidity.

## Roles

- **Validator**: self-bonds stake, validates blocks, can be slashed.
- **Delegator**: delegates to validator(s), can earn delegator rewards, is exposed to validator slashing.
- **Protocol/System**: executes epoch transitions, active-set updates, slashing, and vault transfers.

## Architecture View

### Layer 1: On-chain PoS Contract

Source: `casper/src/main/resources/PoS.rhox`

Primary user-facing methods:

- `bond(deployerId, amount)`
- `withdraw(deployerId)` (validator exit request)
- `delegate(deployerId, validatorPk, amount)`
- `undelegate(deployerId, validatorPk, amount)` (start cooldown)
- `completeUndelegate(deployerId, validatorPk)` (withdraw after cooldown)
- `claimDelegatorRewards(deployerId)`

Read methods used by UI:

- `getBonds` (validator self-bond only)
- `getEffectiveBonds` (self + active delegated)
- `getDelegations`
- `getDelegatedTotals`
- `getDelegatorRewards`
- `getPendingUndelegations`
- `getActiveValidators`
- `getPendingWithdrawer`
- `getMaximumBond`, `getEpochLength`, `getNumberOfActiveValidators`

### Layer 2: Wallet SDK / Integration Layer

The wallet integration layer should provide:

- signed deploy submission
- deploy finalization tracking
- exploratory reads for PoS state
- block height polling (for unlock timing)

### Layer 3: Wallet UI Layer

The UI should present staking as explicit lifecycle states, not only balances:

- available balance
- bonded / delegated (active exposure)
- unlocking (pending undelegation)
- claimable rewards
- slashing risk state

## State Model for UI

Use this mental model in design and state management:

- `selfBond[validator]` from `getBonds`
- `delegation[delegator][validator]` from `getDelegations`
- `delegatedTotal[validator]` from `getDelegatedTotals`
- `pendingUndelegation[delegator][validator] = (amount, unlockBlock)` from `getPendingUndelegations`
- `delegatorRewards[delegator]` from `getDelegatorRewards`
- `effectiveStake[validator] = selfBond + delegatedTotal` from `getEffectiveBonds`

## Core Semantics (Must Be Reflected in UI)

### 1) Maximum bond caps effective stake

Delegation is rejected when:

`selfBond + delegatedTotal + newDelegation > maximumBond`

Implication for UI:

- show validator remaining delegation capacity
- disable/guard over-cap delegation amounts before signing

### 2) Active stake vs unlocking stake

`undelegate` immediately removes amount from active delegation/effective stake and moves it to pending undelegation.

Implication for UI:

- pending undelegation no longer boosts validator effective stake
- pending undelegation is a separate position state

### 3) Pending undelegation is still slashable

Pending undelegation remains slashable escrow until `completeUndelegate`.

If validator is slashed before completion:

- active delegated exposure is confiscated
- pending undelegation tied to that validator is also confiscated
- pending claim entry is removed
- confiscated funds go to the same slash destination (Coop vault)

Implication for UI:

- unlocking does not mean risk-free
- display explicit warning: "Unlocking until block X, still slashable"

### 4) Validator withdrawal constraint

Validator `withdraw` is rejected while validator has active delegations.

Implication for UI:

- validator exit action must be disabled/blocked when delegated total > 0
- display reason and required action path

### 5) Active-set semantics

Active-set refresh on epoch close uses effective bonds as input and excludes pending undelegations (because they are removed from delegated totals).

Implication for UI:

- show that validator influence tracks effective stake, not pending undelegations
- refresh active status on epoch boundaries

## End-User Journeys and UX Requirements

### A) Become a Validator (Bonding)

Why user does this:

- operate validator and earn validator-side rewards

Flow:

1. User enters bond amount.
2. UI validates min/max bond constraints.
3. User signs `bond`.
4. UI waits for finalization and updates self-bond/effective stake views.

Required UX properties:

- preflight validation of amount
- clear pending/finalized transaction state
- post-finalization validator status refresh

### B) Delegate Stake

Why user does this:

- support a validator and earn delegator rewards without running validator infra

Flow:

1. User selects validator.
2. UI checks validator is bonded and not pending withdrawal.
3. UI checks remaining effective capacity to `maximumBond`.
4. User signs `delegate`.
5. UI confirms finalization and updates delegation/effective stake.

Required UX properties:

- validator detail card must show self, delegated, effective, remaining capacity
- instant rejection mapping for cap/validator-state errors

### C) Undelegate (Two-step Unlock)

Why user does this:

- reduce exposure and start exiting delegated position

Flow:

1. User signs `undelegate(amount)`.
2. UI moves amount to "unlocking" with `unlockBlock`.
3. When block height reaches unlock block, UI enables `completeUndelegate`.
4. User signs `completeUndelegate` to receive principal.

Required UX properties:

- show countdown/progress to unlock block
- keep slash-risk badge visible during cooldown
- block duplicate undelegation requests for same delegator-validator pair while one is pending

### D) Claim Delegator Rewards

Why user does this:

- transfer accrued delegator rewards to wallet balance

Flow:

1. UI reads `delegatorRewards[walletPk]`.
2. If > 0, enable claim action.
3. User signs `claimDelegatorRewards`.
4. UI refreshes rewards and wallet balance.

Required UX properties:

- claim button state bound to positive claimable amount
- transaction result must clear displayed claimable amount

### E) Slashing Event Impact

Why user needs this:

- risk visibility and post-event reconciliation

Flow expectations:

1. Validator slash event occurs.
2. UI refreshes affected validator/delegator state.
3. Active and pending exposures tied to slashed validator are removed.
4. Pending undelegation completion should not be offered if claim was slashed away.

Required UX properties:

- clear incident messaging ("position slashed")
- deterministic reconciliation using fresh on-chain reads

## Required Product Properties for Web Wallet

1. **Safety transparency**
   Pending undelegation must always display "unlocking but slashable."
2. **State correctness**
   UI amounts must reconcile with PoS reads after every finalized transaction.
3. **Finalization-aware UX**
   No optimistic "success" before deploy finalization.
4. **Epoch/block awareness**
   Unlock and active-set changes depend on block/epoch progression.
5. **Deterministic error handling**
   Contract errors mapped to stable user messages.
6. **Multi-position isolation**
   One validator event must not mutate unrelated validator positions in the UI state.
7. **Security**
   Never expose private keys; only signed deploy flow through wallet.

## Suggested UI Information Architecture

### Validator List / Details

Each validator row should expose:

- validator ID
- active status
- self bond
- total delegated
- effective stake
- remaining delegable capacity to max bond
- pending withdrawal flag

### User Position Panel

Per validator delegated by current user:

- active delegated amount
- pending undelegation amount (if any)
- unlock block for pending undelegation
- status badge: `Active`, `Unlocking (Slashable)`, `Slashed`, `Claimable`

Global:

- total claimable delegator rewards

### Transaction Center

Show each staking operation with:

- action type (`Bond`, `Delegate`, `Undelegate`, `Complete Undelegate`, `Claim`)
- submitted tx/deploy id
- status (`Signing`, `Submitted`, `Finalized`, `Failed`)
- resolved error message when failed

## Error-to-UI Mapping (Recommended)

Use stable message mapping for:

- `Bond is less than minimum!`
- `Bond is greater than maximum!`
- `Public key is already bonded.`
- `Validator is not bonded.`
- `Validator has no active bond.`
- `Validator is pending withdrawal.`
- `Delegation would exceed validator maximum effective bond.`
- `Undelegation amount must be positive.`
- `Undelegation amount exceeds delegated stake.`
- `Pending undelegation already exists for validator.`
- `Undelegation cooldown not finished.`
- `No pending undelegation for validator.`
- `No delegator rewards available.`
- `Validator has active delegations.`

## Design Checklist

- Bonding, delegation, undelegation, reward-claim flows all represented
- Unlocking position explicitly marked slashable
- Effective stake and max-capacity displayed per validator
- Pending undelegation excluded from active exposure visual totals
- Slashing refresh path removes both active and pending affected stake
- Active-set and unlock timers refreshed at epoch/block boundaries

## References

- `casper/src/main/resources/PoS.rhox`
- `casper/src/test/resources/PoSTest.rho`
- `docs/casper/POS_STAKE_DELEGATION.md`
