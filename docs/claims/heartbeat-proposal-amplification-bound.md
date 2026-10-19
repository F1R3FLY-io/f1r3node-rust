# Heartbeat Proposal Amplification Bound Claim

```yaml
status: pending
revised_on: 2026-10-18
claimed_by: pi-session-heartbeat-h1-cbc
task: TASK-021-26
artifacts:
  - node/src/rust/instances/heartbeat_proposer.rs
  - casper/src/rust/blocks/proposer/proposer.rs
  - casper/src/rust/mod.rs
  - node/src/rust/runtime/setup.rs
  - node/src/rust/instances/proposer_instance.rs
  - node/src/rust/runtime/node_runtime.rs
  - casper/src/rust/api/block_api.rs
  - casper/src/rust/engine/casper_launch.rs
  - node/src/rust/instances/heartbeat_responsive_tests.rs
  - casper/tests/add_block/h1_proposer_spec.rs
```

## Status

This claim is mandatory for heartbeat recovery changes. The H1 revision was registered as pending before implementation. Discharge and maintainer acceptance remain pending.

The [design](../designs/deploy-heartbeat-wake.md) supplies the candidate H1 contract. This claim defines verification obligations, not evidence that the candidate already satisfies them.

The [historical evidence](../casper/cbc-evidence/history/node-src-rust-instances-heartbeat-proposer-rs-20260821.md) remains unchanged. It describes a leader-only formulation that later stall evidence refuted. It does not verify H1 or the current all-eligible recovery policy.

The current [heartbeat evidence record](../casper/cbc-evidence/node-src-rust-instances-heartbeat-proposer-rs.md) is pending. The proposer request change also needs source-bound verification before completion.

## Definitions and scope

- `C` is the configured self-proposal cooldown. `I` is the configured heartbeat check interval.
- The hysteresis window is `W = 2 * I`. The multiplier and the exemption limit are constants, not new configuration keys.
- A rolling cooldown window at time `t` is the half-open interval `(t - C, t]`.
- Routine lanes are pending deploys, the pending-deploy backstop, and frontier follow.
- The successful-block bound counts actual successful publications from those routine lanes, not requests or queue entries.
- An H1 request consumes exemption credit when the request is issued, regardless of its eventual result.
- A user-deploy-bearing block has at least one processed deploy that does not satisfy `is_system_deploy_id`.
- A failed user deploy still makes its block user-deploy-bearing. System-only and rejection-only blocks are empty for this claim.

The rolling bounds assume `C > 0` and `I > 0` within one live heartbeat task. They do not establish durable credit across process restarts.

Zero-valued configurations need explicit handling without panic or an accidental busy loop. Publication timing assumes the production callback awaits publication effects before returning a successful result.

The successful routine-block bound excludes stale recovery, convergence, manual proposals, direct autopropose, and the synchronous proposer recovery path.

Those exclusions do not grant new proposal authority. Each excluded path retains its own eligibility and pacing requirements.

## Claim

### H1-1: mode transitions

A check that observes pending deploys enters or refreshes deploy-responsive mode. It records the observation time.

The mode exits when no check has observed pending deploys for `W`. A check that sees pending work at the expiry boundary refreshes the mode.

Generic signal wakes do not establish separate deploy arrivals. The transition contract uses observed pending work, not an unavailable arrival timestamp.

Mode exit and re-entry never reset exemption credit. Deploy-finalization grace remains a separate mechanism that widens lag caps only.

### H1-2: narrow cooldown exception

Outside deploy-responsive mode, a recent self-proposal stops all routine heartbeat proposals until the cooldown ends.

Inside the mode, only the pending-deploy lane can use an H1 exemption. The validator's most recent successful own block must be empty.

The exemption changes only the cooldown predicate of that lane. Bond status, lag eligibility, backpressure where applicable, and other proposal constraints remain enforced.

The cooldown after a user-deploy-bearing own block remains enforced. The pending-deploy backstop and frontier-follow lanes receive no H1 exemption.

### H1-3: bounded credit and successful publications

At most one exempt request can be issued in any rolling cooldown window. Issuance reserves and consumes credit atomically before another request can observe it.

Success, failure, busy, coalescing, cancellation after issuance, and `NoNewDeploys` all consume that request's credit. They do not replenish credit early.

No mode transition replenishes credit early. A rolling configured cooldown must elapse after the last exempt request before another exempt request is eligible.

In each rolling cooldown window, a validator publishes at most two successful routine-lane blocks. At most one of those blocks is empty.

The request-credit bound and the publication bound are separate obligations. Request spacing alone does not prove publication spacing when execution or queueing delays occur.

### H1-4: nonempty execution and classification

An exempt request forbids empty blocks at proposal execution, not only at the heartbeat check. The request must retain that restriction through queueing and coalescing.

If selection finds no user deploy, the request publishes no block. A pending-pool hint cannot authorize an exempt empty block.

Classification uses the actual processed deploy list of the previous successful own block. A failed request never becomes the previous successful block.

The request binds that block's hash. The proposer rejects a changed predecessor in its execution snapshot before creating another block.

### H1-5: preserved recovery and progress

Stale-LFB recovery stays open to every bonded validator. Its existing LFB-age and own-silence interval checks remain enforced.

H1 never grants a stale-recovery exemption or adds a leader, mode, or credit condition to that lane. The existing deadlock escape remains available.

The pending-deploy backstop remains open to every eligible bonded validator under its existing interval, lag, and cooldown checks. User work never becomes leader-gated.

Convergence retains its existing leader-only, once-per-finalized-block rule. Frontier follow retains its existing eligibility, cooldown, and width checks.

Deploy grace does not itself bypass the cooldown. H1 supplies the only new routine cooldown exception defined by this revision.

An actual exempt block can reset the existing own-silence clock. Recovery timing tests must measure from actual publications, rather than assume unchanged behavior from unchanged predicates.

Given the same snapshot and timing inputs, H1 must leave recovery, convergence, backstop, and frontier-follow decisions unchanged.

When subsequent own publications stop, H1 state must not postpone a due recovery check beyond the existing scheduling contract.

## Formal statement

For `C > 0` and `I > 0`:

```text
recent_self(v) and not h1_exception(v) => not routine_proposal(v)
h1_exception(v) => responsive(v) and pending_lane(v) and previous_own_empty(v)
h1_exception(v) => all_non_cooldown_pending_constraints_hold(v)
exempt_requests(v, (t-C, t]) <= 1
successful_routine_blocks(v, (t-C, t]) <= 2
successful_empty_routine_blocks(v, (t-C, t]) <= 1
exempt_request_published(v) => actual_user_deploy_present(v)
no_user_deploy_after_selection(v) => not exempt_request_published(v)
mode_reentry(v) => exemption_credit_not_replenished_early(v)
not pending_observed_since(v, t-W) => not responsive(v, t)
recovery_due_under_reference_inputs(v) => recovery_due_with_h1_inputs(v)
```

These predicates are verification targets. No passing model or executable binding is recorded for H1 yet.

## Required evidence

- A deterministic state-machine test covers entry, refresh, expiry equality, pending work at expiry, and re-entry without credit reset.
- Boundary tests cover rolling cooldown windows, successive empty and user blocks, and non-default intervals.
- Request tests cover success, failure, busy, cancellation, coalescing, and `NoNewDeploys` without early credit replenishment.
- Execution tests prove that an exempt request cannot publish an empty block after selection changes.
- Classification tests cover failed user deploys, system-only blocks, rejection-only blocks, and failed proposals.
- Queue-delay tests distinguish request issuance from successful publication and check both bounds.
- Recovery tests cover stalls, deadlock escape, convergence, and backstop timing after an exempt block.
- Existing lane tests remain valid with H1 inactive. New active-mode rows check only the intended pending-lane difference.
- Autopropose and manual-path tests establish that their authority is unchanged and excluded proposals are not miscounted as routine heartbeat blocks.
- Positive and deliberately violating fixtures distinguish a valid bounded exemption from an unbounded cooldown bypass.

Source-bound evidence must identify the final heartbeat and proposer source bytes, claim revision, test commands, outcomes, and verification limits.

Soak comparisons and the final performance thresholds require separate maintainer approval. A performance improvement does not discharge the safety and pacing obligations.

## Counter-evidence to leader-only recovery

CI stall instances i1 and i5 showed that one proposer could not rebuild mutual witnessing. Recovery must not silence other eligible bonded validators.

The historical runs are 32284324989 and 32397055615. The committed replay fixtures live in `casper/tests/finalized_floor/oracle_stall_replay_spec.rs`.

This counter-evidence remains applicable. H1 must not restore the rejected leader-only policy.

## Gate rule

Any routine cooldown bypass outside H1-2 refutes this claim. An exempt empty block, early credit replenishment, or a violated rolling bound also refutes it.

A lost nonempty restriction during queueing or coalescing refutes H1-4. A recovery decision that adds H1-specific exclusion refutes H1-5.

Missing tests, absent bindings, or unverified source changes leave the claim pending. They do not establish discharge.
