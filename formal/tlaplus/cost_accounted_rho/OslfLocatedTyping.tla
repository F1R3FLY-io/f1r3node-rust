------------------------ MODULE OslfLocatedTyping ------------------------
EXTENDS Naturals, TLC

CONSTANTS
  \* @type: Bool;
  AllowContraction,
  \* @type: Bool;
  AllowWeakening,
  \* @type: Bool;
  AliasSpatialSurfaces,
  \* @type: Bool;
  TreatUpperBoundAsExact,
  \* @type: Bool;
  CreditCandidateSupply

ASSUME /\ AllowContraction \in BOOLEAN
       /\ AllowWeakening \in BOOLEAN
       /\ AliasSpatialSurfaces \in BOOLEAN
       /\ TreatUpperBoundAsExact \in BOOLEAN
       /\ CreditCandidateSupply \in BOOLEAN

Surfaces == 1..6
WorkSurfaces == {2, 4}

AuthenticatedSupply ==
  [surface \in Surfaces |->
    CASE surface = 1 -> 0
      [] surface = 3 -> 1
      [] surface = 5 -> 2
      [] surface = 6 -> 1
      [] OTHER -> 2]

CandidateSupply ==
  [surface \in Surfaces |-> IF surface = 1 THEN 1 ELSE 0]

ExactDemand ==
  [surface \in Surfaces |->
    CASE surface = 1 -> 1
      [] surface = 3 -> 0
      [] surface = 5 -> 2
      [] surface = 6 -> 0
      [] OTHER -> 1]

UpperDemand ==
  [surface \in Surfaces |-> IF surface = 3 THEN 1 ELSE ExactDemand[surface]]

ObservedFundingSupply ==
  [surface \in Surfaces |->
    AuthenticatedSupply[surface]
      + IF CreditCandidateSupply THEN CandidateSupply[surface] ELSE 0]

LinearAccepted(surface) ==
  /\ AuthenticatedSupply[surface] >= 1
  /\ IF AllowContraction
        THEN ExactDemand[surface] >= 1
        ELSE IF AllowWeakening
               THEN ExactDemand[surface] <= 1
               ELSE ExactDemand[surface] = 1

VARIABLES
  \* @type: Str;
  phase,
  \* @type: Int -> Int;
  supply,
  \* @type: Int -> Int;
  remaining,
  \* @type: Int -> Int;
  spent,
  \* @type: Bool;
  fundingAccepted,
  \* @type: Bool;
  upperModalAccepted,
  \* @type: Bool;
  crossSurfaceDebit

vars == <<phase, supply, remaining, spent, fundingAccepted,
          upperModalAccepted, crossSurfaceDebit>>

Init ==
  /\ phase = "Check"
  /\ supply = AuthenticatedSupply
  /\ remaining = ExactDemand
  /\ spent = [surface \in Surfaces |-> 0]
  /\ fundingAccepted = FALSE
  /\ upperModalAccepted = FALSE
  /\ crossSurfaceDebit = FALSE

Check ==
  /\ phase = "Check"
  /\ fundingAccepted' =
       (ObservedFundingSupply[1] >= UpperDemand[1])
  /\ upperModalAccepted' =
       IF TreatUpperBoundAsExact
         THEN /\ UpperDemand[3] >= 1
              /\ AuthenticatedSupply[3] >= 1
         ELSE FALSE
  /\ phase' = "Run"
  /\ UNCHANGED <<supply, remaining, spent, crossSurfaceDebit>>

DebitTarget(surface) ==
  IF AliasSpatialSurfaces /\ surface = 2 THEN 4 ELSE surface

Spend(surface) ==
  LET target == DebitTarget(surface) IN
  /\ phase = "Run"
  /\ surface \in WorkSurfaces
  /\ remaining[surface] > 0
  /\ supply[target] > 0
  /\ supply' = [supply EXCEPT ![target] = @ - 1]
  /\ remaining' = [remaining EXCEPT ![surface] = @ - 1]
  /\ spent' = [spent EXCEPT ![surface] = @ + 1]
  /\ crossSurfaceDebit' = (crossSurfaceDebit \/ target # surface)
  /\ UNCHANGED <<phase, fundingAccepted, upperModalAccepted>>

Next == Check \/ \E surface \in WorkSurfaces : Spend(surface)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in {"Check", "Run"}
  /\ supply \in [Surfaces -> Nat]
  /\ remaining \in [Surfaces -> Nat]
  /\ spent \in [Surfaces -> Nat]
  /\ fundingAccepted \in BOOLEAN
  /\ upperModalAccepted \in BOOLEAN
  /\ crossSurfaceDebit \in BOOLEAN

LinearNoContraction ==
  \A surface \in Surfaces : LinearAccepted(surface) => ExactDemand[surface] <= 1

LinearNoWeakening ==
  \A surface \in Surfaces : LinearAccepted(surface) => ExactDemand[surface] >= 1

ModalEvidenceSound ==
  upperModalAccepted => ExactDemand[3] >= 1

AuthenticatedFundingOnly ==
  fundingAccepted => UpperDemand[1] <= AuthenticatedSupply[1]

LocationIsolation == ~crossSurfaceDebit

ModalPoststateExact ==
  \A surface \in WorkSurfaces :
    /\ remaining[surface] + spent[surface] = ExactDemand[surface]
    /\ supply[surface] + spent[surface] = AuthenticatedSupply[surface]

LocalSufficiencyComposes ==
  \A surface \in WorkSurfaces :
    spent[surface] <= AuthenticatedSupply[surface]

DisjointSpatialSettlement ==
  (\A surface \in WorkSurfaces : remaining[surface] = 0) =>
    \A surface \in WorkSurfaces :
      supply[surface] = AuthenticatedSupply[surface] - ExactDemand[surface]

=============================================================================
