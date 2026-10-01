---- MODULE PlusProtocol ----
\* ===========================================================================
\* PlusProtocol — Phase 3 TLA+ specification for the additive disjunction (⊕).
\*
\* Models the Sig::Plus connective from the LL-rich signature algebra at
\* `rholang/src/rust/interpreter/accounting/mod.rs:Sig::Plus`. A deploy
\* bearing `Sig::Plus(left, right)` carries an explicit `chosen_branch`
\* witness in the wire format (`SigPlus.chosen_branch` ∈ {0,1}); fuel is
\* consumed ONLY from the chosen branch. This is the signer-decides
\* additive disjunction; verifier-decides additive conjunction is in
\* WithProtocol.tla.
\*
\* The branch witness is replay-deterministic by virtue of being
\* serialized in the wire format — replay re-reads the same witness from
\* the on-chain `ProcessedDeploy.deploy: DeployDataProto.sig_algebra` field.
\*
\* Invariants:
\*   AdditiveChoiceDeterminism: chosenBranch is fixed at wire decode
\*   PlusBranchWitness: only the chosen branch's fuel is consumed
\*   PlusNonChosenUntouched: non-chosen branch fuel is never debited
\*   PlusBranchInRange: chosenBranch ∈ {0, 1}
\* Liveness:
\*   PlusEventuallyAuthorizes: chosen-branch authorization eventually fires
\* ===========================================================================

EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS
    PhloPerBranch       \* fuel available on each branch (symmetric)

ASSUME PhloPerBranch \in 1..100

VARIABLES
    chosenBranch,       \* 0 (left) or 1 (right) — fixed at Init
    leftFuel,           \* fuel still available on the left branch
    rightFuel,          \* fuel still available on the right branch
    phase               \* "decoded" | "authorizing" | "authorized" | "rejected"

vars == <<chosenBranch, leftFuel, rightFuel, phase>>

Init ==
    /\ chosenBranch \in {0, 1}        \* signer's choice — fixed at wire decode
    /\ leftFuel = PhloPerBranch
    /\ rightFuel = PhloPerBranch
    /\ phase = "decoded"

\* Begin authorization: consume fuel ONLY from the chosen branch.
ConsumeChosenFuel ==
    /\ phase = "decoded"
    /\ \/ /\ chosenBranch = 0
          /\ leftFuel > 0
          /\ leftFuel' = leftFuel - 1
          /\ rightFuel' = rightFuel
       \/ /\ chosenBranch = 1
          /\ rightFuel > 0
          /\ rightFuel' = rightFuel - 1
          /\ leftFuel' = leftFuel
    /\ phase' = "authorizing"
    /\ UNCHANGED <<chosenBranch>>

CompleteAuthorize ==
    /\ phase = "authorizing"
    /\ phase' = "authorized"
    /\ UNCHANGED <<chosenBranch, leftFuel, rightFuel>>

\* Chosen-branch exhausted before completion → reject.
RejectExhausted ==
    /\ phase \in {"decoded", "authorizing"}
    /\ \/ /\ chosenBranch = 0 /\ leftFuel = 0
       \/ /\ chosenBranch = 1 /\ rightFuel = 0
    /\ phase' = "rejected"
    /\ UNCHANGED <<chosenBranch, leftFuel, rightFuel>>

Next ==
    \/ ConsumeChosenFuel
    \/ CompleteAuthorize
    \/ RejectExhausted

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

\* ---------------------------------------------------------------------------
\* Invariants
\* ---------------------------------------------------------------------------

\* The chosen branch is fixed at decode (every Next action keeps
\* chosenBranch UNCHANGED). This is a temporal property — every
\* state's chosenBranch equals the next state's chosenBranch.
AdditiveChoiceDeterminism == [][chosenBranch' = chosenBranch]_vars

PlusBranchInRange == chosenBranch \in {0, 1}

\* Only the chosen branch loses fuel; the other stays at PhloPerBranch.
PlusBranchWitness ==
    \/ /\ chosenBranch = 0
       /\ rightFuel = PhloPerBranch
    \/ /\ chosenBranch = 1
       /\ leftFuel = PhloPerBranch

\* Non-chosen branch is never touched.
PlusNonChosenUntouched ==
    \/ /\ chosenBranch = 0
       /\ rightFuel = PhloPerBranch
    \/ /\ chosenBranch = 1
       /\ leftFuel = PhloPerBranch

\* Liveness: eventually authorization terminates.
PlusEventuallyAuthorizes ==
    [](phase \in {"decoded", "authorizing"} =>
        <>(phase \in {"authorized", "rejected"}))

\* ===========================================================================
\* End of PlusProtocol
\* ===========================================================================
====
