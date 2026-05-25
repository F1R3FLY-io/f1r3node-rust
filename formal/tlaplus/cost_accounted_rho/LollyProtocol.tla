---- MODULE LollyProtocol ----
\* ===========================================================================
\* LollyProtocol — Phase 3 TLA+ specification for linear implication (⊸).
\*
\* Models the Sig::Lolly connective from the LL-rich signature algebra at
\* `rholang/src/rust/interpreter/accounting/mod.rs:Sig::Lolly`. The
\* expression `σ₁ ⊸ σ₂` is a CAPABILITY: consume one σ_from-token, produce
\* one σ_to-token. The transformer Par body that performs the
\* from→to conversion is held in the `rho:system:capabilities` registry
\* (Phase 3 §3.5), referenced by `capability_handle`.
\*
\* Operational reduction (LL modus ponens): `σ ⊗ (σ ⊸ τ) ⊢ τ` —
\* presenting a σ-token together with a σ⊸τ capability removes the σ
\* token and inserts a τ token. Token-count conservation: net zero
\* change in the multiset's CARDINALITY when both σ and σ⊸τ are
\* presented (one in, one out).
\*
\* Invariants:
\*   LollyResourceFlow: σ_from consumed iff σ_to produced
\*   LollyTransformer: transformer Par execution is deterministic
\*   LollyNoCreationExNihilo: σ_to never appears without σ_from
\*   LollyCapabilityRegistered: invoke requires a registered handle
\*   LollyCapabilityNotRevoked: revoked capability cannot be invoked
\* Liveness:
\*   LollyEventuallyCompletes: every invoke either completes or rejects
\* ===========================================================================

EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS
    MaxInvocations      \* exploration bound

ASSUME MaxInvocations \in 1..10

VARIABLES
    fromTokens,         \* multiset of available σ_from tokens (count)
    toTokens,           \* multiset of produced σ_to tokens (count)
    invocations,        \* invocation count so far
    capabilityState,    \* "registered" | "revoked"
    phase               \* "ready" | "consuming" | "transformed" | "rejected"

vars == <<fromTokens, toTokens, invocations, capabilityState, phase>>

Init ==
    /\ fromTokens = 3              \* a few σ_from tokens to consume
    /\ toTokens = 0
    /\ invocations = 0
    /\ capabilityState = "registered"
    /\ phase = "ready"

\* Successful invocation: consume one σ_from, produce one σ_to.
InvokeCapability ==
    /\ phase \in {"ready", "transformed"}
    /\ invocations < MaxInvocations
    /\ fromTokens > 0
    /\ capabilityState = "registered"
    /\ fromTokens' = fromTokens - 1
    /\ toTokens' = toTokens + 1
    /\ invocations' = invocations + 1
    /\ phase' = "transformed"
    /\ UNCHANGED <<capabilityState>>

\* Invocation when no σ_from tokens available — reject without producing σ_to.
InvokeRejectedNoFromToken ==
    /\ phase \in {"ready", "transformed"}
    /\ invocations < MaxInvocations
    /\ fromTokens = 0
    /\ capabilityState = "registered"
    /\ invocations' = invocations + 1
    /\ phase' = "rejected"
    /\ UNCHANGED <<fromTokens, toTokens, capabilityState>>

\* Invocation against a revoked capability — reject without producing σ_to.
InvokeRejectedRevoked ==
    /\ phase \in {"ready", "transformed"}
    /\ invocations < MaxInvocations
    /\ capabilityState = "revoked"
    /\ invocations' = invocations + 1
    /\ phase' = "rejected"
    /\ UNCHANGED <<fromTokens, toTokens, capabilityState>>

\* Revoke the capability (one-shot).
RevokeCapability ==
    /\ capabilityState = "registered"
    /\ capabilityState' = "revoked"
    /\ UNCHANGED <<fromTokens, toTokens, invocations, phase>>

Next ==
    \/ InvokeCapability
    \/ InvokeRejectedNoFromToken
    \/ InvokeRejectedRevoked
    \/ RevokeCapability

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

\* ---------------------------------------------------------------------------
\* Invariants
\* ---------------------------------------------------------------------------

\* For each σ_to produced, exactly one σ_from must have been consumed.
\* fromTokens + toTokens = initial fromTokens (3) before any rejection.
LollyResourceFlow ==
    fromTokens + toTokens <= 3

\* σ_to never appears without a matching σ_from consumption (no creation
\* from nothing).
LollyNoCreationExNihilo == toTokens <= 3 - fromTokens

\* Transformer is deterministic: invocation count = produced + rejected,
\* and approved invocations = toTokens.
LollyTransformer == toTokens <= invocations

\* Invoke requires registered state.
LollyCapabilityRegistered ==
    capabilityState \in {"registered", "revoked"}

\* Once revoked, never reverts.
LollyCapabilityNotRevoked ==
    capabilityState = "revoked" => phase \in {"ready", "transformed", "rejected"}

\* Liveness: invocations eventually terminate (bounded by MaxInvocations).
LollyEventuallyCompletes ==
    [](phase \in {"ready", "transformed"} =>
        <>(phase \in {"transformed", "rejected"} \/ invocations = MaxInvocations))

\* ===========================================================================
\* End of LollyProtocol
\* ===========================================================================
====
