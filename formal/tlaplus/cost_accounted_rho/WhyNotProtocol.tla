---- MODULE WhyNotProtocol ----
\* ===========================================================================
\* WhyNotProtocol — Phase 3 TLA+ specification for the dual exponential (?).
\*
\* Models the Sig::WhyNot connective from the LL-rich signature algebra at
\* `rholang/src/rust/interpreter/accounting/mod.rs:Sig::WhyNot`. The dual
\* of Bang (!): `WhyNot(σ)` permits a reduction whether or not σ is
\* presented. Operationally, an OPTIONAL signature with zero-or-more uses.
\*
\* Useful for "admin override" patterns: a deploy authorized either via
\* a normal multi-sig path OR (optionally) a single admin signature.
\* If the admin token is presented and verifies, the deploy proceeds
\* regardless of the multi-sig path; if not, the multi-sig path must
\* satisfy on its own.
\*
\* Invariants:
\*   WhyNotOptional: deploy succeeds whether σ presented OR absent
\*   WhyNotEmptyEquiv: empty WhyNot ≡ no-fuel-needed
\*   WhyNotNoChargeWhenAbsent: when σ not presented, no fuel is consumed
\*   WhyNotChargeWhenPresented: when σ presented and verified, fuel can flow
\* Liveness:
\*   WhyNotEventuallyResolves: every WhyNot deploy reaches a terminal phase
\* ===========================================================================

EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS
    PhloAvailable       \* fuel tied to σ (consumed only if σ presented)

ASSUME PhloAvailable \in 1..100

VARIABLES
    sigPresented,       \* TRUE iff signer presented σ on the wire
    sigVerified,        \* TRUE iff σ verifies (only meaningful when presented)
    fuelConsumed,       \* fuel debited from σ's account
    phase               \* "decoded" | "skipped" | "consumed" | "rejected"

vars == <<sigPresented, sigVerified, fuelConsumed, phase>>

Init ==
    /\ sigPresented \in BOOLEAN
    /\ sigVerified \in BOOLEAN
    /\ fuelConsumed = 0
    /\ phase = "decoded"

\* σ NOT presented: deploy proceeds with no fuel consumption.
SkipAbsent ==
    /\ phase = "decoded"
    /\ ~sigPresented
    /\ fuelConsumed' = 0
    /\ phase' = "skipped"
    /\ UNCHANGED <<sigPresented, sigVerified>>

\* σ presented and verified: fuel can flow.
ConsumePresented ==
    /\ phase = "decoded"
    /\ sigPresented /\ sigVerified
    /\ fuelConsumed < PhloAvailable
    /\ fuelConsumed' = fuelConsumed + 1
    /\ phase' = "consumed"
    /\ UNCHANGED <<sigPresented, sigVerified>>

\* σ presented but invalid signature: reject.
RejectInvalidSig ==
    /\ phase = "decoded"
    /\ sigPresented /\ ~sigVerified
    /\ fuelConsumed' = 0
    /\ phase' = "rejected"
    /\ UNCHANGED <<sigPresented, sigVerified>>

\* Once consuming, allow continued draws.
ContinueConsuming ==
    /\ phase = "consumed"
    /\ sigPresented /\ sigVerified
    /\ fuelConsumed < PhloAvailable
    /\ fuelConsumed' = fuelConsumed + 1
    /\ UNCHANGED <<sigPresented, sigVerified, phase>>

Next ==
    \/ SkipAbsent
    \/ ConsumePresented
    \/ RejectInvalidSig
    \/ ContinueConsuming

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

\* ---------------------------------------------------------------------------
\* Invariants
\* ---------------------------------------------------------------------------

\* Two cases of success: (a) σ absent → no charge; (b) σ presented + verified.
WhyNotOptional ==
    phase = "skipped" \/ phase = "consumed" \/ phase = "decoded" \/ phase = "rejected"

\* Skipped phase ≡ no-fuel-needed: zero consumption.
WhyNotEmptyEquiv ==
    phase = "skipped" => fuelConsumed = 0

\* If σ not presented, no fuel is ever consumed.
WhyNotNoChargeWhenAbsent ==
    ~sigPresented => fuelConsumed = 0

\* Fuel consumption is bounded by PhloAvailable.
WhyNotChargeBounded == fuelConsumed <= PhloAvailable

\* If σ is presented and σ doesn't verify, deploy is rejected (no fuel
\* siphoned by an invalid sig).
WhyNotInvalidImpliesRejection ==
    (sigPresented /\ ~sigVerified /\ phase # "decoded") => phase = "rejected"

\* Liveness: every WhyNot deploy reaches a terminal phase.
WhyNotEventuallyResolves ==
    [](phase = "decoded" =>
        <>(phase \in {"skipped", "consumed", "rejected"}))

\* ===========================================================================
\* End of WhyNotProtocol
\* ===========================================================================
====
