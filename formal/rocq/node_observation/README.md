# Node observation construction proofs

This project proves challenge-allocation, cross-incarnation, and bounded-capture properties. It does not discharge either node observation claim.

## Modules

| Module | Scope |
| --- | --- |
| `ObserverSession` | Challenge allocation, replay refusal, checked counters, and incarnation-qualified tokens. |
| `BoundedCapture` | Transaction clocks, capture observations, insertion generation, charge budgets, length prefixes, and the capture protocol. |
| `MainTheorem` | The 14 exported results. |

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

`MainTheorem` exports all 14 results. Each result must report `Closed under the global context` after compilation.

No custom axiom, admission, or parameter is permitted in the trust base. Conditional library correspondence is not a kernel proof of those libraries.

## Verification

Run these commands in the resource-limited verifier container:

```bash
coq_makefile -f _CoqProject -o Makefile
make -j1
coqchk -Q theories NodeObservation NodeObservation.MainTheorem
```

The formal gate separately prints the assumptions of each exported theorem and requires 14 closed sets. Build success alone does not establish a closed assumption set.

The [TLA+ area](../../tlaplus/node_observation/README.md) contains the canonical session and capture models and the applicability review per property.

The `Begin` action projects to `Hello`. A successful `Reply` projects to `Response`, while `Tick` and `Close` project to `Rejected`. Capture actions project to `capture_step` in TLA+ phase order.

The projection reverses the TLA+ history because `issued` stores the newest token first. Additional admission guards restrict enabled transitions without changing allocation.

These projections are documented source arguments, not machine-checked refinement proofs. The theorems do not prove directory safety, cleanup, canonical identity, scratch independence, or the complete-row predicate.

Named maintainer review of every applicability decision and acceptance of both claims were recorded on 2026-09-23 at revision `4c0c0dbe7`.
