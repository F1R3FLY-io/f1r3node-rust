# Protocol-v6.1 deploy identity and authority

This document defines the consensus wire contract for protocol-v6 deploys.
Protocol-v6.1 binds a deployment's exact program intent, authorization policy,
and selected signer subset into one `DeployIdV6`. It then derives Rholang
authority and RevVault funding only from signers who supplied valid witnesses.
The design is a concrete F1R3node refinement of the signature-indexed resource
semantics in [*Cost-Accounted Rho Calculus*][cost-rho] and the continued GSLT
cost construction in [*Continued Interactive GSLTs and the Cost
Endofunctor*][continued-gslt].

Protocol-v6.1 is a fresh-genesis protocol. A node must never reinterpret a
legacy deploy, an earlier draft-v6 envelope, or an existing database as v6.1.

## Terms

| Term | Definition |
| --- | --- |
| **principal** | A pair `(signature scheme, canonical public key)` that can verify one witness. |
| **ground authority** | Stable custody identity derived from a key family and canonical public key. The native and Ethereum secp256k1 schemes share a ground authority for the same key. |
| **policy member** | One principal in the canonically ordered authorization policy. |
| **selected member** | A policy member whose bit is set in the presence bitmap and whose witness is present and valid. |
| **funding authority** | The canonical composition of selected members' distinct ground authorities. Only this authority can fund or authorize the deployment. |
| **intent** | Canonical bytes for the language, source term, time window, shard, and authority presentations. |
| **`DeployIdV6`** | The Blake2b-256 commitment to the intent, complete policy, and exact selected-member bitmap. |
| **witness** | A canonical signature by one selected principal over the scheme-separated v6.1 signing message. |

The separation between principal and ground authority is essential. Signature
schemes identify verification rules; ground authorities identify reusable
wallet custody. Treating those two identities as interchangeable either makes
wallet identity unstable or permits the same wallet to appear twice in one
policy.

## Why the selected subset is committed

A scalar threshold and a member list do not identify a state transition. For
example, a two-of-three policy over members `A`, `B`, and `C` admits the
subsets `{A,B}`, `{A,C}`, and `{B,C}`. Those subsets authorize and debit
different purses. If all three shared one deployment identifier, two valid
envelopes could alias in deploy storage while producing different state.

Protocol-v6.1 therefore commits the presence bitmap. For fixed intent `I` and
policy `P`, changing selected subset `S` changes the commitment preimage and
thus the deployment identity:

```math
S_1 \ne S_2 \Longrightarrow
\operatorname{preimage}(I,P,S_1) \ne
\operatorname{preimage}(I,P,S_2).
```

This is a semantic requirement, not a user-interface choice. Deploy storage,
duplicate detection, occurrence indexing, replay evidence, lifecycle records,
and APIs all use the resulting typed identity.

## Wire schema

`DeployDataProto` uses these protocol-v6.1 fields:

| Field | Tag | Requirement |
| --- | ---: | --- |
| `authorityPresentations` | 18 | Canonical ordered list included in the intent. |
| `deployId` | 19 | Exactly 32 bytes and equal to the independently recomputed `DeployIdV6`. |
| `authorizationV61` | 20 | Required v6.1 policy, bitmap, and witnesses. |

The legacy authorization fields `deployer`, `sig`, `sigAlgorithm`,
`cosigners`, `cosigner_threshold`, and `sig_algebra` must be absent or their
protobuf defaults. Mixed legacy/v6.1 authorization is rejected. The
`authorizationV61.formatVersion` value is exactly hexadecimal `00060001`.

The block store's partial decoder follows the enclosing block protocol
version rather than byte length. Protocol v6 and later read the 32-byte
`DeployDataProto.deployId` and `RejectedDeployProto.deployIdV6` fields and
require their legacy `sig` fields to be empty. Earlier versions read only the
legacy `sig` fields and require the v6 identity fields to be empty. Duplicate
scans, scope construction, rejection disposition, and their shared cache all
consume this selected identity. Missing, mixed, cross-version, or wrong-width
identities fail block decoding instead of becoming a negative duplicate result.

`AuthorizationPolicyV61` contains exactly one of:

- `AllOfPolicyV61`, where every member is selected; or
- `ThresholdPolicyV61`, where $`1 \leq k < N`$ and at least `k` of `N`
  members are selected.

An `N`-of-`N` threshold is non-canonical and must be encoded as `AllOf`.

## Canonical binary primitives

All integer encodings are unsigned big-endian. `U8`, `U16BE`, `U32BE`, and
`U64BE` denote fixed-width encodings. `LP64(x)` is:

```math
\operatorname{LP64}(x)=\operatorname{U64BE}(|x|)\mathbin\|x.
```

`UTF8(x)` is the exact UTF-8 byte sequence supplied by the client. Validators
do not apply Unicode normalization. Protobuf serialization is not a consensus
commitment preimage; the canonical encodings below are.

## Signature schemes and key families

| Scheme ID | Name | Key family | Public key | Signature | Activation |
| ---: | --- | ---: | --- | --- | --- |
| 0 | unspecified | — | — | — | Always rejected. |
| 1 | `secp256k1` | 1 | 65-byte uncompressed SEC1 | Strict DER ECDSA | Active. |
| 2 | `secp256k1:eth` | 1 | 65-byte uncompressed SEC1 | 64-byte `r || s` ECDSA | Active. |
| 3 | Schnorr secp256k1 | 2 | 32-byte x-only | 64-byte BIP-340 | Allocated, inactive. |
| 4 | FROST secp256k1 | 2 | 32-byte x-only | 64-byte BIP-340 aggregate | Allocated, inactive. |
| 5 | Ed25519 | 3 | 32 bytes | 64 bytes | Allocated, inactive. |

The consensus allowlist is exactly `{1,2}` and is independent of cargo feature
flags. Compiling experimental cryptography cannot activate it for deploys.

For schemes 1 and 2, a public key must parse as a finite secp256k1 point and
re-encode byte-for-byte as the 65-byte uncompressed SEC1 form. Compressed,
hybrid, off-curve, infinity, and non-canonical encodings reject.

A scheme-1 signature must be a minimally encoded DER pair with both scalars in
range and low `s`; parsing and re-encoding must reproduce the input exactly. A
scheme-2 signature is exactly 64 bytes, parses as `r || s`, has in-range
scalars, and has low `s`. Validators reject high-`s` inputs instead of
normalizing them.

## Principal and custody encodings

For scheme `s`, key family `f`, and canonical key `K`:

```math
\begin{aligned}
\operatorname{Principal}(s,K)
  &= \operatorname{U16BE}(s)\mathbin\|
     \operatorname{U32BE}(|K|)\mathbin\|K,\\
\operatorname{Ground}(f,K)
  &= \operatorname{U16BE}(f)\mathbin\|
     \operatorname{U32BE}(|K|)\mathbin\|K.
\end{aligned}
```

Policy members are strictly increasing by `Principal` bytes. Duplicate
principals reject. Duplicate `Ground` bytes also reject, including the same
secp256k1 key listed once under scheme 1 and once under scheme 2. This prevents
one custody purse from satisfying two nominal policy slots.

### Blessed genesis identity and custody projection

Fresh protocol-6 genesis constructs every blessed deployment as a complete
protocol envelope before it enters occurrence indexing, normalization,
execution, or replay. The occurrence identity is its `DeployIdV6`; construction
and replay receive the same cosigned envelope rather than reconstructing a
legacy blessed signature.

The Rholang deployer identity is `GPrincipalId(keyFamily, publicKey)`. Existing
SystemVault contracts address secp256k1 custody by the corresponding ground
public key. The native custody projection therefore accepts exactly a family-1
principal and returns that canonical ground key. It also accepts the historical
`GDeployerId` only when replaying historical protocol data. Other principal
families, malformed keys, and compound identities do not project to one purse
and fail closed.

This projection does not weaken protocol identity. Occurrence records, replay
evidence, RNG seeds, reservation identities, and signer verification remain
bound to the complete v6 envelope. Only the contract-facing lookup key is
projected to the stable ground-custody representation shared by the native and
Ethereum secp256k1 schemes.

The cost-accounting funding ground carries the principal as
`U16BE(keyFamily) || U32BE(keyLength) || publicKey`. The payer resolver computes
the accounting lane from those complete bytes before applying the following
custody-only projection:

```text
projectFundingGround(bytes):
    if bytes is one canonical uncompressed secp256k1 key:
        return NativeVault(bytes)
    require bytes contains U16BE(1) and one U32BE length
    require the declared length equals the exact remaining byte count
    require the remaining bytes are one canonical uncompressed secp256k1 key
    return NativeVault(remaining bytes)
```

The first branch preserves historical ground-custody replay. The second branch
realizes the protocol-v6 family-1 projection. A different family, truncated or
extended encoding, false length, compressed key, or invalid curve point cannot
alias native custody. Such input remains on its distinct typed accounting lane
and has no authority over the public-key purse.

This distinction closes an implementation-refinement defect found by the
protocol-v6 state-bound admission regression. The formal lifecycle model and
Rocq theorem already required family-1 projection, but the Rust payer resolver
recognized only the historical raw-key representation. Earlier fixtures also
constructed legacy signatures inside protocol-v6 blocks, so they could not
exercise the missing projection. The regression now constructs an authenticated
protocol envelope, draws from a genesis-funded SystemVault purse, and checks the
same typed occurrence through proposal and replay.

## Canonical policy

Let `members` be the strictly ordered principal encodings and `N` their count.
The canonical policy bytes are:

```text
AllOf:     U8(1) || U32BE(N) || members...
Threshold: U8(2) || U32BE(k) || U32BE(N) || members...
```

`AllOf` requires $`N > 0`$. `Threshold` requires
$`1 \leq k < N`$. Every member must use an active scheme and a canonical key.

## Presence bitmap and witnesses

The presence bitmap contains `ceil(N / 8)` bytes. Bit `i` is the
least-significant-first bit `i % 8` of byte `i / 8`. Unused high bits in the
last byte are zero.

- `AllOf` requires every member bit.
- `Threshold(k,N)` requires a population count of at least `k`.
- Witnesses are strictly increasing by `memberIndex`.
- Witness indexes equal the set bits exactly.
- Every selected member has one nonempty canonical valid signature.
- An unselected member has no witness and contributes no authority or funding.

The policy describes who may participate; the bitmap and witnesses describe
who actually authorized this deployment.

## Canonical intent

The v6.1 intent is:

```text
U16BE(1)
|| U8(1)                              // Rholang language discriminator
|| LP64(UTF8(term))
|| U64BE(timestamp)
|| U64BE(validAfterBlockNumber)
|| LP64(UTF8(shardId))
|| expiration
|| U32BE(authorityPresentationCount)
|| LP64(canonicalAuthorityPresentation[0])
|| ...
```

The outer `language` field must be exactly `rholang`. Timestamp and valid-after
must be nonnegative. `shardId` must be nonempty. Expiration is `U8(0)` when
absent, or `U8(1) || U64BE(value)` for a positive timestamp.

### Authority presentations

Authority presentations are strictly ordered and unique by their canonical
bytes. Their recursive encoding is:

| Form | Encoding |
| --- | --- |
| Unit | `U8(0)` |
| Ground bytes `b` | `U8(1) || LP64(b)` |
| Quote `P` | `U8(2) || LP64(canonicalPar(P))` |
| Name `P` | `U8(3) || LP64(canonicalPar(P))` |
| Compound children | `U8(4) || U32BE(count) || LP64(child[0]) || ...` |

Quoted and named `Par` values must already equal the canonical sorted form.
Compounds are flattened, units are removed, and children are sorted while
preserving multiplicity. Zero remaining children canonicalize to Unit; one
canonicalizes to that child; two or more use Compound. Unresolved bound levels
and false units reject.

## Deployment identity

Let `I` be canonical intent bytes, `P` canonical policy bytes, and `B` the
presence bitmap. The commitment preimage is:

```text
LP64("f1r3fly:casper:deploy-envelope:v6.1")
|| U16BE(6)
|| LP64(I)
|| LP64(P)
|| U32BE(len(B))
|| B
```

The deployment identity is:

```math
\operatorname{DeployIdV6}=
\operatorname{Blake2b256}(\operatorname{preimage}).
```

The wire `deployId` is checked against this value. No identifier is inferred
from byte length, a primary signature, a timestamp, or a payload hash.

## Signing messages

For scheme ID `s` and deployment identity `D`, construct:

```text
LP64("f1r3fly:casper:deploy-envelope-signature:v6.1")
|| U16BE(6)
|| U16BE(s)
|| D
```

Scheme 1 signs the Blake2b-256 digest of these bytes. Scheme 2 applies the
Ethereum personal-message prefix to these exact bytes and signs the resulting
Keccak-256 digest. Each verifier derives the scheme from the policy member;
algorithm probing and fallback verification are forbidden.

## Rholang authority projection

Every selected principal maps to its `Ground` bytes. The selected ground
authorities are sorted. A singleton becomes one ground signature; multiple
authorities become one canonical flattened compound signature. Threshold is
an admission rule only: it does not create a shared threshold purse and does
not authorize unsigned members.

The compound authority identifier is:

```text
Blake2b256(
  LP64("f1r3fly:rholang:compound-authority:v6.1")
  || U32BE(selectedCount)
  || LP64(selectedGround[0])
  || ...
)
```

The normalizer exposes:

- `rho:system:deployId` as `GDeployId(DeployIdV6)`;
- `rho:system:authorityId` as `GAuthorityId(compoundAuthorityId)`;
- signer and policy-member introspection as inert tuples containing scheme ID,
  key family, and canonical public key;
- `rho:system:cosigners` as an alias for selected signers; and
- `rho:system:deployerId` only for a singleton selected authority, represented
  by `GPrincipalId(keyFamily, publicKey)`.

There is no privileged “member zero” for a compound deployment. A compound
authority ID is an identifier, not a component SystemVault authorization
capability, and must never be expanded into access to its member purses.

## RevVault funding and settlement

Each selected ground authority names the same reusable RevVault purse across
deployments. State-bound admission freezes authenticated pre-state capacity
for exactly those selected purses. Unsigned policy members contribute neither
supply nor debit capacity.

For selected payer lane `a`, physical bound $`B_A(a)`$, quantitative-byte
bound $`B_Q(a)`$, fee $`F(a)`$, and authenticated supply $`\Sigma(a)`$:

```math
B_A(a)+B_Q(a)+F(a)\leq\Sigma(a).
```

Settlement applies the replay-checked realized physical cost $`\kappa(a)`$,
realized quantitative-byte cost $`Q(a)`$, and fee:

```math
\Sigma'(a)=\Sigma(a)-\kappa(a)-Q(a)-F(a).
```

The operation is one atomic SystemVault transition. The policy cannot debit an
unselected member, one payer cannot rescue an underfunded located purse, and a
concurrent top-up cannot expand an already certified reservation snapshot.

## Ingress algorithm

The following literate pseudocode defines the fail-closed validation order.
Each step produces typed data consumed by the next step; no later subsystem
reinterprets raw protobuf fields.

```text
validate_v61(proto):
    require protocol version == 6
    require legacy authorization fields are absent/default
    require authorizationV61.formatVersion == 0x00060001

    intent  := canonical_intent(proto)
    policy  := canonical_policy(proto.authorizationV61.policy)
    bitmap  := validate_bitmap(policy, proto.authorizationV61.presenceBitmap)
    members := validate_principals(policy.members)
    require distinct ground authorities(members)
    require witnesses exactly index bitmap set bits

    deploy_id := blake2b256(commitment_preimage(intent, policy, bitmap))
    require proto.deployId == deploy_id

    for witness in witnesses:
        principal := members[witness.memberIndex]
        require canonical_signature(principal.scheme, witness.signature)
        require verify(principal, signing_message(principal.scheme, deploy_id), witness)

    selected := members selected by bitmap
    require policy quorum is satisfied by selected
    authority := canonical_ground_projection(selected)
    return VerifiedEnvelopeV61(deploy_id, intent, policy, selected, authority)
```

Ingress stores the complete verified envelope under `DeployIdV6`. It never
stores only a primary signer or reconstructs a v6.1 envelope from legacy
fields.

### API encodings

The gRPC deployment endpoint accepts the exact `DeployDataProto` v6.1 wire
shape above. The HTTP deployment endpoint is a JSON projection of the same
policy: `deployer` is member zero, `cosigners` contains the remaining policy
members, and an optional `threshold` selects Threshold policy. Omitting
`threshold` encodes AllOf. An empty member signature means that member is not
selected; it is not an anonymous or deferred witness.

HTTP signatures are already signatures of the scheme-bound v6.1 envelope
message. The node reconstructs the canonical principal order, bitmap, policy,
and `DeployIdV6`, then performs the same verifier used by protobuf ingress.
The HTTP response returns that computed deployment ID. Payload-signed legacy
HTTP requests fail at the authoritative Casper admission boundary under
protocol 6.

## Block admission, replay, and publication

For each certified block, validators derive an authenticated admission context
from the block and frozen DAG. They recompute the complete ordered admitted and
rejected partition by typed `DeployIdV6`, execute only admitted deployments,
and validate every resulting state root, cost, causal log, authority
allocation, settlement witness, and terminal close.

The typed identity is retained through every downstream derivation. Evidence
queues, vault reservations, fee region scopes, fee event identities, and the
user-deploy private-name generator are keyed by `DeployIdV6`; none reads a
member signature. Every supplied evidence entry must be consumed exactly once.
Missing, duplicate, substituted, or leftover evidence rejects the block.

The protocol-v6 reservation commitment is:

```text
Blake2b256(
    "f1r3node:vault-cost-reservation:v2" ||
    pre_state_root ||
    canonical_program_hash ||
    DeployIdV6
)
```

All three suffix fields are fixed 32-byte values. Replay recomputes the
canonical program hash and reservation before vault or RSpace mutation. The
fee region scope is Blake2b-256 of
`f1r3node:cost-accounted-rho:deploy-scope:v1 || DeployIdV6`; its event identity
is Blake2b-256 of
`f1r3node:cost-accounted-rho:fee-event:v3 || DeployIdV6`. The private-name
random-number generator (RNG) is seeded from
`f1r3node:user-deploy-unforgeable:v6 || DeployIdV6` on both proposal and replay.

Pre-v6 reservations, fee identities, and private-name seeds retain their exact
historical byte encodings. Protocol selection is structural and validated at
ingress; byte length never chooses a compatibility branch.

Protocol 6 derives each private-name stream from `DeployIdV6`. A legacy
key-and-timestamp preview cannot predict this stream and must fail closed.
The source term is part of `DeployIdV6`, so a name-dependent term creates a
circular identity. Applications can publish the capability in one deploy and
use dependent data in a later deploy.

A future one-deploy preview design needs an authenticated seed or reserved
identity. That design must prove uniqueness, replay agreement, and collision
rejection before activation.

Replay evidence becomes visible only after the complete result is validated
and durably inserted. The insertion is compare-and-swap:

- absent row: insert the complete locally derived evidence;
- identical row: accept as an idempotent retry;
- different row: fail closed without overwrite.

Cache publication follows durable insertion. Peer-provided bytes, a bare
database row, matching counts, a primary signature, or an empty expected
rejection set are never sufficient evidence. Crash recovery exposes either no
row or one complete validated row.

Deploy occurrences are keyed by `(DeployIdV6, source block hash)`. Arrival
order may change neither the exact occurrence set nor its deterministic
canonical representative. Lifecycle finalization and occurrence compaction
are one atomic storage transaction, so a terminal deploy cannot reopen behind
an independently committed occurrence index.

The pending-deploy store persists the complete envelope, including the ordered
policy members, selected-member bitmap implied by signature presence, quorum,
and every witness. Opening the store reconstructs and cryptographically
validates each envelope before the pool can serve proposal work. Startup
rejects noncanonical member order, an invalid selected signature, an
unsatisfied quorum, a legacy signing domain, or a key that differs from the
recomputed envelope commitment. A valid threshold envelope therefore survives
submit, close, reopen, proposal conversion, and `ProcessedDeploy` round-trip
without a primary-signer downgrade.

## Activation and migration

Protocol-v6.1 requires a fresh-genesis activation marker and empty v6.1 deploy,
occurrence, lifecycle, and replay-evidence stores. Startup rejects:

- an absent activation marker with nonempty protocol-v6 storage;
- an incompatible schema or protocol marker;
- a mixture of legacy and v6.1 deploy rows;
- a stored envelope with invalid signer order, signature, quorum, signing
  domain, or key-to-commitment binding; or
- a partial occurrence/lifecycle transaction.

There is no compatibility decoder for an earlier draft-v6 envelope. If a
public network ever accepts another protocol-6 meaning, this format must move
to a new protocol number rather than reinterpret committed bytes.

## Required negative cases

Conformance suites must reject at least these classes:

- unsupported or unspecified schemes;
- malformed, compressed, hybrid, infinity, or off-curve secp256k1 keys;
- nonminimal DER, wrong fixed width, zero/out-of-range scalar, or high-`s`
  signatures;
- duplicate principal or duplicate ground authority;
- empty `AllOf`, invalid threshold, or `Threshold(N,N)`;
- wrong bitmap length, nonzero padding, insufficient population, witness/bit
  mismatch, duplicate or unordered witness indexes;
- selected invalid signature or unselected witness;
- mixed legacy and v6.1 fields;
- noncanonical authority presentation, unresolved bound level, false unit,
  empty shard, negative time, invalid expiration, or non-Rholang language;
- mismatched `deployId` and every mutation of intent, policy, or selected
  subset;
- replay count-only, primary-signature, legacy-wire-field, raw evidence,
  reservation, fee, or RNG identity, unconsumed evidence, caller-context,
  early-publication, peer-byte, bare-row, conflict-overwrite, and cache-first
  defects;
- protocol-v6 startup over legacy, partial, or incompatible persistent state;
  and
- persisted envelope signature or quorum corruption, including corruption
  that leaves the envelope commitment unchanged.

## Verification and executable evidence

| Layer | Artifact | Obligation |
| --- | --- | --- |
| Rocq | `ThresholdEnvelopeAuthority.v` | Exact selected-subset authority and funding, duplicate-ground exclusion, semantic identity binding, and quorum soundness. |
| Rocq | `ReplayAdmissionPublication.v` | Protocol-selected wire identity, mixed/cross-version rejection, injective evidence/reservation/fee/RNG identity, exact evidence consumption, reservation mutation rejection, exact partition, proof-carrying replay artifacts, atomic idempotent publication, and crash behavior. |
| TLA+/TLC/Apalache | `ThresholdEnvelopeAuthority.tla` plus six unsafe controls | Concurrent validator agreement; every unsafe authority/subset projection produces its named counterexample. |
| TLA+/TLC/Apalache | `ReplayAdmissionPublication.tla` plus fifteen unsafe controls | Exact authenticated replay, protocol-selected storage/evidence/reservation/fee/RNG identity, exact evidence consumption, and durable publication under arbitrary validator interleavings; every shortcut is independently refuted. |
| TLA+/TLC | `deploy_recovery/ProtocolVersionLifecycle.tla` plus four genesis-identity controls | Protocol-6 ceremony, occurrence, construction, replay, and family-1 ground-custody projection use one approved protocol envelope; legacy occurrence, execution, replay, and missing custody projection are independently refuted. |
| Rocq | `finalized_floor/theories/ProtocolVersionLifecycle.v` and `MainTheorem.v` | Current blessed execution and replay use the protocol envelope; canonical family-1 funding ground projects to stable legacy custody; unsupported families, false lengths, and noncanonical keys fail closed. |
| Rust examples/properties | crypto, models, normalizer, Casper, and block-storage suites, including `vault_payer`, `physical_rejection_rolls_back_before_later_state_bound_execution`, `threshold_envelope_survives_lmdb_reopen_and_processed_round_trip`, and `lmdb_reopen_rejects_tampered_persisted_envelope_signature` | Canonical bytes, strict cryptography, permutation invariance, typed identity, exact custody projection, malformed-input isolation, funded protocol-v6 proposal/replay, authenticated store restart, transaction atomicity, and occurrence reduction. |
| Loom | occurrence/lifecycle and replay-publication linearization models | Every explored concurrent schedule refines one atomic commit order without lost updates, partial visibility, or terminal reopening. |
| Cross-language vectors | `test-vectors/deploy-envelope-v6.1.json` | Rust, Python, and JavaScript compute identical intent, policy, bitmap, deployment ID, signing message, and verifier outcome. |
| System integration | fresh protocol-v6 network | Submission, proposal, independent replay, duplicate handling, query, restart, finalization, and multi-validator agreement. |

The safe replay-publication TLC instance explores 130,321 distinct states at
depth 37. The threshold-authority instance explores 512 distinct states at
depth 10. Apalache independently checks both safe models and all twenty-one
required unsafe controls.

The `preview_private_names_should_fail_closed_for_protocol_v6` Rust test traces
to the `RawRngIdentity` unsafe control. The control demonstrates why protocol 6
cannot use a legacy preview identity.

## References

- [Cost-Accounted Rho Calculus][cost-rho]
- [Continued Interactive GSLTs and the Cost Endofunctor][continued-gslt]
- [SEC 1: Elliptic Curve Cryptography, version 2.0][sec1]
- [BIP-340: Schnorr Signatures for secp256k1][bip340]
- [Ethereum personal-sign message prefix][eip191]

[cost-rho]: https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting/cost-accounted-rho.tex
[continued-gslt]: https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting-as-monad/continued-gslt-cost-v2.tex
[sec1]: https://www.secg.org/sec1-v2.pdf
[bip340]: https://github.com/bitcoin/bips/blob/master/bip-0340.mediawiki
[eip191]: https://eips.ethereum.org/EIPS/eip-191
