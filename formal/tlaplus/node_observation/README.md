# Node observation verification

This area covers the pending Batch A and Batch B1 claims under TASK-019-4. The user confirmed the implementation scope and mandatory tags.

The current cycle covers challenge allocation and replay within one observer lifetime. It does not establish cross-incarnation uniqueness or complete either claim.

## Model correspondence

| Action or field | Rust boundary | Interpretation |
| --- | --- | --- |
| `Hello` | `Observer::session`, `Observer::event` | A verified peer receives one challenge with a new event sequence. |
| `Request` | The identity comparisons in `Observer::session` | The model selects a request from a current or earlier handshake. Other identity fields remain equal. |
| `Disconnect` | Session return or cancellation | A connection closes without a response. Its challenge sequence remains consumed. |
| `sequence` | The checked `u64` event counter | Successful hello and response events advance the counter. |
| `challenges` | Verification history only | The model retains challenges to state the invariant. Production does not retain this list. |
| `BreakFreshness` | The original random-only challenge | The negative control removes the event sequence from the challenge identity. |

The abstract token contains the random value and the hello sequence. Rust encodes these components with a colon between the UUID and decimal sequence.

Rust request validation compares the complete challenge string. It does not accept a matching random prefix with a different sequence.

Repeated random values are permitted. No randomness-uniqueness assumption is used for this cycle.

The model selects only peers that pass authentication. This restriction isolates the freshness defect and does not verify peer authentication.

The model does not cover filesystem checks, frame parsing, deadlines, capability effects, public serialization, normal shutdown, or storage capture. Those obligations remain pending.

## Configurations

| Configuration | Instance bounds | Expected result |
| --- | --- | --- |
| `MC_ObserverSession.cfg` | Three sessions and two random values. | TLC exits zero with all three invariants preserved. |
| `MC_ObserverSession_freshness_pre_fix.cfg` | The same bounds, with `BreakFreshness = TRUE`. | TLC exits 12 on `FreshChallenges`. |

Both configurations use `ObserverSession.tla`. The gate maps this configuration family to that module without duplicate wrapper modules.

The negative control permits two equal random outputs. Its counterexample concerns an earlier request occurrence, not a new identity inferred from equal signatures.

These bounds belong to the model instance. They do not classify the complete claim as bounded by design.

## Construction and binding

The [Rocq project](../../rocq/node_observation/README.md) proves allocation properties over arbitrary finite event histories and arbitrary natural-number random values.

The private Rust entropy parameter supplies the deterministic repeated-output regression. Production passes `Uuid::new_v4`, and the test passes `Uuid::nil`.

The test invokes the production session function over Unix sockets. It constructs the observer state directly, so it does not verify activation or executable measurement.

The event-counter exhaustion test verifies refusal before a handshake. Other existing integration tests exercise activation, transport, and cleanup separately.

The counterexample regression requires failure on the random-only implementation and success after the sequence correction. A test-only token implementation is insufficient.

The proof assumes the documented behavior of Rust integer checks, UUID formatting, decimal formatting, and exact string equality. Dependency correspondence remains subject to review.

The kernel theorems contain no custom axioms. Their abstraction still requires Rust correspondence and named maintainer review before acceptance.

## Applicability per property

The [23-property map](../../../docs/work-logs/task-019-4-node-claim-verification.md#property-coverage-plan) records inputs, histories, bounds, source boundaries, and evidence gaps.

All classifications remain provisional. All properties require binding, and no property has an accepted construction exemption.

The following table records the local applicability decision for each property. `U` requires construction over arbitrary permitted histories or states.

`F/U` includes a possible finite predicate within a larger property. That predicate requires complete-domain evidence before any construction exemption.

| Claim property | Proposed class | Current coverage | Maintainer review |
| --- | --- | --- | --- |
| A1: disabled startup | U | Pending. | Pending. |
| A2: activation and limits | F/U | Pending. | Pending. |
| A3: directory safety | U | Pending. | Pending. |
| A4: peer identity | U | Pending. | Pending. |
| A5: request identity | U | Only the challenge component within one lifetime. | Pending. |
| A6: single request and freshness | U | Challenge allocation and stale-token rejection only. | Pending. |
| A7: frames and deadline | F/U | Pending. | Pending. |
| A8: capabilities and effects | F/U | Pending. | Pending. |
| A9: cleanup and shutdown | U | Pending. | Pending. |
| A10: public configuration | F/U | Pending. | Pending. |
| B1: input limits | U | Pending. | Pending. |
| B2: bounded locks | U | Pending. | Pending. |
| B3: environment partition | U | Pending. | Pending. |
| B4: identity at open | U | Pending. | Pending. |
| B5: allocation limits | U | Pending. | Pending. |
| B6: copied state and effects | U | Pending. | Pending. |
| B7: environment validation | U | Pending. | Pending. |
| B8: generation validation | U | Pending. | Pending. |
| B9: incomplete rows | U | Pending. | Pending. |
| B10: resource release | U | Pending. | Pending. |
| B11: canonical identity | U | Pending. | Pending. |
| B12: scratch independence | U | Pending. | Pending. |
| B13: unsupported backends | F/U | Pending. | Pending. |
