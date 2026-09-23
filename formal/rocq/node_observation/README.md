# Node observation construction proofs

This project proves challenge-allocation properties for one observer lifetime. It does not discharge the combined node observation claim.

## Correspondence

| Definition | Rust boundary | Scope |
| --- | --- | --- |
| `allocate` | Hello sequence allocation in `Observer::session`. | The next event sequence distinguishes challenges even when random values repeat. |
| `advance` | Response sequence allocation in `Observer::event`. | Responses can consume sequence values between handshakes. |
| `Rejected` | A rejected request or disconnected session. | No response event is required for freshness. |
| `matches` | Complete challenge equality in request validation. | Both the random component and sequence must match. |
| `checked_allocate` | `checked_add(1)` in `Observer::event`. | Counter exhaustion refuses allocation rather than wrapping. |

The model uses natural numbers for event sequences and random values. It proves the properties for arbitrary finite event histories without a fixed history bound.

Rust uses a checked `u64` counter. The refusal theorem covers the boundary that prevents machine arithmetic from invalidating the natural-number model.

The proof treats tokens as pairs. Rust formatting must preserve pair identity, and Rust equality must compare the complete encoded token.

This correspondence depends on UUID formatting, decimal formatting, and string equality. No theorem claims that equal hashes imply equal source artifacts.

## Exported theorems

| Theorem | Result |
| --- | --- |
| `observer_challenges_unique` | Every modeled history contains distinct issued tokens. |
| `observer_replay_refused` | A prior token cannot match the next allocated token. |
| `observer_counter_exhaustion_refused` | Allocation at or above the counter bound returns no state. |
| `observer_checked_allocation_valid` | Successful checked allocation preserves uniqueness and the history bound. |

`MainTheorem` exports all four results. Each result must report `Closed under the global context` after compilation.

No custom axiom, admission, or parameter is permitted in the trust base. Conditional library correspondence is not a kernel proof of those libraries.

## Verification

Run these commands in the resource-limited verifier container:

```bash
coq_makefile -f _CoqProject -o Makefile
make -j1
coqchk -Q theories NodeObservation NodeObservation.MainTheorem
```

The formal gate separately prints the assumptions of each exported theorem. Build success alone does not establish a closed assumption set.

The [TLA+ area](../../tlaplus/node_observation/README.md) records the bounded counterexample and the Rust binding tests.

Cross-incarnation uniqueness, the other Batch A properties, and all Batch B1 properties remain pending. Named maintainer review also remains pending.
