# Node observation construction proofs

This project proves challenge-allocation, cross-incarnation, and bounded-capture properties. It does not discharge either node observation claim.

## Modules

| Module | Scope |
| --- | --- |
| `ObserverSession` | Challenge allocation, replay refusal, checked counters, and incarnation-qualified tokens. |
| `BoundedCapture` | Transaction clocks, capture observations, insertion generation, charge budgets, length prefixes, and the capture protocol. |
| `InterfaceSafety` | Directory and peer admission, identity-sensitive cleanup, awaited shutdown, and deadline admission. |
| `CaptureIntegrity` | Complete rows, logical field framing, and fresh scratch storage. |
| `MainTheorem` | The 25 existing exported results. |
| `b11/` | Eight additional byte-schema results in `NodeObservationB11.MainTheorem`. |

## Correspondence

| Definition | Rust boundary | Scope |
| --- | --- | --- |
| `allocate` | Hello sequence allocation in `Observer::session`. | The next event sequence distinguishes challenges even when random values repeat. |
| `advance` | Response sequence allocation in `Observer::event`. | Responses can consume sequence values between handshakes. |
| `Rejected` | A rejected request or disconnected session. | No response event is required for freshness. |
| `matches` | Complete challenge equality in request validation. | Both the random component and sequence must match. |
| `checked_allocate` | `checked_add(1)` in `Observer::event`. | Counter exhaustion refuses allocation rather than wrapping. |
| `qualify`, `qualified_matches` | The incarnation field checked with the challenge in request validation. | Distinct incarnations never match. Incarnation distinctness is an assumption about UUID generation at observer start. |
| `clock`, `commit_at` | `Env::info().last_txn_id` per environment and every LMDB write commit. | The LMDB transaction identifier is trusted to be monotone. |
| `observation` | `BoundedLmdbReader::open` and `validate` identities per participating environment. | Participants are any finite list. |
| `generation_clock` | `BlockDagKeyValueStorage::current_generation` before and after capture. | The generation is trusted to be monotone. |
| `charge`, `charge_all` | `ReadUsage::charge_record`, `charge_operation`, and `WorkMeter::charge`. | Failure keeps the prior usage. |
| `checked_add` | `usize::checked_add` before each limit comparison. | A machine bound at or above the limit cannot mask a limit failure. |
| `encode`, `decode` | `encode_length_prefixed` and `decode_length_prefixed`. | The prefix is the payload length. Rust uses an eight-byte little-endian `u64`. |
| `capture_step` | The `capture_observed` phases and `BoundedLmdbReader` lifetime. | Three guards, any number of transactions, and release before serialization. |

The proof uses natural numbers for sequences, random values, clocks, and lengths. Rust uses checked `u64` and `usize` arithmetic. The exhaustion and overflow theorems cover the boundaries that prevent machine arithmetic from invalidating the natural-number model.

The correspondence depends on UUID formatting, decimal formatting, string equality, the LMDB transaction rule, and the lock library. No theorem claims that equal hashes imply equal source artifacts.

## Exported theorems

| Theorem | Result |
| --- | --- |
| `observer_challenges_unique` | Every modeled history contains distinct issued tokens. |
| `observer_replay_refused` | A prior token cannot match the next allocated token. |
| `observer_counter_exhaustion_refused` | Allocation at or above the counter bound returns no state. |
| `observer_checked_allocation_valid` | Successful checked allocation preserves uniqueness and the history bound. |
| `observer_cross_incarnation_distinct` | Tokens from distinct incarnations never match. |
| `observer_qualified_replay_refused` | Within one incarnation, a prior qualified token cannot match the next allocated token. |
| `capture_no_interference` | Equal clock observations at before, open, and validation imply no commit on any participant during the interval. |
| `capture_generation_stable` | An equal generation at start and end implies no insertion during the interval. |
| `capture_budget_bounded` | Any successful charge sequence stays within the limit and sums exactly. |
| `capture_overflow_fails_limit` | A checked-add overflow under a bound at or above the limit is also a limit failure. |
| `capture_prefix_roundtrip` | Decoding an encoded payload returns the payload. |
| `capture_prefix_sound` | Any successful decode came from the encoding of its result. |
| `capture_guard_order` | Every reachable capture state holds guards in acquisition order. |
| `capture_detached` | Every reachable released, done, or rejected state holds no guard and no transaction. |

`MainTheorem` exports all 25 results. Each result must report `Closed under the global context` after compilation.

No custom axiom, admission, or parameter is permitted in the trust base. Conditional library correspondence is not a kernel proof of those libraries.

## Verification

Run these commands in the resource-limited verifier container:

```bash
coq_makefile -f _CoqProject -o Makefile
make -j1
coqchk -Q theories NodeObservation NodeObservation.MainTheorem
```

The formal gate requires 25 closed sets from `NodeObservation` and eight from `NodeObservationB11`. Build success alone does not establish a closed assumption set.

The [TLA+ area](../../tlaplus/node_observation/README.md) contains the canonical session and capture models and the applicability review per property.

The `Begin` action projects to `Hello`. A successful `Reply` projects to `Response`, while `Tick` and `Close` project to `Rejected`. Capture actions project to `capture_step` in TLA+ phase order.

The projection reverses the TLA+ history because `issued` stores the newest token first. Additional admission guards restrict enabled transitions without changing allocation.

These projections are documented source arguments, not machine-checked refinement proofs. The additional modules prove conditional boundary properties. The following assumptions define their correspondence.

Named maintainer review of every applicability decision and acceptance of both claims were recorded on 2026-09-23 at revision `4c0c0dbe7`.

## Boundary correspondence

| Definition | Rust boundary | Assumption or limit |
| --- | --- | --- |
| `safe_path` | `safe_directory` examines every ancestor and the final directory. | Kernel metadata is accurate. Trusted owners do not replace path components during admission. |
| `peer_admitted` | `Observer::session` checks UID, PID, and process start ticks. | Linux credentials and process metadata are accurate. Process metadata remains available. |
| `cleanup` | `SocketGuard::drop` compares socket type, device, and inode. | The namespace remains stable between the metadata check and unlink. Unlink errors remain possible. |
| `shutdown_run` | `NodeRuntime::start` returns from `main`, awaits `stop`, then handles exit. | The runtime executes this path. Abort, crashes, and destructor failures are outside this theorem. |
| `write_admitted` | `Observer::write` rejects an expired deadline before output. | Tokio supplies a monotone clock and cooperative task scheduling. This theorem bounds admission, not operating-system latency. |
| `lock_admitted` | All three capture guards receive one absolute deadline. | The lock library honors the deadline. An uncontended lock can succeed after that deadline. |
| `rows_complete` | Capture requires metadata for held blocks and bodies for requested held blocks. | Row predicates represent successful decoding and identity checks. Missing cache seeds remain permitted. |
| `canonical_record` | Logical fields have unambiguous length framing. | The separate B11 project proves injectivity for the complete byte schema. |
| `populate`, `addresses_fresh` | `scratch_view` creates fresh stores and populates captured rows. | Rust allocation, ownership, and store isolation hold. Initialization errors prevent a returned view. |

The canonical reader test decodes the production bytes independently and compares each field with the captured data. Four cases cover body and cache availability.

The reader test does not prove every permitted schema value. The logical framing theorem does not establish SHA-256 collision freedom.

The directory test checks all 4,096 leaf permission values. Separate tests check identity mismatches, expired output, socket identity, and awaited shutdown.

Scratch tests compare mutable allocation identities and check independent metadata, floor, frontier, and block mutations.

The additional exports are `observer_path_admission`, `observer_peer_admission`, `observer_cleanup_preserves_replacement`, `observer_shutdown_before_exit`, and `observer_write_before_deadline`.

Capture adds `capture_lock_deadline`, `capture_complete_metadata`, `capture_complete_requested_bodies`, `capture_canonical_record_injective`, `capture_scratch_preserves_production`, and `capture_scratch_preserves_sibling`.

The Kani prefix harnesses call `length_prefix` and `checked_prefixed_payload`, which the public encode and decode paths use.

The public decoder formats the same errors after those checks. Native tests exercise allocation, formatting, and the complete decoder.

The block preflight harness checks every `usize` length against `check_compressed_length`. Production calls this check before varint decoding or decompression.

Kani verifies the arithmetic boundary. It does not verify allocation, diagnostic formatting, decompression, or the complete block decoder.

## B11 wire correspondence

The [B11 project](b11/README.md) covers every canonical field, integer width, tag, count, and option marker. Its eight exports have closed assumption sets.

The combined binding driver runs both B11 tests and exports 75 production cases. The Rocq CI job verifies those cases and six rejection controls.

The Rocq job requires successful binding execution and retrieves the artifact from the same workflow attempt. The verifier checks source, script, and case digests.

The B11 model proofs apply to arbitrary valid values. The Rust correspondence cases are finite and do not establish universal machine-checked Rust refinement.
