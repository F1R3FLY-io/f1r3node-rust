---------------- MODULE ActiveValidatorBoundaryConvergence ----------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS
  \* @type: Set(Int);
  Validators,
  \* @type: Int;
  ProspectiveValidator,
  \* @type: Int;
  ActiveValidatorLimit,
  \* @type: Bool;
  UseCanonicalSelection,
  \* @type: Bool;
  ActivateOnlyAtBoundary

Universe == Validators \union {ProspectiveValidator}
InitialActive == Validators
\* @type: Seq(Int);
LeftArrival == <<1, ProspectiveValidator, 2, 3>>
\* @type: Seq(Int);
RightArrival == <<3, 2, ProspectiveValidator, 1>>

ASSUME /\ Validators = {1, 2, 3}
       /\ ProspectiveValidator = 4
       /\ ActiveValidatorLimit = 3
       /\ UseCanonicalSelection \in BOOLEAN
       /\ ActivateOnlyAtBoundary \in BOOLEAN

\* @type: (Int) => Int;
CanonicalRank(validator) ==
  IF validator = 1 THEN 1
  ELSE IF validator = ProspectiveValidator THEN 2
  ELSE IF validator = 2 THEN 3
  ELSE 4

\* @type: (Set(Int)) => Set(Int);
CanonicalActive(bonds) ==
  {validator \in bonds :
    Cardinality(
      {candidate \in bonds :
        CanonicalRank(candidate) <= CanonicalRank(validator)})
      <= ActiveValidatorLimit}

\* @type: (Set(Int), Seq(Int)) => Set(Int);
ArrivalActive(bonds, arrival) ==
  LET indexes ==
        {index \in 1..ActiveValidatorLimit : arrival[index] \in bonds}
  IN {arrival[index] : index \in indexes}

\* @type: (Set(Int), Seq(Int)) => Set(Int);
SelectedActive(bonds, arrival) ==
  IF UseCanonicalSelection
  THEN CanonicalActive(bonds)
  ELSE ArrivalActive(bonds, arrival)

\* @typeAlias: boundaryState = {
\*   leftBonds: Set(Int),
\*   rightBonds: Set(Int),
\*   leftActive: Set(Int),
\*   rightActive: Set(Int),
\*   leftBondSeen: Bool,
\*   rightBondSeen: Bool,
\*   leftBoundary: Bool,
\*   rightBoundary: Bool
\* };
module_typedefs == TRUE

VARIABLE
  \* @type: $boundaryState;
  state

\* @type: <<$boundaryState>>;
vars == <<state>>

Init ==
  state =
    [leftBonds |-> Validators,
     rightBonds |-> Validators,
     leftActive |-> InitialActive,
     rightActive |-> InitialActive,
     leftBondSeen |-> FALSE,
     rightBondSeen |-> FALSE,
     leftBoundary |-> FALSE,
     rightBoundary |-> FALSE]

DeliverLeftBond ==
  /\ ~state.leftBondSeen
  /\ state' =
       [state EXCEPT
         !.leftBonds = @ \union {ProspectiveValidator},
         !.leftBondSeen = TRUE,
         !.leftActive =
           IF ActivateOnlyAtBoundary
           THEN @
           ELSE SelectedActive(
                  state.leftBonds \union {ProspectiveValidator},
                  LeftArrival)]

DeliverRightBond ==
  /\ ~state.rightBondSeen
  /\ state' =
       [state EXCEPT
         !.rightBonds = @ \union {ProspectiveValidator},
         !.rightBondSeen = TRUE,
         !.rightActive =
           IF ActivateOnlyAtBoundary
           THEN @
           ELSE SelectedActive(
                  state.rightBonds \union {ProspectiveValidator},
                  RightArrival)]

SelectLeftBoundary ==
  /\ state.leftBondSeen
  /\ ~state.leftBoundary
  /\ state' =
       [state EXCEPT
         !.leftActive = SelectedActive(state.leftBonds, LeftArrival),
         !.leftBoundary = TRUE]

SelectRightBoundary ==
  /\ state.rightBondSeen
  /\ ~state.rightBoundary
  /\ state' =
       [state EXCEPT
         !.rightActive = SelectedActive(state.rightBonds, RightArrival),
         !.rightBoundary = TRUE]

Next ==
  \/ DeliverLeftBond
  \/ DeliverRightBond
  \/ SelectLeftBoundary
  \/ SelectRightBoundary

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(DeliverLeftBond)
  /\ WF_vars(DeliverRightBond)
  /\ WF_vars(SelectLeftBoundary)
  /\ WF_vars(SelectRightBoundary)

TypeOK ==
  /\ state.leftBonds \subseteq Universe
  /\ state.rightBonds \subseteq Universe
  /\ state.leftActive \subseteq Universe
  /\ state.rightActive \subseteq Universe
  /\ state.leftBondSeen \in BOOLEAN
  /\ state.rightBondSeen \in BOOLEAN
  /\ state.leftBoundary \in BOOLEAN
  /\ state.rightBoundary \in BOOLEAN

Inv_DeliveryPreservesCompleteBondLedger ==
  /\ (state.leftBondSeen => state.leftBonds = Universe)
  /\ (state.rightBondSeen => state.rightBonds = Universe)

Inv_ActivationOccursOnlyAtBoundary ==
  /\ (~state.leftBoundary => state.leftActive = InitialActive)
  /\ (~state.rightBoundary => state.rightActive = InitialActive)

Inv_ActiveValidatorsAreBonded ==
  /\ state.leftActive \subseteq state.leftBonds
  /\ state.rightActive \subseteq state.rightBonds

Inv_ActiveCommitteeIsBounded ==
  /\ Cardinality(state.leftActive) <= ActiveValidatorLimit
  /\ Cardinality(state.rightActive) <= ActiveValidatorLimit

Inv_BoundarySelectionIsCanonical ==
  /\ (state.leftBoundary =>
        state.leftActive = CanonicalActive(state.leftBonds))
  /\ (state.rightBoundary =>
        state.rightActive = CanonicalActive(state.rightBonds))

Inv_CompleteBoundaryReplicasConverge ==
  (state.leftBoundary /\ state.rightBoundary) =>
    /\ state.leftBonds = state.rightBonds
    /\ state.leftActive = state.rightActive

Live_CompleteBoundaryReplicasConverge ==
  <> /\ state.leftBoundary
     /\ state.rightBoundary
     /\ state.leftBonds = state.rightBonds
     /\ state.leftActive = state.rightActive

=============================================================================
