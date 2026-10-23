# Deploy admission

Issue #710 shows sustained intake above local block capacity.
The deploy pool retains included deploys until their terminal verdict, so its total size does not measure the admission queue.

Admission counts fresh predecessors in the block selection order.
The order uses validity height, timestamp, and signature.
An eight-block sample measures actual first inclusions in this validator's accepted blocks while fresh work waits.
Three blocks of measured service limit the new deploy's queue position.
This delay budget permits short bursts without filling the full validity window.
The estimate does not guarantee execution when service or consensus conditions change.

Before the first sample, capacity is eight deploys per block, bounded by the shard cap.
Idle blocks do not reduce capacity.
A block with demand and no first inclusion contributes zero.
A zero service estimate permits one probe only when the fresh queue is empty.
Admission and insertion share the existing pool lock.

Expiry removal first saves an open lifecycle row.
The register evaluates the row after the consensus bound and checks floor membership before an expiry verdict.
Another block can therefore finalize a dropped local copy.
The existing row encoding supports restart recovery without a migration.

See the [specification](../specs/deploy-admission.md) and [correctness claims](../claims/deploy-admission.md).
