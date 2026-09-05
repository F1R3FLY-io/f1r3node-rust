# D-02 Certified Finalized Floor and Authority Committee

**Status:** Proposed. Pending maintainer ratification.
**Kind:** Protocol.
**Sources:** dev [finalized-floor specification](../../theory/finalized-floor/finalized-floor-specification.md) rules R-FLOOR, R-COMM, and S8, [Consensus Philosophy](../../CONSENSUS_PHILOSOPHY.md) ground truth 1, [Consensus Protocol](../../CONSENSUS_PROTOCOL.md) section 2 step 2. PR #216 rules R-AUTHORITY, R-POST-STATE-BONDS, R-PROPOSAL-AUTHORITY, R-PARENT-FLOOR, R-CERTIFICATE-DEPENDENCY to R-CERTIFICATE-RESTART, R-CARRIER-EQUIVALENCE, R-CARRIER-PAIR, R-CARRIER-WAKE, invariants S8, S42, S44, models `CertifiedFloorCommitment.tla`, `FinalizationCertificateRetrieval.tla`, `WitnessEquivalentCarrier.tla`, `WitnessEquivalentCarrier.v`.

## 1. Question

Which committee authorizes a block, and does a block carry a signed commitment to its finalized floor with a detachable certificate?

## 2. Position on dev

R-COMM says the committee that validates a block's bonds is `bonds_of(floor(B))`, a pure function of the floor. S8 forbids validation against a non-floor committee. The protocol document's proposal constraint says the sender must be in the bonded validator set with non-zero stake, and the synchrony constraint counts other validators' recent blocks.

Ground truth 1 says validators replay declared parents and never recompute fork choice. Only the recomputed merge base and the bond check bind the main-parent order. A block carries no floor commitment. The floor is derived from the block's frozen justifications on every node.

## 3. Position on PR #216

- **R-AUTHORITY.** The authority committee is the positive active bonds of `post_state(floor(B))`. The justification validators equal that committee exactly. The sender is a member with positive stake. Synchrony weights use the same committee. A bond transition in the block's own post-state never authorizes that block.
- **R-POST-STATE-BONDS.** The serialized bonds field is a post-state cache, not an authority declaration. Only an accepted block may use it to register a new validator's latest-message slot.
- **R-PROPOSAL-AUTHORITY.** Before replay, a proposer derives the prospective floor from its selected parents and defers when the committee differs from the captured LFB committee.
- **R-PARENT-FLOOR.** A protocol-6 block declares at least one parent that descends from its signed finalized floor. Parent floors form one comparable chain. A verified certificate cache never bypasses the check. The receiver never requires equality with its own preferred frontier.
- **Certificate rules.** A block that names an unavailable certificate is stored detached and waits on a typed dependency. Requests are content-addressed, bounded, and retried with backoff. Responses mutate storage only when they hash to the requested digest. Detached blocks and sidecars survive restart.
- **Carrier rules.** A predecessor certificate carrier is eligible by accepted causal membership, protocol version, exact floor hash, and exact post-state. Two honest nodes can certify the same state from different latest-message snapshots, so the digest may differ. Selection returns the carrier and its own digest as one pair. A parked finalizer wakes on any eligible carrier.

The Consensus Philosophy on PR #216 adds one boundary to ground truth 1: a declared parent must carry the block's signed floor, and the receiver still does not require frontier equality. The floor rule R-FLOOR gains a third source, universal certified advancement, which entry D-04 covers.

## 4. Divergence

| Aspect | dev | PR #216 |
|---|---|---|
| Authority derivation | Bonds of the floor validate bonds. Sender must be bonded. | Positive active bonds of the floor post-state authorize sender, justifications, and synchrony. |
| Justification set | One per bonded validator | Exactly the authority committee |
| Bonds field | Current validator set | Post-state cache with no authority |
| Floor commitment | None. Derived on every node. | Signed in the block. Certificate in a sidecar. |
| Missing floor evidence | Floor walk holds when a block is absent (AbsenceHold) | Typed certificate dependency with bounded retrieval |
| Proposer deferral | None | Defer when the prospective committee differs from the captured LFB committee |
| Wire change | None | Yes. Protocol 6 only. |

## 5. Options

- **A. Adopt the full certificate model.** Signed commitments, sidecars, retrieval, and carrier equivalence become normative at protocol 6.
- **B. Adopt the committee rules now and the certificate rules at the protocol boundary.** R-AUTHORITY, R-POST-STATE-BONDS, and R-PROPOSAL-AUTHORITY need no wire change. They are computable from existing blocks. The certificate rules ratify with D-01 and apply only at protocol 6.
- **C. Keep dev.** The floor stays a derived fact with no commitment.

## 6. Unification proposal

Adopt option B.

The committee rules sharpen R-COMM without changing the wire. They make the sender, justification, and synchrony checks read one committee, which closes the head-local authority split that S8 on PR #216 names. This follows principle P2. The bonds-as-cache rule closes an injection path where an invalid block could register a validator slot from unverified bytes.

The certificate rules change block bytes and add a dependency type. They belong to the protocol-6 boundary that D-01 governs. Ratify them as the protocol-6 form of the floor rule, conditional on D-01.

## 7. Ratification checklist

- Confirm that R-AUTHORITY changes no verdict for any block valid on `dev` today. The sources give no differential evidence for this claim.
- Decide the liveness rule for a block whose certificate is unavailable for longer than the retrieval budget. R-CERTIFICATE-REQUEST retains the obligation. The sources state no upper bound.
- After ratification, amend ground truth 1, replace R-COMM with R-AUTHORITY, and update the proposal constraint table.

## 8. Open questions

1. Does the exact-justification rule reject a block from a validator that learned of a bond change late? R-PROPOSAL-AUTHORITY defers on the proposer side. The receiver-side consequence is not stated.
2. Does the read-only observer need certificate retrieval, or does it inherit certificates from the blocks it validates?
