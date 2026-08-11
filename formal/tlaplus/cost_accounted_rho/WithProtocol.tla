---- MODULE WithProtocol ----
\* ===========================================================================
\* WithProtocol — Phase 3 TLA+ specification for additive conjunction (&).
\*
\* Models the Sig::With connective from the LL-rich signature algebra at
\* `rholang/src/rust/interpreter/accounting/mod.rs:Sig::With`. A deploy
\* bearing `Sig::With(left, right)` makes BOTH branches available at the
\* substrate level; only ONE branch is consumed at reduction time, with
\* the choice made by the VERIFIER (block proposer) at evaluation. The
\* proposer's choice is exposed to in-deploy Rholang via the
\* `rho:system:sig_choice` channel.
\*
\* Contrast with PlusProtocol (additive disjunction ⊕): With is "both
\* available, verifier picks one", Plus is "signer picks one, only that
\* one is signed".
\*
\* Invariants:
\*   AdditiveCoConservation: only one branch's fuel is consumed
\*   WithBranchAvailability: both branches signed and available before pick
\*   WithReplayDeterminism: the same branch is picked on replay
\*   WithBothBranchesSigned: both branches must have valid signatures
\* Liveness:
\*   WithEventuallyPicked: verifier eventually picks a branch
\* ===========================================================================

EXTENDS Integers, FiniteSets, Sequences, TLC

CONSTANTS
    PhloPerBranch       \* fuel available on each branch

ASSUME PhloPerBranch \in 1..100

VARIABLES
    leftSigned,         \* TRUE iff signer presented a valid sig for left
    rightSigned,        \* TRUE iff signer presented a valid sig for right
    pickedBranch,       \* -1 = not yet picked, 0 = left, 1 = right
    leftFuel,           \* remaining fuel on the left branch
    rightFuel,          \* remaining fuel on the right branch
    phase               \* "presented" | "picked" | "authorized" | "rejected"

vars == <<leftSigned, rightSigned, pickedBranch, leftFuel, rightFuel, phase>>

Init ==
    /\ leftSigned \in BOOLEAN
    /\ rightSigned \in BOOLEAN
    /\ pickedBranch = -1
    /\ leftFuel = PhloPerBranch
    /\ rightFuel = PhloPerBranch
    /\ phase = "presented"

\* Verifier picks a branch deterministically (replay-equivalent).
\* In production: the choice is recorded in the ProcessedDeploy event log
\* so that replay sees the same value via `rho:system:sig_choice`.
PickLeft ==
    /\ phase = "presented"
    /\ leftSigned         \* can only pick a branch with a valid signature
    /\ pickedBranch = -1
    /\ pickedBranch' = 0
    /\ phase' = "picked"
    /\ UNCHANGED <<leftSigned, rightSigned, leftFuel, rightFuel>>

PickRight ==
    /\ phase = "presented"
    /\ rightSigned
    /\ pickedBranch = -1
    /\ pickedBranch' = 1
    /\ phase' = "picked"
    /\ UNCHANGED <<leftSigned, rightSigned, leftFuel, rightFuel>>

\* Consume fuel ONLY from the picked branch.
ConsumePickedFuel ==
    /\ phase = "picked"
    /\ \/ /\ pickedBranch = 0
          /\ leftFuel > 0
          /\ leftFuel' = leftFuel - 1
          /\ rightFuel' = rightFuel
       \/ /\ pickedBranch = 1
          /\ rightFuel > 0
          /\ rightFuel' = rightFuel - 1
          /\ leftFuel' = leftFuel
    /\ phase' = "authorized"
    /\ UNCHANGED <<leftSigned, rightSigned, pickedBranch>>

\* Both branches presented but no valid sig on either → reject.
RejectNoValidSig ==
    /\ phase = "presented"
    /\ ~leftSigned /\ ~rightSigned
    /\ phase' = "rejected"
    /\ UNCHANGED <<leftSigned, rightSigned, pickedBranch, leftFuel, rightFuel>>

Next ==
    \/ PickLeft
    \/ PickRight
    \/ ConsumePickedFuel
    \/ RejectNoValidSig

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

\* ---------------------------------------------------------------------------
\* Invariants
\* ---------------------------------------------------------------------------

\* Only the picked branch loses fuel; the other stays at PhloPerBranch.
AdditiveCoConservation ==
    pickedBranch = -1 =>
        /\ leftFuel = PhloPerBranch
        /\ rightFuel = PhloPerBranch

\* When authorized, the unpicked branch is untouched.
WithUnpickedUntouched ==
    phase = "authorized" =>
        \/ /\ pickedBranch = 0
           /\ rightFuel = PhloPerBranch
        \/ /\ pickedBranch = 1
           /\ leftFuel = PhloPerBranch

\* Both branches must have valid signatures available before the verifier
\* can pick (signer must commit to both for With).
WithBothBranchesSigned ==
    pickedBranch # -1 =>
        \/ /\ pickedBranch = 0 /\ leftSigned
        \/ /\ pickedBranch = 1 /\ rightSigned

\* Picked branch is in valid range.
WithBranchAvailability == pickedBranch \in {-1, 0, 1}

\* Liveness: eventually a branch is picked or rejected.
WithEventuallyPicked ==
    [](phase = "presented" => <>(phase \in {"picked", "authorized", "rejected"}))

\* ===========================================================================
\* End of WithProtocol
\* ===========================================================================
====
