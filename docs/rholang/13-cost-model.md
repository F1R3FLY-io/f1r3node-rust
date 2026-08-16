# Cost-accounted Rholang

F1R3node meters Rholang communication with authority tokens. It does not use
the legacy client-selected `phlo_limit × phlo_price` gas model. A deploy cannot
buy permission to run by declaring a larger limit: validators derive its
capacity from authority committed in the authenticated pre-state, prove that
the capacity is sufficient, execute within that finite capacity, and settle the
communication events that actually occurred.

The design follows the five gated communication rules and the join-cost schema
in [`cost-accounted-rho.tex`](../../../publications/cost-accounting/cost-accounted-rho.tex),
and the cost endofunctor over continued interactive generalized structured
labelled transition systems (GSLTs) in
[`continued-gslt-cost-v2.tex`](../../../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex).

## Terms and units

| Term | Meaning |
| --- | --- |
| `COMM` | One successful, atomic RSpace synchronization between a waiting continuation and all required data |
| Authority signature | The canonical identity of a purse allowed to fund a communication |
| Supply $`\Sigma(a)`$ | Available authority at signature $`a`$ in the authenticated pre-state |
| Certified bound $`B(a)`$ | A proved finite upper bound for authority $`a`$ |
| Realized cost $`\kappa(a)`$ | Authority consumed by successful `COMM` events in the committed execution |
| Fixed fee $`f(a)`$ | Deterministic per-deploy transfer to the proposing validator, separate from communication cost |
| State-bound witness | Exact execution evidence tied to the pre-state root, block context, envelopes, causal event log, and adjacent roots |

The base unit is a funded rendezvous. A send that waits consumes no authority.
A receive or join that waits consumes no authority. When matching succeeds, the
runtime reserves authority once before RSpace mutates tuple-space state or the
causal log. A binary rendezvous therefore contributes one `COMM`; an
$`N`$-channel join also contributes one atomic `COMM`, not $`N`$ independently
committed partial events.

The authority shape can still have several components. Whole-redex signing may
draw one combined cell. Separately signed participants may require component
authority from every participant. Regrouping changes which purses supply the
atomic event; it does not split the join into partially committed rendezvous.

## Admission, execution, and settlement

For each authority key $`a`$, admission requires:

```math
B(a) + f(a) \leq \Sigma(a).
```

The structural analyzer returns one of two results:

- `FiniteUpperBound`: a conservative proof for a closed fragment; parallel
  branches add and mutually exclusive alternatives take their point-wise
  maximum.
- `Unprovable`: the submitted syntax cannot bound interactions induced by
  authenticated ambient state, received code, or unresolved dequotation.

`Unprovable` is not rejection by guesswork. Production evaluates the canonical
deploy sequence against the authenticated merged root under finite,
authority-derived capacity. The completed execution becomes the state-bound
witness and the committed user transition. An exhausted evaluation cannot
certify itself.

After execution, validators settle only realized authority:

```math
\Sigma'(a) = \Sigma(a) - \kappa(a) - f(a).
```

The unused reservation remains in the purse:

```math
\operatorname{unused}(a) = B(a) - \kappa(a).
```

This is the publication's conservative-reservation and refund semantics without
an unsafe debit/refund window: the implementation never debits the unused
portion.

The admission algorithm is a terminating fixed point:

```text
retained := canonical_sort(candidates)
repeat
    capacity := read_authority(retained, authenticated_pre_state)
    witness, exhausted := execute_once(retained, capacity)
    retained := retained minus exhausted
    admitted, underfunded := verify_exact_cost_and_fee(witness, capacity)
    retained := admitted
until exhausted and underfunded are empty
return witness bound to retained and its exact execution context
```

Each non-terminal iteration removes at least one candidate. The final witness is
opaque to checkpoint callers and can be consumed only with its exact pre-state,
block data, invalid-block set, and canonical envelope list.

## Atomic `COMM` accounting

The accounting hook is inside RSpace's locked match-commit path. Its order is:

```text
find a complete match
construct the canonical COMM identity
reserve authority for that COMM
record the triggering I/O and COMM
remove the continuation and matched data
resume the continuation
```

If reservation fails, the triggering I/O is not recorded, no matched datum or
continuation is removed, and the enclosing deploy returns
`OutOfPhlogistonsError`. The deploy soft checkpoint restores all earlier changes
from that deploy. The same ordering is used by replay before it consumes a
recorded `COMM` binding.

Charging attempted send and receive introductions is incorrect. Which side
arrives last depends on scheduling, native continuations may produce responses
without traversing the same reducer path, and a join has several introductions
but only one atomic contraction. Charging introductions caused play/replay cost
divergence even when both executions reproduced the same RSpace interaction.

## Schedule independence and replay

The exact RSpace log is a causal replay witness. Validators rig replay with that
witness and independently derive authority capacity from authenticated state.
They verify the canonical envelope, status, realized cost, event log, adjacent
pre/post roots, settlement, and fee.

The diagnostic cost digest intentionally excludes scheduler-local source paths,
redex identifiers, and local indices. It includes the stable deploy identity,
authority identity, event kind, primitive descriptor, weight, and multiplicity.
The digest can therefore diagnose equivalent work without becoming an
alternative consensus trace.

Independent operations may arrive in either order. They need not produce the
same physical instruction interleaving, but they must produce the same
authority-event multiset, realized scalar cost, accepted/rejected verdict, and
post-state. The causal RSpace witness chooses the concrete replay ordering when
several matches are possible.

## Examples

An unmatched output is free:

```rholang
@"requests"!(42)
```

It remains stored and has realized communication cost zero until a receive
matches it.

A matched pair costs one atomic event:

```rholang
@"requests"!(42) |
for (value <- @"requests") { Nil }
```

Reversing which side is installed first does not change that cost.

A two-channel join is also one atomic event:

```rholang
@"left"!(1) |
@"right"!(2) |
for (x <- @"left"; y <- @"right") { Nil }
```

Neither message is partially consumed. The join fires only after both are
available and authority reservation succeeds.

## Failure semantics

- A parser or signature error occurs before execution and consumes no
  communication authority.
- Insufficient authenticated supply rejects the deploy at admission.
- Runtime exhaustion cannot produce a state-bound certificate and cannot commit
  the triggering `COMM`.
- A replay cost, status, event, root, or settlement mismatch is objective block
  invalidity.
- Missing local history, unavailable storage, and temporary node-local inability
  to validate are local faults. They are retried or recovered and never create
  slash evidence.

## Implementation and verification map

| Obligation | Implementation | Verification |
| --- | --- | --- |
| One debit per successful atomic match | `rspace_interface::CommObserver`; `RSpace::locked_consume`; `RSpace::process_match_found`; replay counterparts | `comm_observer_tests`; Rholang trigger-order, join, native-response, and exhaustion tests |
| Finite authority-derived execution | `RuntimeManager::certify_state_bound_admission`; `RuntimeOps::state_bound_cost_evidence_for_state_cosigned` | `StateBoundAdmission.tla`; `EndToEndAuthority.v`; Casper state-bound tests |
| Schedule-independent realized cost | locked RSpace match linearization; semantic-only `COMM::cost_identity`; canonical budget fold | `AtomicCommAccounting.tla`; `AtomicCommAccounting.v`; trigger-order and telemetry-mutation tests |
| No mutation on rejected `COMM` | observer-before-log/state ordering plus deploy soft checkpoint | `AtomicCommRejection.tla`; RSpace atomic rejection test; Rholang OOP rollback test |
| Independent replay agreement | causal event witness and replay-derived settlement | `StateBoundValidatorConvergence.tla`; independent-runtime bridge replay regression |
| No local-fault slashing | `ValidationDisposition` | end-to-end TLA+ negative controls and block-processor tests |

See
[`end-to-end-authority-settlement.md`](../theory/cost-accounting-impl/end-to-end-authority-settlement.md)
for the full architecture and
[`cost-accounted-rho-verification.md`](../theory/cost-accounted-rho-verification.md)
for the proof and test catalog.
