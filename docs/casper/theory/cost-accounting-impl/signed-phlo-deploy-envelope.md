# Signed phlo deploy envelope

## Purpose and execution boundary

`FundedDeploy` binds a deploy body to its complete canonical funding intent.
`OfferedFundedDeploy` also binds a separate offered `phloPrice` and execution `phloLimit`.
The funding intent includes owner price ceilings, usage limits, permitted schedules, the selected schedule, exposure, and source permissions.
An envelope signature authenticates these bytes together with the signer policy and exact selected witnesses.

This type provides a signing boundary, not permission to execute a funded deploy.
Existing `DeployData` decoders reject the funding field, including an explicitly present empty field.
They also reject authorization versions other than `0x00060001`.
The dedicated decoders return distinct funded envelope types, not the type accepted by existing deploy admission.
No automatic conversion removes the funding controls.

Native admission must also verify custody authority, schedule eligibility, conservative resource bounds, and settlement before it can accept this format.
Signature validity alone does not prove any of these conditions.
This implementation does not change voting, fork choice, finality, or the Casper block version.

## Signature-bound funding checks

[`PhloFundingIntentView::check_signed_family`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/intent.rs) connects envelope authentication to the existing funding-family checker.
This method checks the member cap, canonical funding-record equality, envelope signatures, and the family's consent and schedule constraints.
It rejects a valid signature over a different funding record.
It also rejects a changed deploy body with signatures from the original body.

The result retains immutable references to the authenticated envelope and checked funding intent.
Its `verified_witnesses` iterator includes only members with verified, nonempty signatures.
Unsigned threshold members remain part of the signed policy but do not become signature witnesses.
The member cap counts the complete policy, including absent members.
Source caps separately limit physical custody entries.

The checked intent supplies the existing canonical settlement capture, including original sources, debits, fees, and refunds.
An envelope signature does not grant control over every custody identity listed in the record.
Native admission must authenticate each source's applicable capability or withdrawal authority against chain state before it publishes any effect.
It must execute the retained envelope's body and bind the execution evidence to that body and state.
The signature-bound checker does not perform these stateful steps or activate the funded format in node admission.

## Wire representation

The existing `DeployDataProto` carries the new payload through an optional field.
An absent field and an explicitly present empty field remain distinguishable.

| Component | Representation | Rule |
| --- | --- | --- |
| Funding intent | Optional bytes at protobuf tag 21, `fundingIntent` | Required and canonically encoded for funded deploys |
| Funded-v1 authorization | `authorizationV61.formatVersion = 0x00060002` | `FundedDeploy` preserves its original payload and rejects nonzero scalar offers. |
| Offered authorization | `authorizationV61.formatVersion = 0x00060003` | `OfferedFundedDeploy` signs and retains both scalar offers. |
| Deployment identity | `deployId`, 32 bytes | Must equal the authenticated envelope commitment |
| Signer policy | Existing AllOf or Threshold representation | Existing canonical order and duplicate-authority rejection apply |
| Witness selection | Existing bitmap and indexed witnesses | Witnesses must match the bitmap exactly |
| Offered price | Signed `int64 phloPrice`, original protobuf tag 7 | Nonnegative exact offered price, not an owner ceiling. |
| Execution limit | Signed `int64 phloLimit`, original protobuf tag 8 | Nonnegative execution limit. |
| Retired signer share | Reserved tag 15 | No per-signer scalar share field. |

The authorization container retains its protobuf name for schema continuity.
Its numeric format distinguishes funded authorization from existing v6.1 authorization.
This number is not a Casper block version.

Legacy authorization fields cannot accompany a funded envelope.
These fields include the primary signature fields, flat cosigners, legacy threshold, and signature algebra.
Changing only the authorization version cannot convert an existing signature into a funded signature.

## Canonical signing payload

Let `LP64(x)` mean an unsigned, eight-byte, big-endian length followed by the bytes of `x`.
Let `U32BE(x)` mean the unsigned, four-byte, big-endian encoding of `x`.
Let `bodyIntentV61` mean the existing canonical deploy intent, including its original two-byte version prefix.

The funded payload is:

```text
0x00 0x02
|| LP64("f1r3node:funded-deploy-intent:v1")
|| LP64(U32BE(0x00060002))
|| LP64(bodyIntentV61)
|| LP64(canonicalFundingIntent)
```

Here, `||` means byte concatenation.
The complete funding record is included, not only its digest or selected scalar fields.
The existing envelope commitment includes this payload, the canonical policy, and the presence bitmap.
The existing scheme-specific envelope signing procedure then signs that commitment.

The outer cryptographic domains and envelope protocol value remain those of the existing v6.1 envelope machinery.
The new inner prefix, domain, and authorization format distinguish the funded payload.
An existing v6.1 intent begins with `0x00 0x01`.
Thus the old and funded payloads cannot be equal, regardless of their remaining bytes.
This preserves existing signing vectors without interpreting old signatures as new consent.

The constructor stores the validated funding bytes and canonical signing payload in private fields.
Callers can inspect the body and funding bytes but cannot mutate individual fields through these accessors.
The type does not implement unchecked deserialization.

### Offered-price payload

The offered format preserves the funded-v1 payload and its signing vectors as a separate format.
It uses this payload:

```text
0x00 0x03
|| LP64("f1r3node:offered-funded-deploy-intent:v1")
|| LP64(U32BE(0x00060003))
|| LP64(bodyIntentV61)
|| LP64(canonicalFundingIntent)
|| LP64(U64BE(phloLimit))
|| LP64(U64BE(phloPrice))
```

`U64BE` encodes a nonnegative scalar as an eight-byte, big-endian integer.
The constructor rejects negative signed integers before this conversion.
Zero remains a signed value even when protobuf omits its default-valued field.
Old decoders reject nonzero scalar offers rather than discard them before signature verification.
Exact version checks prevent retries through another decoder after a rejected envelope.
The authorization format is independent of the Casper block version.

[`SignedPhloIntentWire.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloIntentWire.v) proves complete field binding and separation from both existing payload formats.
It also proves that equal offered payloads bind equal decoded funding records, limits, and prices.
These proofs assume bounded canonical encodings. They do not prove cryptographic signature security.
The [native regressions](../../../../models/src/rust/signed_phlo_deploy/offered_tests.rs) check independent framing, restored tags, scalar mutations, version separation, and numeric boundaries.

## Decode procedure and bounds

1. Check the decoded protobuf size against the supplied deploy-byte limit.
2. Require the funded authorization format and reject legacy authorization fields.
3. Validate the policy, bitmap, witness indices, signer order, and active signature schemes.
4. Require the funding field and decode it with the supplied structural limits.
5. Re-encode the funding record and require exact equality with the supplied bytes.
6. Construct the canonical signing payload within its supplied byte limits.
7. Verify the selected signatures and the deployment identity.

The limits separately bound deploy bytes, signing bytes, funding bytes, owners, schedules, sources, resource permissions, and authority nodes.
The signing payload limit includes its two-byte prefix.
No two-signer restriction applies to envelope authorization.
Absent threshold members do not supply signature witnesses.
The later custody check must separately determine which authenticated authorities can fund each source.

The protobuf size check operates on an already decoded message.
The network layer must separately bound raw message bytes before protobuf allocation.
These structural limits do not establish an end-to-end resident-memory bound or replace host-work accounting during admission.

## Checked envelope retention

[`DeployEnvelope`](../../../../models/src/rust/deploy_envelope.rs) retains one authenticated envelope without a mutable copy of its legacy body.
The wrapper uses exact authorization-version dispatch.
A failed decoder does not cause a retry through another format.

| Format | Retained payload | Identity |
| --- | --- | --- |
| Legacy | Verified `Cosigned<DeployData>`, original primary index, and original wire-order permutation | Original primary signature |
| `0x00060001` | Verified `Cosigned<DeployData>` | Envelope commitment |
| `0x00060002` | Verified `Cosigned<FundedDeploy>` | Envelope commitment |
| `0x00060003` | Verified `Cosigned<OfferedFundedDeploy>` | Envelope commitment |

The legacy primary signer remains unchanged when canonical signer order differs from the received primary position.
The wrapper also retains the order of additional legacy signers as private indices into the verified signer list.
The constructor requires a complete permutation, so the metadata cannot add, omit, or repeat a signer.
Legacy signatures authenticate the body, not this ordering. Historical processed-deploy serialization preserves the ordering, which can affect block bytes.
The encoder therefore preserves legacy order instead of imposing a new order on historical records.
This order guarantee applies to flat legacy authorization fields.
Legacy ingress with `sig_algebra` derives its verified members from that algebra and normalizes them into a flat envelope.
In that path, ignored flat cosigner hints do not define the retained member order.
For threshold envelopes, absent members remain in the policy but cannot become the selected primary witness.
Read-only views expose each concrete payload type.
No view converts funded signatures into legacy body signatures.

The byte decoder checks the raw byte limit before protobuf decoding.
The decoded-message constructor also checks the encoded size and complete member count.
These checks complement the network transport limit. They do not establish a complete node memory bound.

Format recognition and execution permission remain separate checks.
`require_format` rejects a recognized format unless the caller's execution policy permits it.
Neither decoding nor storage changes a shard's active execution policy.

### Pending records and body-only consumers

[`PendingDeploy`](../../../../block-storage/src/rust/deploy/pending_deploy.rs) owns one checked envelope.
Its identity bytes and encoded length derive from that envelope and have no mutation interface.
The encoded length includes every signer, not only the primary signature.
This distinction prevents compound legacy envelopes from understating their wire size.

The historical bincode layout retains its three fields: typed identity, identity bytes, and body envelope.
Deserialization verifies the signatures and requires both stored identities to match the checked envelope.
Serialization borrows the checked payload and preserves the historical field order.
It does not serialize the new runtime structure directly.

A body-only adapter permits body-v6.1 envelopes and legacy envelopes whose complete original signer order matches canonical order.
It rejects funded envelopes because a body-only projection would discard signed funding controls.
It also rejects legacy envelopes whose primary or additional-signer order would change after projection into canonical signer order.
The full checked envelope and versioned store preserve those envelopes without this projection.
Historical processed-deploy replay needs an adapter that preserves the original identity while exposing verified body authorization.
The restricted pending-record adapter alone does not establish that replay compatibility.

Historical processed decoding and legacy ingress also select different authorization sources when `sig_algebra` is present.

| Decode boundary | Authorization source | Treatment of `sig_algebra` |
| --- | --- | --- |
| Historical legacy processed record | Flat primary, cosigners, and threshold | Ignores the field and drops it on output |
| Legacy ingress | Algebra members and threshold, when present | Validates the algebra before normalization |

A processed-record refactor must preserve the historical source selection through a dedicated decoder.
Reusing the ingress decoder would change acceptance or the required threshold.
For example, valid flat authorization with invalid algebra passes historical processed decoding but fails ingress decoding.
Matching signer sets with different flat and algebra thresholds also retain different authorization requirements at these boundaries.

The current pending protocol gate permits legacy envelopes below protocol version six and body-v6.1 envelopes from version six.
Recognition of a funded format does not activate that format, even when the caller supplies a higher protocol version.
Funded execution requires a separate, explicit integration of admission, processed deploys, and replay.

Historical pending-record reads repeat signature verification.

### Pending API responses

The pending API retains `DeployEnvelope` records through the Casper interface and `PendingDeploysSnapshot`.
The query reads complete records from the configured pending and rejected stores.
It does not use the proposal format gate to decode a read-only response.
Listing a record does not permit its execution. Proposal admission still requires the active execution policy.

The query preserves the existing validity-window and merge-scope filters.
A record present in both queues appears once, with the rejected flag set.
The optional deployer filter compares the checked primary public key.
Results sort by timestamp, then typed deploy identity, before the existing result cap applies.
Identity comparison uses the retained checked identity. It does not recompute a body-only commitment or substitute an empty commitment after failure.

The gRPC response encodes the complete envelope, including signed funding terms and offered phlo parameters.
Serialization errors produce an error response instead of a partial list.
The HTTP response remains a summary of the body, primary signer, identity, and queue status.
That summary is not a signed envelope and cannot replace one for submission or replay.

The pending API regression covers funded and offered-funded records, exact envelope restoration, queue deduplication, primary filtering, and deterministic ordering.
It also checks that the historical execution gate still rejects funded records after the query succeeds.
This check prevents unchecked stored data from becoming authenticated runtime authority.
Repeated-read tests cover threshold policies with 3, 17, and 65 members, including an absent first member.
The database regression repeats these reads after two reopen cycles and checks that the historical record bytes remain unchanged.
These tests do not establish database latency bounds or production throughput.

### Processed records

[`ProcessedDeploy`](../../../../models/src/rust/casper/protocol/casper_message.rs) owns one private checked envelope and derived identity bytes.
Read-only accessors expose the body, primary signer, complete signer list, effective threshold, and typed identity.
Receipt fields remain independently mutable. Changing a receipt does not change signed authorization or funding controls.

`from_envelope` accepts an already checked envelope.
The signed-body constructors return errors when signature verification fails.
Runtime result construction performs this check before the user checkpoint and evaluation.
The result retains the checked envelope instead of reconstructing a primary-only signed body.

`from_proto` preserves the historical body-format decoder boundary.
`from_proto_with_limits` additionally accepts funded formats with explicit byte and member limits.
Neither decoder authorizes funded execution. The body-only replay adapter rejects funded formats.
Both funded formats retain their full payload when a processed record passes through the bounded codec.

The historical legacy decoder verifies the flat primary and complete flat authorization, without selecting `sig_algebra` as authority.
It preserves additional-signer order and supported algorithm alias spellings through private, validated encoding metadata.
The primary algorithm name follows the historical decoder's normalization.
Legacy wire metadata does not replace the verified signer list.

Historical records with one signer ignore the wire threshold when they derive authorization.
For compound legacy records, positive thresholds specify the quorum. Nonpositive thresholds select all signers.
The decoder retains the historical threshold field for re-encoding, separately from the effective authorization threshold.
These compatibility rules do not relax body-v6.1 or funded threshold validation.
The lossless pending adapter rejects metadata that its body envelope cannot represent exactly.

The processed body-authorization adapter returns canonical verified signers for legacy replay, while the processed record retains the original identity and wire order.
Callers must use the processed identity for indexing and receipt binding, rather than derive an identity from canonical signer position.
Full funded execution still requires the runtime and replay integration described below.

Tests distinguish invalid authorization from invalid execution receipts.
Unsigned changes to the body or authorization must fail checked construction or decoding.
A different, correctly signed envelope can pass decoding but must not match another envelope's execution evidence.
Receipt tests retain valid authorization while changing costs, event logs, state roots, admission status, or authority evidence.
Replay must bind each receipt field and reject inconsistent evidence.

The protocol-domain accessor rejects legacy identities in version-six blocks and envelope identities in earlier blocks.
This identity check does not activate funded execution.

### Nested block decoding

The explicit-limit decoder continues through `Body`, `BlockMessage`, `ApprovedBlockCandidate`, `ApprovedBlock`, `BlockApproval`, and `UnapprovedBlock`.
Each container passes the same caller-supplied `DeployEnvelopeLimits` to every processed deploy.
The limits bound each deploy's encoded payload and authorization membership.
They do not replace transport limits, aggregate block-size checks, or receipt-evidence limits.

The decoder preserves deploy order, cardinality, complete envelopes, and receipts.
If any deploy fails decoding, the enclosing container returns an error rather than a partial block.
The existing header, state, and effect-sequence checks still apply.
Successful decoding does not establish a valid block signature, successful replay, or funded-format activation.

The historical `from_proto` entry points keep their body-format behavior.
Callers must select `from_proto_with_limits` explicitly to decode funded formats.
No container supplies a hidden default limit or converts a funded envelope into a body-only envelope.

### Versioned persistence

[`VersionedDeployStorage`](../../../../block-storage/src/rust/deploy/versioned_deploy_storage.rs) stores canonical envelopes in separate database namespaces.
Each value contains a storage schema number, an authorization version, and canonical protobuf bytes.
The key contains the typed deployment identity, which distinguishes legacy signatures from envelope commitments.

| Store | Database namespace |
| --- | --- |
| Authenticated pending envelopes | `deploy_authenticated_envelopes_v1` |
| Authenticated rejected envelopes | `rejected_authenticated_envelopes_v1` |

The earlier stores retain their original bincode types and namespaces.
They include `deploy_storage`, `deploy_envelope_storage_v6`, and `rejected_deploy_buffer`.
The production store mapping registers both funded namespaces in the existing `deploystorage` LMDB environment.
This placement permits strict atomic batches with historical deploy records without changing the RSpace environment layout.
The new component does not rewrite these records or automatically migrate live consumers.
Live pending and processed-deploy consumers require explicit integration before funded formats can execute.

Restore uses the following checks:

1. Check the raw key and value sizes before bincode decoding.
2. Decode with fixed-width integers, a byte limit, and trailing-byte rejection.
3. Require storage schema version one and a recognized authorization version.
4. Decode the envelope with the applicable byte and structural limits.
5. Verify signatures and require agreement between the stored format, identity, and envelope.
6. Require exact canonical protobuf re-encoding.

Every envelope read repeats these checks. Opening a store checks all records through a streaming iterator.
Malformed records produce an error. Validation does not delete or replace them.
Canonical protobuf encoding preserves signed fields, policy members, selected witnesses, and the retained legacy signer order.
It does not preserve arbitrary protobuf field order or unknown fields.

Key serialization uses the same byte cap as key decoding.
With this bincode configuration, a legacy key occupies 12 bytes plus its signature length.
A commitment key occupies 36 bytes, including its enum discriminator.
The serializer checks the encoded size before it allocates the output buffer.
These bounds do not include memory already owned by the caller.

Insertion uses the key-value store's atomic insert-if-absent operation.
A duplicate insertion does not overwrite the existing record and checks any existing record for corruption.
Deletion uses the store's atomic delete result, not a separate existence check.
The component does not make a pending-to-rejected move atomic across two databases.
Such a move requires a separate lifecycle transaction or a recoverable journal.

### Pending storage facade

[`KeyValueDeployStorage`](../../../../block-storage/src/rust/deploy/key_value_deploy_storage.rs) combines historical storage with an explicitly bounded funded store.
`new_with_limits` opens all three namespaces. The historical `new` constructor and `from_legacy_stores` do not open the funded namespace.
Funded insertion through a historical-only handle returns an error.
Node startup must select the bounded constructor before funded submissions can use this facade.

| Envelope format | Insertion method | Destination |
| --- | --- | --- |
| Historical single signature | `add_pending_if_absent` | `deploy_storage`, with the original bincode representation |
| Body-v6.1 authorization | `add_pending_if_absent` | `deploy_envelope_storage_v6`, with the complete body authorization |
| Funded or offered-funded authorization | `add_pending_if_absent` | `deploy_authenticated_envelopes_v1`, with the complete versioned payload |

The historical signature namespace stores one `Signed<DeployData>` value, not compound authorization.
Body-v6.1 and funded formats preserve their complete signer sets under the supplied limits.
The funded facade rejects historical formats in its namespace, including during store opening.
It does not copy historical rows into that namespace.

`get_pending` checks the stored identity and rejects ambiguous locations.
`read_all_pending` returns checked records from all configured namespaces without granting execution permission.
The historical `read_all_for_protocol` method rejects a nonempty funded namespace instead of returning an incomplete candidate set.
`remove_pending` selects the namespace from the checked format and deletes only that record.

`contains_pending_id` searches both historical and funded namespaces for the typed identity.
It checks row presence without decoding the payload. It rejects an identity present in both namespaces.
This membership check does not authenticate stored bytes or authorize execution.
`remove_pending_by_id` first loads and checks the complete envelope, then removes its row from the selected namespace.
An invalid envelope or ambiguous location causes an error before deletion.
Both removal methods return the backend deletion count as a Boolean, not an earlier presence observation.
Concurrent removals therefore report one successful deletion when no insertion intervenes.

Admission duplicate checks and terminal cleanup use the typed identity methods.
Proposal cleanup uses the checked pending record when that record is already available.
These calls preserve the existing expiration, recovery, and terminal-status decisions.
Separate namespace reads are not a cross-namespace transaction. Valid inserts retain the format-specific identity and namespace contract.
These queue operations do not debit purses or publish execution effects.

Each insertion uses atomic insert-if-absent in the selected namespace.
Cloned handles share that operation without a new application-wide lock.
This property does not make a pending-to-rejected move atomic across namespaces.

### Rejected storage facade

[`KeyValueRejectedDeployBuffer`](../../../../block-storage/src/rust/deploy/key_value_rejected_deploy_buffer.rs) retains complete envelopes for later proposal attempts.
`new_with_limits` combines `rejected_deploy_buffer` with `rejected_authenticated_envelopes_v1`.
The historical namespace retains its original bincode representation. The funded namespace uses bounded versioned records.
The historical `new` constructor and `from_legacy_store` do not enable funded storage.

Each bounded `add` batch prepares every key and value before the backend receives any mutation.
The backend then commits all mutations through `strict_atomic_mutate`.
An encoding error leaves both namespaces unchanged. A backend transaction error also leaves both namespaces unchanged.
The same transaction interface removes mixed-format batches.
Repeated keys retain historical batch semantics: the last put wins, and repeated deletion has no additional effect.

The LMDB backend requires both namespaces in the same environment.
It rejects mixed environments or unsupported backends without a sequential fallback.
The in-memory backend requires stores from the same manager and stages changes before publication.
Both backends provide the transaction boundary without a new facade-wide lock.

For example, a batch can contain one historical deploy and one funded deploy.
If the funded record exceeds its configured limit, neither record enters the buffer.
If validation succeeds, the transaction publishes both records or neither record.
No queue operation charges a purse or changes execution authorization.

`get_by_id` checks identity correspondence and rejects duplicate locations.
`remove_by_id` reports the atomic deletion result from the selected store.
`read_all` combines checked records from both namespaces.
Its separate namespace reads do not constitute a cross-namespace snapshot during concurrent mutation.
Callers that require a frozen candidate set must use their existing lifecycle coordination.
These batch guarantees do not establish an atomic move between the pending and rejected queues.

### Block storage

[`KeyValueBlockStore`](../../../../block-storage/src/rust/key_value_block_store.rs) supports explicit deploy-envelope limits through `create_from_kvm_with_limits` and `with_deploy_envelope_limits`.
The limits select the bounded decoder for ordinary blocks and approved-block candidates.
Historical constructors retain historical decoding and reject funded envelope writes.
This check prevents a handle from storing a funded block that its own decoder cannot read.

Before storage, each processed envelope must pass the selected format and size checks.
The check precedes block-byte writes and finalization-certificate persistence.
Detached blocks awaiting a certificate use the same check. Existing certificate shape and commitment checks still apply.
Approved-block writes check every envelope in the candidate block.

The bounded path preserves envelope order, repeated records, signed funding terms, and execution receipts.
It retains existing block compression, storage keys, and certificate handling.
The existing decompressed-block limit remains separate from the per-deploy limits.
Store configuration does not grant execution permission or validate funding settlement.

Reads with insufficient limits return an error without deleting the stored record.
Changing limits requires a handle configured for the new limits. It does not migrate stored bytes.
Restart tests verify exact block and approved-block restoration, typed identity lookup, and rejection without mutation under smaller limits.
Generated sequences verify repeated funded records, receipt preservation, empty blocks, and rejected rewrites.

### Persistence proof boundary

`SignedPhloIntentWire.v` proves exact format dispatch and the schema, format, identity, and decode requirements of successful restoration.
It proves complete retained-envelope equality when the supplied encoder and decoder satisfy their round-trip premise.
It also proves that successful decoding does not imply execution permission.
Key-size lemmas establish the legacy payload bound and the exact configured-size condition.
Pending-record lemmas preserve the complete envelope, derived identity, and original primary under the same codec premise.
Routing lemmas separate funded formats from historical namespaces and preserve unrelated namespaces and keys.
Deletion lemmas prove target removal, preservation of other namespaces and keys, idempotence, and commutativity.
These lemmas model namespace routing. They do not prove the storage backend or cryptographic identity construction.
Lookup lemmas reject two occupied locations and preserve a unique record.
Batch lemmas preserve storage after preparation failure or transaction failure.
They establish complete publication, unchanged unrelated rows, ordered batch composition, and last-write behavior for repeated keys.
These lemmas assume an atomic backend commit. They do not prove that the Rust backend implements that assumption.
Block preparation lemmas require every envelope to pass the selected check before storage and preserve the complete envelope sequence.
They reject funded formats through the historical path and reject the whole preparation when any envelope fails.
The native block tests exercise this precondition against actual codecs and LMDB storage.
The body-adapter lemmas reject funded projection and preserve the original legacy primary on successful projection.
They also require canonical legacy order for a successful body-only projection.
Order-projection lemmas preserve list length and prevent references from creating signers outside the verified list.

The processed-record model separates the complete envelope from the execution receipt.
Arbitrary receipt updates preserve authorization and legacy order. Updates to distinct record keys commute under the model's atomic update semantics.
Restore preserves both fields when the envelope codec satisfies its round-trip premise, and rejects envelope decoding failure before retaining a receipt.
Native processed-record tests check these contracts against the checked-envelope representation and protobuf codec.
The sequence-decoding proofs preserve every checked record, its position, and the number of records.
They prove rejection when any entry fails decoding and round-trip preservation under the per-record codec premise.
Generated nested-container tests exercise mixed envelope formats, repeated entries, empty lists, and invalid entries at different positions.
The abstract contracts do not prove Rust refinement or shared-wallet settlement concurrency.

The [model tests](../../../../models/src/rust/signed_phlo_deploy/envelope_tests.rs) check native round trips, format substitution, threshold witnesses, and canonical record rejection.
The [storage tests](../../../../block-storage/src/rust/deploy/versioned_deploy_storage/tests.rs) check restart behavior, unchanged legacy records, corruption, duplicate races, and key limits.
Facade tests exercise mixed-format operation histories, complete envelope retention, namespace isolation, restart, and concurrent insertion through cloned handles.
Pending tests also cover typed membership, typed removal, concurrent removal, and rejection of ambiguous storage without deletion.
Generated histories compare both removal methods and membership queries with an independent map of retained envelopes.

| Deletion property | Native check |
| --- | --- |
| The selected row becomes absent. | Retirement examples and generated mixed-operation histories check absence after removal. |
| Other namespaces remain unchanged. | Generated histories compare raw historical stores before and after funded operations. |
| Other keys retain their complete envelopes. | The reference map checks all retained envelopes after each generated operation. |
| Repeated deletion preserves absence. | Retirement examples and generated histories check repeated deletion and its return value. |
| Deletion order does not change the result. | All 16 format pairs run in both orders and compare every remaining envelope. |

Concurrent removal tests use eight cloned handles against both the in-memory backend and LMDB.
These tests check one successful deletion per stored envelope without an intervening insertion.
They supplement the abstract proofs with backend checks. They do not exhaust every operating-system schedule.

Rejected-buffer tests exercise mixed-format batch histories, bounded preparation failure, cross-environment rejection, restart, and concurrent complete-batch outcomes.
Generated batch histories compare full envelopes with an independent map, including empty batches and repeated keys.
The [production transaction Loom tests](../../../../formal/loom/cost_accounting/tests/loom_production_sparse_transaction.rs) check competing mixed-namespace insertion and removal.
They import the production staging function and replace synchronization primitives with Loom primitives.
Exploration uses three threads and a 5,000-branch failure limit per execution, without preemption, permutation, duration, or checkpoint-resume cutoffs.
A negative control removes the transaction guard and must expose partial publication.
This check covers transaction interleavings, not envelope cryptography or LMDB internals.
Generated operation histories compare retained envelopes against an independent map after insertion, deletion, lookup, and store reopening.
The [pending-record tests](../../../../block-storage/src/rust/deploy/pending_deploy/tests.rs) check fixed historical digests, exact re-encoding, identity corruption, signature corruption, and lossy-adapter rejection.
Generated restoration sequences preserve the signed body, identity, and protocol gate across repeated deserialization.
Legacy wire tests cover every three-member permutation and generated permutations with up to 32 members, including duplicate-signer rejection.
The historical processed decoder acts as a differential reference for the reversed additional-signer regression.
Separate differential fixtures preserve historical flat authorization when an algebra field is invalid or specifies a different threshold.
An ingress fixture verifies algebra-derived members when flat cosigner hints omit those members.
These tests do not prove all thread schedules or complete crash recovery across the deployment lifecycle.
The Rocq proofs do not establish Rust refinement, cryptographic security, LMDB durability, or host-memory safety.

## Price semantics and dev alignment

The integration contract preserves dev's `phloPrice` as the signed offered price.
Owner ceilings remain separate signed funding terms.
The selected schedule must charge exactly the offered price, not any lower price that the ceilings permit.
The on-chain policy supplies the minimum price, not a second node-local configuration value.

The offered envelope restores and signs the scalar wire fields.
The older funded-v1 ceiling record does not establish an offered-price signature.
Both dedicated decoders remain separate from live admission until native validation satisfies this contract.
The offered constructor validates representation, not resource sufficiency or equality with the selected schedule.

For a draw, all prices must use the same asset and phlo units.
Admission must enforce:

```text
actual schedule price = signed offered phloPrice
on-chain minimum <= signed offered phloPrice <= each required owner's ceiling
signed phloLimit = funding execution limit
```

The generic reference checker retains the minimum in `CheckedPhloControls`.
`check_offered_phlo_controls` additionally checks the offered price and limit, then returns an immutable `CheckedPhloOffer`.
`CheckedPhloControls::bind_offer` refines existing checked controls without reconstructing their numeric evidence or changing their minimum price.
The [formal contract](signed-phlo-formal-contract.md#decoded-schedule-composition) defines its proofs and tests.
Live admission must still obtain and authenticate the applicable chain policy before calling this checker.

`PhloFundingIntentView::check_offered_signed_family` checks the member limit, exact funding record, signatures, family consent, and funding-right terms.
It then binds the offered scalars to every prepared outcome's checked controls.
A cheaper permitted schedule does not satisfy a different signed offer.
The funding family requires identical checked controls across all outcomes, including uncharged outcomes.

### Adopted chain context

`bind_native_family_from_genesis` obtains the required policy from a loaded [genesis resource policy](genesis-resource-policy.md).
The loaded value also supplies the authoritative minimum from that same genesis root.
The binding rejects an adopted minimum from another context before funding-policy planning.
The lower-level helper below checks policy equality but leaves policy provenance to its caller.

`DirectWalletPolicySnapshot<OfferedFundedDeploy>::bind_native_family` requires the adopted `CasperShardConf` and the required `PhloSchedulePolicy`.
The binding rejects a captured minimum, schedule protocol version, or schedule shard that differs from this context.
The binding also rejects negative adopted minima, negative protocol versions, and empty shard names.
The selected descriptor must match the required resource policy, including all environment fields, ordered classes, weights, and interpretation rules.
The separate offered-price check still requires the descriptor's actual price to equal the signed offer.
These checks run before funding-policy planning.
They do not change voting, finality, or the signed offered price.

The family constructor requires identical checked controls across every outcome.
The binding therefore checks the first outcome without repeating the comparison for every outcome.
The constructor rejects an empty family.
An uncharged outcome must retain the same context as a charged outcome.

Callers must supply the shard configuration after genesis parameter adoption.
Callers must obtain the required resource policy from authenticated execution policy, not from the submitted funding record.
The arguments do not authenticate their own origin.
The [schedule-policy contract](price-schedules-and-denominations.md#resource-policy-and-offered-price) separates exact content comparison from that state-authentication requirement.
The older `FundedDeploy` helper does not establish this offered-envelope contract.

The [Rocq context model](../../../../formal/rocq/cost_accounted_rho/theories/AdoptedPhloContext.v) proves exact matching and rejection of altered context fields.
It also proves the offered-price bounds and extends the context check to an arbitrary family with shared controls.
The model uses natural numbers for versions and minima, and abstract identifiers for shards.
Rust additionally checks signed-integer conversion and compares exact shard bytes.
Property tests compare the Rust result with the model's equalities across generated values.
Family property tests cover mixed minima, charged and uncharged outcomes, empty families, and reversed case order.
Integration tests obtain the minimum from genesis state and check rejection of each mismatched context field before native settlement.
They also reject changed resource weights, measurement rules, and compatibility rules while retaining the same signed envelope and wallet snapshot.
These results do not establish complete production-path correctness or formal refinement of the Rust implementation.

### Normalizer context

`normalizer_env_from_envelope` builds the normalizer environment directly from the retained envelope.
Funded formats use their original envelope commitment for both deploy-identity names.
The helper does not convert funded signatures into body-only signatures.
It uses the existing authority projection for selected signers, policy members, and threshold.
Unsigned policy members remain visible as members, but they do not become selected signers or funding authorities.

Changed signed funding terms change the deploy commitment under the existing cryptographic assumptions.
Unchanged selected principals retain the same authority identity even when the funding terms change.
The full signed envelope remains available to admission and settlement.
The normalizer environment is not a replacement for that envelope or evidence of sufficient funding.

Legacy records retain their original primary signature and deployer identity.
Canonical signer order does not replace a stored primary identity.
The existing body-only normalizer entry point retains its output and shares the same internal context construction.
The legacy path no longer clones the deploy body to obtain its signer metadata.

The envelope model separates the retained deploy commitment from a parameterized authority projection.
Its context lemmas prove preservation of those projections, not the cryptographic implementation or the complete interpreter.
Tests cover each envelope format, wire round trips, threshold placeholders, legacy primary order, and changed offered-price or limit fields.
The funded context helper does not enable live admission or change the compatibility policy.

### System-process metadata and random names

`SystemProcessDeployData::from_envelope` preserves the deploy timestamp, original commitment, and selected authority.
One selected signer produces a principal identity.
Multiple selected signers produce the existing compound authority identity.
Unsigned threshold members do not change that selected authority.
The system-process metadata and normalizer environment must identify the same deploy and authority.
Both retained-envelope helpers reuse the checked commitment without hashing the full funding payload again.

`Tools::user_envelope_rng` derives the bound-format random seed from the existing domain prefix and the original envelope commitment.
Changing the signed offer or limit changes that commitment under the existing cryptographic assumptions.
A wire round trip must preserve the random generator state.
Legacy records retain the existing public-key and timestamp seed, using the stored original primary signer.
Both older entry points share their internal implementations with the retained-envelope helpers.

The envelope model proves that fixed-prefix bound seeds preserve and distinguish committed identities.
It also proves the exact seed length.
These lemmas concern seed construction, not cryptographic collision resistance or random-generator security.
Cross-crate tests compare the metadata, normalizer environment, and random seed across all supported envelope representations.
The tests include threshold placeholders, noncanonical legacy primary order, generated offers, and wire round trips.
Generated negative timestamps must fail payload construction before the helpers can create runtime context.
These context tests do not replace interpreter, native settlement, or validator replay tests.

### Interpreter processing boundary

The user-deploy processor retains a `DeployEnvelope` throughout evaluation and result construction.
Historical body-only callers construct that envelope before the existing soft checkpoint.
The processor does not reconstruct body-only signatures from funded signatures.

`RuntimeDeployRef` selects the existing context adapters without copying the source term.
The retained envelope supplies system-process metadata, normalizer bindings, and the random seed.
The meter receives the original bound commitment and the authority of the selected signers.
Unsigned threshold members cannot become funding authorities.
Historical signatures retain their existing meter initialization.

The shared processor returns the complete `EvaluateResult` with the processed deploy.
This result includes raw byte measurements, authority events, execution errors, and mergeable-channel changes.
Native settlement must use these measurements with the authenticated resource policy.
It must not reconstruct raw quantities from an already weighted cost.
Historical callers retain their existing return values without cloning the complete evaluation result.

Evaluation errors, invalid authority traces, and unresolved resource births retain the existing rollback behavior.
The processed deploy retains its original signed envelope on success and failure.
Failure measurements remain available after state rollback.
These measurements do not authorize payment by themselves.

Interpreter tests compare historical processing against the body-only evaluator, including costs, event logs, generated names, and resulting state roots.
Funded tests check changed commitments, selected signers, threshold placeholders, wire round trips, exhaustion, and isolated concurrent runtime instances.
Generated cases compare independent executions of an envelope and its decoded representation.
This comparison is not a validator replay test.

The interpreter fixtures test envelope transport, not funding sufficiency.
Their records omit economic sources and eligible schedules and must not pass native funding admission.
The runtime boundary does not activate funded network submission, proposal selection, or settlement.
Those paths still require an authenticated policy, a sufficient funding proof, and replay verification.

### Recorded-trace evaluation

The replay evaluator accepts the retained envelope when its caller supplies an execution budget and an authority allocation.
It uses the same bound commitment, selected signers, normalizer bindings, and random seed as play.
The optional host-work budget remains separate from the economic budget.
The evaluator does not derive funding permission from successful decoding.

Historical legacy replay retains its existing body-context projection.
This preserves the context previously obtained through `ProcessedDeploy::to_cosigned` without cloning the body.
Bound formats use their retained context directly.
The genesis path still rejects funded envelopes instead of treating them as system initialization.

Recorded-trace tests execute a COMM in play and replay its recorded tuple-space events with a replay runtime.
They compare costs, realized authority demand, raw byte measurements, meter identity, selected authority, and resulting state roots.
Example cases and generated cohorts include unsigned threshold members.
A negative test changes signed funding terms while retaining the recorded trace and requires replay rejection.

These tests validate the interpreter boundary, not complete block replay or economic admission.
Ordinary block replay still requires its funding certificate and authenticated state snapshot.
Its current body-only settlement path cannot yet verify funded envelopes.
Tests require funded genesis rejection and missing-certificate rejection to preserve these activation boundaries.

The checked intent, family policy, native policy, and scoped capture retain the concrete envelope type.
Each scoped capture retains an immutable reference to the original authenticated envelope.
Native amount conversion and observed-outcome matching preserve that reference.
The generic policy methods cannot create a checked authorization because its constructor fields remain private.
Only the concrete envelope checkers create these authorizations.

The family tests compare both envelope formats across cohorts of one, two, three, four, 64, 65, and 129 purses.
They check equal allocation, original-custody refunds, cursor transitions, signature mutations, and rejection of resigned but inconsistent offers.
These helpers do not publish SystemVault state or activate a network format.

Checking only the ceiling against the minimum is insufficient.
The solver must use an eligible schedule when it establishes the funding bound.
Settlement must use the same captured schedule and authorized terms.
Admission and replay must use the same applicable on-chain policy.

This rule does not make price ceilings allocation weights or change Casper transaction selection.
Offered-price equality preserves upstream charging semantics while separate ceilings preserve owner consent.
Historical execution must retain the offered-price semantics selected by its historical format and execution policy.
The existing v6.1 vector checks do not establish complete replay compatibility with dev's older price-bearing deploy format.
That compatibility requires its own historical decoder and execution evidence.

For example, a minimum of two, an offer of three, and owner ceilings of four and five permit price three.
An explicitly permitted schedule at price two still fails because it differs from the signed offer.
Six newly funded phlo cost 18 units before the separately specified deployment fee.
Prepaid resource rules and canonical wallet allocation remain independent requirements.

## Formal properties and tests

`SignedPhloIntentWire.v` proves the following payload properties under explicit length-representation bounds:

| Property | Native test obligation |
| --- | --- |
| Equal funded payloads contain equal domains, versions, body bytes, and funding bytes | Compare native output with independent length framing |
| Funded payloads cannot alias existing v6.1 intents | Test distinct prefixes and signature-transplant rejection |
| Different funding bytes produce different payloads | Mutate every funding-byte position and generate varied funding records |
| Canonical funding encoding binds all record terms | Retain the funding codec's structural and field-mutation tests |
| Equal funded payloads bind equal bodies and decoded funding records | Check signed records through native family validation and reject record substitution |

The proofs concern canonical preimages.
They do not prove cryptographic collision resistance or signature unforgeability.
Those security properties remain assumptions of the selected cryptographic primitives.

Native regression tests also cover protobuf round trips, byte limits, truncation, mixed formats, malformed commitments, and selected-witness preservation.
Signature-bound family tests include threshold policies and funding cohorts with 1, 2, 3, 4, 64, 65, and 129 entries.
These larger cases use an explicitly increased member cap, not an exception to the configured cap.
The tests retain canonical capture checks for accepted and platform-failed outcomes, original refunds, and reordered source presentations.
The fixed `Nil` fixture records the Blake2b-256 digests `8e5fe94cf45bd1556d96fbe29553a9986472f6f3f37cfb3a91fbc5fc7e99e179` for the complete protobuf and `25753d4b0dfbecbc129ea1926fa8980a484a8bb9403cbf717ddfa366d3151ea8` for the funding intent.
Existing v6.1 golden vectors remain a separate compatibility check.
These pure encoding operations do not share mutable runtime state.
Runtime concurrency and atomic settlement require separate model, property, and interleaving tests at their implementation boundaries.

## Related contracts

- [Existing deploy envelope](deploy-envelope-v6-1.md)
- [Signed phlo formal contract](signed-phlo-formal-contract.md)
- [Signed price consent](signed-price-consent.md)
- [Authority and custody identity](authority-custody-identity-contract.md)
- [Price transitions and replay](price-transitions-and-replay.md)
