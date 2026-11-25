# catalog deploy queue 001

[Inventory](inventory.md) | [PlantUML](catalog-deploy-queue-001.puml)

![catalog-deploy-queue-001](catalog-deploy-queue-001.svg)

## Cross-Page References

- `Command: ClosedChecked.result -> NodeDeployStatus [catalog-deploy-queue-006.svg]`
- `Command: ClosedChecked.result -> NodeError [catalog-deploy-queue-005.svg]`
- `Command: ClosedChecked.sig -> Sig [catalog-deploy-queue-006.svg]`
- `Command: Connected.0 -> ChainSnapshot [catalog-deploy-queue-006.svg]`
- `Command: Intake.deploy -> Signed [catalog-crypto-003.svg]`
- `Command: Intake.reply -> IntakeError [catalog-deploy-queue-002.svg]`
- `Command: Intake.reply -> Sig [catalog-deploy-queue-006.svg]`
- `Command: Node.0 -> NodeEvent [catalog-deploy-queue-003.svg]`
- `Command: Resynced.result -> NodeDeployStatus [catalog-deploy-queue-006.svg]`
- `Command: Resynced.result -> NodeError [catalog-deploy-queue-005.svg]`
- `Command: Resynced.sig -> Sig [catalog-deploy-queue-006.svg]`
- `Command: Status.reply -> DeployView [catalog-deploy-queue-007.svg]`
- `Command: Status.sig -> Sig [catalog-deploy-queue-006.svg]`
- `Command: Submitted.outcome -> SubmitOutcome [catalog-deploy-queue-006.svg]`
- `Command: Submitted.sig -> Sig [catalog-deploy-queue-006.svg]`
- `Command: Watch.sigs -> Sig [catalog-deploy-queue-006.svg]`
- `QueueSnapshot: shard -> ShardView [catalog-deploy-queue-005.svg]`
- `QueueSnapshot: validator -> ValidatorView [catalog-deploy-queue-005.svg]`
- `Transition: sig -> Sig [catalog-deploy-queue-006.svg]`
- `Transition: view -> DeployView [catalog-deploy-queue-007.svg]`
- `WatchStart: current -> DeployView [catalog-deploy-queue-007.svg]`
- `WatchStart: current -> Sig [catalog-deploy-queue-006.svg]`
