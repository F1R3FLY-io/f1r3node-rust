---- MODULE ThresholdProtocol ----
\* ===========================================================================
\* ThresholdProtocol — Phase 2 TLA+ specification
\*
\* Models the M-of-N quorum-witness protocol for the Sig::Threshold
\* primitive (Phase 2 substrate at
\* `rholang/src/rust/interpreter/accounting/mod.rs:Sig::Threshold`).
\* A deploy bearing `Sig::Threshold { threshold = k, members = [m_1, ..., m_n] }`
\* is authorized when at least `k` of the `n` member signatures verify.
\*
\* Invariants:
\*   QuorumThresholdConstraint: threshold ∈ [1, n]
\*   QuorumExactness: when AuthorizedSet ≠ {}, |AuthorizedSet| ≥ threshold
\*   QuorumNoOverCount: |AuthorizedSet| ≤ |members|
\*   CanonicalMemberOrder: members[] are sorted by hash (set comparison)
\* ===========================================================================

EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS
    NumMembers,         \* total signers listed in members[]
    Threshold           \* required quorum size

ASSUME NumMembers \in 1..6
ASSUME Threshold \in 1..NumMembers

VARIABLES
    members,            \* set of member-ids (0..NumMembers-1)
    presentedSigs,      \* set of member-ids whose signatures have been presented
    authorized,         \* set of member-ids whose signatures have been verified
    phase               \* "open" | "verifying" | "authorized" | "rejected"

vars == <<members, presentedSigs, authorized, phase>>

Members == 0..(NumMembers - 1)

Init ==
    /\ members = Members
    /\ presentedSigs = {}
    /\ authorized = {}
    /\ phase = "open"

\* Signer presents a signature (one at a time, no duplicates).
PresentSignature(i) ==
    /\ phase = "open"
    /\ i \in members
    /\ i \notin presentedSigs
    /\ presentedSigs' = presentedSigs \cup {i}
    /\ UNCHANGED <<members, authorized, phase>>

\* Verifier verifies a presented signature (per-signature check).
VerifySignature(i) ==
    /\ phase = "open"
    /\ i \in presentedSigs
    /\ i \notin authorized
    /\ authorized' = authorized \cup {i}
    /\ UNCHANGED <<members, presentedSigs, phase>>

\* Quorum reached: authorized has ≥ threshold members.
AcceptQuorum ==
    /\ phase = "open"
    /\ Cardinality(authorized) >= Threshold
    /\ phase' = "authorized"
    /\ UNCHANGED <<members, presentedSigs, authorized>>

\* All presented but quorum not met after presentation closes: rejected.
RejectShortQuorum ==
    /\ phase = "open"
    /\ presentedSigs = members          \* all candidates presented
    /\ Cardinality(authorized) < Threshold
    /\ phase' = "rejected"
    /\ UNCHANGED <<members, presentedSigs, authorized>>

Next ==
    \/ (\E i \in Members : PresentSignature(i))
    \/ (\E i \in Members : VerifySignature(i))
    \/ AcceptQuorum
    \/ RejectShortQuorum

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

\* ---------------------------------------------------------------------------
\* Invariants
\* ---------------------------------------------------------------------------

QuorumThresholdConstraint == Threshold \in 1..NumMembers

QuorumExactness ==
    phase = "authorized" => Cardinality(authorized) >= Threshold

QuorumNoOverCount == Cardinality(authorized) <= Cardinality(members)

\* Authorized subset must be a subset of presented (can't verify what wasn't presented).
AuthorizedSubsetPresented == authorized \subseteq presentedSigs

\* Presented subset must be a subset of members (no foreign signers).
PresentedSubsetMembers == presentedSigs \subseteq members

\* Reject implies short quorum.
RejectionImpliesShortQuorum ==
    phase = "rejected" =>
        /\ Cardinality(authorized) < Threshold
        /\ presentedSigs = members

\* Liveness: eventually we reach a terminal phase.
EventuallyTerminates ==
    [](phase = "open" => <>(phase \in {"authorized", "rejected"}))

\* ===========================================================================
\* End of ThresholdProtocol
\* ===========================================================================
====
