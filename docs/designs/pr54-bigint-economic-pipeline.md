# Proposal: exact amounts in the economic pipeline

This branch is a proposal for team review, based on PR #54 at `604e1ea82b3b28ba4a12915c92fd2d85c1747a55`.
The user authorized a separate branch, one commit, and publication of these proposed changes.
`isBigInt()` and the staking migration are new proposal code, not existing team-approved protocol features.
The protocol number remains 1 at the user's direction. The team must decide activation, versioning, and migration before any deployment.
Do not deploy this branch into an existing or mixed-version network. The unchanged version number does not establish compatibility.

PR #54 accepts native ASI balances as BigInt values. Staking still uses signed 64-bit amounts across contracts and consensus.
The interpreter also lacks the type predicate required by lsASI contracts. Fee multiplication can overflow before payment.

## Design

Add `isBigInt()` as a zero-argument predicate on evaluated values. Only a single BigInt scalar returns true.
Other valid values return false. The existing method dispatcher supplies method-call gas accounting.

Use exact integer amounts for staking throughout the contract, genesis configuration, consensus model, and API.
Keep block heights, indexes, and epoch counters as their existing integer types.
Do not scale, round, clamp, or truncate stake to fit a smaller consensus weight.
Preserve legacy bond decoding where possible. Define an explicit canonical representation for larger stakes.
Validate the representation at protocol boundaries and preserve deterministic block hashing.
Public monetary values use decimal strings.

Use checked precharge and refund arithmetic. Reject invalid fee metadata and unrepresentable precharges before admission.
Apply the same checks during execution and replay. Compute refunds without narrowing unsigned costs.
Keep fee metadata and its supported range unchanged.

## Delivery and verification

Work on `proposal/pr54-bigint-staking-fees`, directly based on PR #54. Commit and push the reviewed proposal to the existing team repository.
Use interpreter, protocol, consensus, API, and economic contract regression tests.
Run an independent Opus review after implementation. Resolve confirmed findings before reporting readiness.
Validate locally. Deployment and live state changes are outside this delivery.

The staking storage and wire changes require coordinated node upgrades and explicit state migration or a fresh test genesis.
A local test does not establish compatibility with an existing deployed shard.

## Compatibility boundaries

The proposed rules require a coordinated activation strategy and a fresh genesis or an explicit migration.
The protocol constant remains 1. No production activation decision is included in this proposal.
Legacy nonnegative stakes through `i64::MAX` retain their protobuf field and bond hash encoding.
Larger stakes use `BondProto.stakeBigInt` with canonical signed big-endian bytes and zero in the legacy field.
The decoder rejects negative, nonminimal, mixed-field, and small-value extension encodings.
Large bond hash leaves use tag `0x16` and the encoded protobuf, which supplies field lengths.
Legacy bond leaves retain tag `0x15` and their existing bytes.

The metadata store uses schema marker `bigint-v1`. Startup rejects existing unmarked metadata before deserialization.
The guard preserves all records and requires an explicit migration or a fresh data directory.
The detached observation snapshot uses schema version 3. Observation JSON uses decimal strings for weights.

Clients must read `BondInfo.stakeBigInt`. The legacy `stake` field is deprecated for monetary reads.
The legacy field remains populated only when the amount fits signed 64-bit storage. Larger amounts put zero in that field.
The decimal field always carries the full value. Clients that only read the legacy field cannot support this protocol.
HTTP staking amounts use decimal strings. Configuration bond limits accept decimal strings. Heights and epoch counters remain JSON numbers.

Fee metadata remains signed 64-bit. Admission rejects invalid limits, negative prices, and multiplication overflow before storage.
Block validation returns `InvalidDeployCost` before replay. Proposal selection excludes invalid stored fee metadata.
Genesis can retain zero-stake placeholders. Public bond, delegation, and undelegation operations require positive BigInt amounts.

## Decisions reserved for the team

The team must approve the new language predicate, staking representation, and stricter fee validation rules.
The team must select the protocol version or activation mechanism and assess historical replay compatibility.
New checks reject malformed fees and forged failed-precharge metadata that previous code can accept.
Changing those rules without coordinated activation can split a network or prevent replay of historical blocks.
The schema guard refuses unmarked metadata but does not implement a state migration.

The proposed default bond maximum is 2^256−1. This changes genesis for shards that use the defaults.
Exact rational arithmetic can change stored fault-tolerance f32 bits, including with small stakes.
Metadata uses the num-bigint serde representation within bincode. The team must approve this storage format and provide migration tooling.
The large-stake decoder currently has no separate byte-length limit. Message limits bound input size, while PoS limits bound accepted contract amounts.
The team must choose a wire stake limit before network rollout.

The existing slashing formal models describe a historical verdict set and do not cover `InvalidDeployCost`.
Production regression tests verify that the new verdict does not authorize slashing. B11 verification covers canonical snapshot encoding, not slashing taxonomy.
