# FX-18 (lane C): statuses for joiners and the void hook

Status: recommended fix for the P12 track. Not built. Decision 118 moves FX-18 to the post-handoff track, and Meredith rules when that track resumes.

Base: `patch/FX-17` at 488122957. Design r24 (P12), Decisions 109, 115 to 119. Names: V1…Vn validators, u1…um deployers, d1…di deployments, B1…Bj blocks, void reasons (b) to (d). Decision 119 removes reason (a).

## 1. Finding: a joiner cannot select the canonical record without execution

**Label: blocking for FX-18 (Decision 88, High).**

- Several blocks can cite a record for the same target B1. An executing node takes the record of the earliest citer that replays (`ExecutorCore::next_action`).
- A joiner cannot replay. It can check only that the pre-roots and post-roots chain back from the attested anchor root.
- The roots do not fix the statuses:
  - An attestation carries only (block, root).
  - A record's `ran` flags are not in its roots.
  - Every void record has post = pre. So the roots cannot show which void reason applies, and they cannot show a void record from a normal record that changes nothing.
- **Attack:** V1 executes B1, copies the honest roots, and cites a record with changed `ran` flags or another void reason. V1's block comes before the honest citer in the order. Executing nodes dismiss that record. A joiner takes it, computes other gate verdicts after the anchor, and attests other roots.
- **Result:** one Byzantine validator can keep every joiner from computing the network's state.

## 2. Recommended fix: a chain of record digests

| Change | Detail |
|---|---|
| Record field 14, `prev_digest` | The digest of the canonical record at the previous position. At the anchor, the digest that the join checkpoint gives. |
| Attestation field 3, `record_digest` | The digest of the canonical record at the attested position. P5 counts support for each (root, digest) pair. Lane A2 owns this format. |
| Replay check | `next_action` dismisses a candidate whose `prev_digest` is not the canonical digest at the previous position, and records `InvalidRecord`. The check is the same as the check of `pre_root`. |
| Execution | `prev_digest` comes from the last speculative record, or else from the canonical record, as `pre_root` does. |
| Cited key | `cited` is keyed by (`pre_root`, `prev_digest`). If the key is `pre_root` only, a record that is dismissed and executed again is never cited again, and its position stops. |
| Joiner | It verifies the quorum on (LFB, root, digest). Then it follows `prev_digest` back through the citing blocks that it downloads. Each position has one record with the expected digest, so no execution is necessary. |

**Cost:** 32 bytes for each record and each attestation, and one comparison at each replay.

**Alternatives that were not chosen:**
- Write the statuses into the tuple space, so that the attested root covers them. This changes the execution and the cost of every block.
- Trust the statuses of the responder. This violates the quorum rule of P12-2.

## 3. The void hook: field `returned`

| Item | Detail |
|---|---|
| Field | Record field 15, `returned`: the full signed deployments (`DeployDataProto`) that the void record gives back, in block order |
| (b) | Every deployment of the block |
| (c) | The readable deployments only |
| (d), and normal records | Empty |
| Release | `V6Engine::release_voided` reads the deployments from the record. It does not load the void block. |
| Check | `next_action` refuses a void record whose `returned` list differs from the block's deployments for its reason, with `InvalidRecord`. The record digest covers the field. |
| Size | A void record carries again the bodies of the returned deployments. Void blocks are rare. |

## 4. Statuses from the record alone

With `returned`, each record gives its statuses without its block body:

| Record | Status |
|---|---|
| Normal | Each deployment with `ran` true is `Included` |
| (d) | Each deployment is `Discarded` |
| (c) | Each envelope that is not in `returned` is `Discarded` |
| (b) | No status |

- The joiner takes each target's block number from the downloaded block. It keeps a status only inside the lifespan window, as `ExecutorCore::prior` does.
- An imported status gets the anchor's position. Every later attested position covers it, so `prior_at` keeps it.

## 5. Plan when the track resumes

- **Files:**
  - `v6/block_record.rs` and `v6/record.rs`: fields 14 and 15.
  - `v6/core.rs`: the digest check, statuses from the record, and the cited key.
  - `v6/store.rs`: the import of statuses.
  - A new `v6/join.rs`: the statuses from the anchor and the downloaded blocks.
  - `v6/engine.rs`: `release_voided`.
- **Lane A2:** the attestation field and the P5 tally.
- **Tests:**
  - A joiner ignores a forged record that is cited first.
  - Reasons (b), (c) and (d) give the correct statuses.
  - A broken chain is refused.
  - The void deployments are released when the block body is absent.
