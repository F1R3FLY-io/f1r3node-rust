# catalog casper 012

[Inventory](inventory.md) | [PlantUML](catalog-casper-012.puml)

![catalog-casper-012](catalog-casper-012.svg)

## Cross-Page References

- `Effect: DiscardPending.0 -> BlockHash [catalog-models-027.svg]`
- `Effect: DiscardStored.0 -> BlockHash [catalog-models-027.svg]`
- `Effect: Fetch.0 -> FetchAction [catalog-casper-010.svg]`
- `Effect: ReleaseReservation.0 -> BlockHash [catalog-models-027.svg]`
- `Effect: Reserve.hash -> BlockHash [catalog-models-027.svg]`
- `Effect: Struck.outcome -> StrikeOutcome [catalog-casper-013.svg]`
- `Effect: Struck.peer -> PeerNode [catalog-comm-003.svg]`
- `Effect: Struck.strike -> Strike [catalog-casper-013.svg]`
- `IngestInput: Announced.hash -> BlockHash [catalog-models-027.svg]`
- `IngestInput: Announced.peer -> PeerNode [catalog-comm-003.svg]`
- `IngestInput: BlockPruned.hash -> BlockHash [catalog-models-027.svg]`
- `IngestInput: BlockPruned.peer -> PeerNode [catalog-comm-003.svg]`
- `IngestInput: Inserted.hash -> BlockHash [catalog-models-027.svg]`
- `IngestInput: IntakeRefused.0 -> Refused [catalog-casper-011.svg]`
- `IngestInput: LocalMiss.hash -> BlockHash [catalog-models-027.svg]`
- `IngestInput: Misbehaved.peer -> PeerNode [catalog-comm-003.svg]`
- `IngestInput: Misbehaved.strike -> Strike [catalog-casper-013.svg]`
- `IngestInput: Need.hash -> BlockHash [catalog-models-027.svg]`
- `IngestInput: Outcome.hash -> BlockHash [catalog-models-027.svg]`
- `IngestInput: Outcome.outcome -> WorkOutcome [catalog-casper-011.svg]`
- `IngestInput: Received.0 -> HeldMeta [catalog-casper-008.svg]`
- `IngestInput: StateFetch.root -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `IngestInput: Tick.connected -> PeerNode [catalog-comm-003.svg]`
- `IngestState: reputation -> PeerReputation [catalog-casper-013.svg]`
- `IngestState: reserved -> BlockHash [catalog-models-027.svg]`
- `IngestState: scheduler -> FetchScheduler [catalog-casper-011.svg]`
- `IngestState: tracker -> DependencyTracker [catalog-casper-009.svg]`
- `Sets: cited -> BlockHash [catalog-models-027.svg]`
- `Sets: held -> BlockHash [catalog-models-027.svg]`
- `Sets: ignored_until -> PeerNode [catalog-comm-003.svg]`
- `Sets: needed -> BlockHash [catalog-models-027.svg]`
- `Sets: solicited_held -> BlockHash [catalog-models-027.svg]`
- `Sets: solicited_needs -> BlockHash [catalog-models-027.svg]`
- `StateFetchResult: Dropped.0 -> StateFetchDrop [catalog-casper-011.svg]`
