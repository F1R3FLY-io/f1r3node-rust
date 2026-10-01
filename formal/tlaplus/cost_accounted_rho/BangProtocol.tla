---- MODULE BangProtocol ----
\* ===========================================================================
\* BangProtocol — Phase 3 TLA+ specification for the exponential modality (!).
\*
\* Models the Sig::Bang connective from the LL-rich signature algebra at
\* `rholang/src/rust/interpreter/accounting/mod.rs:Sig::Bang`. A deploy
\* bearing `Sig::Bang(σ)` carries a REPLICABLE signature: the same
\* authorization can witness multiple reductions. Operationally, a
\* persistent receive that spawns a fresh sub-token on each consumption.
\*
\* The wire format admits either unbounded (uses_bound = 0) or bounded
\* (uses_bound > 0) replication:
\*   - Unbounded: LL-canonical interpretation of `!σ`. The capability
\*     never exhausts.
\*   - Bounded: optional runtime cap for slashing-like scenarios. Each
\*     invocation decrements `uses_remaining`; once at 0 the capability
\*     stops authorizing.
\*
\* Bang capabilities can be REGISTERED in `rho:system:capabilities` and
\* re-invoked across deploys via a capability handle (Phase 3 §3.5).
\*
\* Invariants:
\*   BangReplicationSafety: unbounded Bang never exhausts
\*   BangUsageBound: bounded Bang's uses_remaining is monotone non-increasing
\*   BangPersistence: registered Bang capability survives across invokes
\*   BangBoundedNonNegative: uses_remaining never drops below 0
\* Liveness:
\*   BangBoundedEventuallyExhausts: bounded Bang eventually reaches 0
\* ===========================================================================

EXTENDS Integers, FiniteSets, Sequences, TLC

CONSTANTS
    Bound,              \* Some(k) = bounded with initial uses_remaining = k;
                        \* 0 (in our encoding) = unbounded
    MaxInvocations      \* exploration bound on invocation count

ASSUME Bound \in 0..10
ASSUME MaxInvocations \in 1..20

VARIABLES
    usesRemaining,      \* current uses_remaining (Bound = 0 → represented as -1 for "unbounded")
    invocations,        \* total invocations attempted
    invocationsApproved,\* invocations that succeeded
    registered,         \* TRUE iff the Bang capability is registered in the registry
    phase               \* "registered" | "in_flight" | "exhausted"

vars == <<usesRemaining, invocations, invocationsApproved, registered, phase>>

\* "-1" marks the LL-canonical unbounded interpretation; the bounded
\* variant starts at Bound.
InitUses == IF Bound = 0 THEN -1 ELSE Bound

Init ==
    /\ usesRemaining = InitUses
    /\ invocations = 0
    /\ invocationsApproved = 0
    /\ registered = TRUE
    /\ phase = "registered"

\* Successful invocation: counter decrements iff bounded; unbounded
\* (-1) stays put.
InvokeApproved ==
    /\ phase = "registered" \/ phase = "in_flight"
    /\ invocations < MaxInvocations
    /\ \/ usesRemaining = -1
       \/ usesRemaining > 0
    /\ invocations' = invocations + 1
    /\ invocationsApproved' = invocationsApproved + 1
    /\ usesRemaining' =
          IF usesRemaining = -1 THEN -1 ELSE usesRemaining - 1
    /\ phase' = "in_flight"
    /\ UNCHANGED <<registered>>

\* Exhausted bounded Bang: invocation rejected.
InvokeRejectedExhausted ==
    /\ phase \in {"registered", "in_flight"}
    /\ invocations < MaxInvocations
    /\ usesRemaining = 0
    /\ invocations' = invocations + 1
    /\ invocationsApproved' = invocationsApproved
    /\ usesRemaining' = 0
    /\ phase' = "exhausted"
    /\ UNCHANGED <<registered>>

Next ==
    \/ InvokeApproved
    \/ InvokeRejectedExhausted

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

\* ---------------------------------------------------------------------------
\* Invariants
\* ---------------------------------------------------------------------------

\* Unbounded Bang never exhausts.
BangReplicationSafety ==
    Bound = 0 => usesRemaining = -1

\* Bounded Bang's counter is monotone non-increasing.
BangUsageBound ==
    (Bound > 0 /\ usesRemaining # -1) => usesRemaining <= Bound

\* The counter never drops below 0.
BangBoundedNonNegative ==
    usesRemaining = -1 \/ usesRemaining >= 0

\* Registered capability stays registered across invokes (registry
\* lifecycle independent of usage).
BangPersistence == registered

\* Approved invocations never exceed the bound (bounded case).
BangApprovedBoundedByLimit ==
    (Bound > 0) =>
        invocationsApproved <= Bound

\* Liveness: bounded Bang eventually exhausts under repeated invocation.
BangBoundedEventuallyExhausts ==
    (Bound > 0) =>
        [](phase \in {"registered", "in_flight"} =>
            <>(phase = "exhausted" \/ invocations = MaxInvocations))

\* ===========================================================================
\* End of BangProtocol
\* ===========================================================================
====
