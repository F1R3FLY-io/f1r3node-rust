-------------------- MODULE AuthorityFloorStateBinding --------------------
EXTENDS FiniteSets

CONSTANT
  \* @type: Bool;
  RequireExactStateBinding

ASSUME RequireExactStateBinding \in BOOLEAN

Validators == {"v1", "v2", "v3"}
ActiveFloorCommittee == {"v1", "v2"}
FloorHashes == {"floor-a", "floor-b"}
StateHashes == {"state-a", "state-b"}
StoredFloorHash == "floor-a"
StoredFloorState == "state-a"
Phases == {"Idle", "Received", "Bound"}

VARIABLES
  \* @type: Str;
  phase,
  \* @type: Str;
  claimedFloorHash,
  \* @type: Str;
  claimedFloorState,
  \* @type: Set(Str);
  targetCommittee,
  \* @type: Set(Str);
  headCommittee,
  \* @type: Set(Str);
  selectedCommittee,
  \* @type: Bool;
  finalityAuthorized

vars == <<phase, claimedFloorHash, claimedFloorState, targetCommittee,
          headCommittee, selectedCommittee, finalityAuthorized>>

ExactFloorIdentity ==
  claimedFloorHash = StoredFloorHash /\ claimedFloorState = StoredFloorState

BindingAccepts ==
  claimedFloorHash = StoredFloorHash /\
  (IF RequireExactStateBinding
   THEN claimedFloorState = StoredFloorState
   ELSE TRUE)

Init ==
  /\ phase = "Idle"
  /\ claimedFloorHash = StoredFloorHash
  /\ claimedFloorState = StoredFloorState
  /\ targetCommittee = {}
  /\ headCommittee = {}
  /\ selectedCommittee = {}
  /\ finalityAuthorized = FALSE

ReceiveTarget(floorHash, floorState, targetView, headView) ==
  /\ phase = "Idle"
  /\ floorHash \in FloorHashes
  /\ floorState \in StateHashes
  /\ targetView \in SUBSET Validators
  /\ headView \in SUBSET Validators
  /\ phase' = "Received"
  /\ claimedFloorHash' = floorHash
  /\ claimedFloorState' = floorState
  /\ targetCommittee' = targetView
  /\ headCommittee' = headView
  /\ selectedCommittee' = {}
  /\ finalityAuthorized' = FALSE

ChangeAmbientViews(targetView, headView) ==
  /\ phase \in {"Received", "Bound"}
  /\ targetView \in SUBSET Validators
  /\ headView \in SUBSET Validators
  /\ targetCommittee' = targetView
  /\ headCommittee' = headView
  /\ UNCHANGED <<phase, claimedFloorHash, claimedFloorState,
                  selectedCommittee, finalityAuthorized>>

SelectCertifiedCommittee ==
  /\ phase = "Received"
  /\ phase' = "Bound"
  /\ finalityAuthorized' = BindingAccepts
  /\ selectedCommittee' =
       IF BindingAccepts THEN ActiveFloorCommittee ELSE {}
  /\ UNCHANGED <<claimedFloorHash, claimedFloorState,
                  targetCommittee, headCommittee>>

Next ==
  \/ \E floorHash \in FloorHashes,
       floorState \in StateHashes,
       targetView \in SUBSET Validators,
       headView \in SUBSET Validators :
       ReceiveTarget(floorHash, floorState, targetView, headView)
  \/ \E targetView \in SUBSET Validators,
       headView \in SUBSET Validators :
       ChangeAmbientViews(targetView, headView)
  \/ SelectCertifiedCommittee

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in Phases
  /\ claimedFloorHash \in FloorHashes
  /\ claimedFloorState \in StateHashes
  /\ targetCommittee \in SUBSET Validators
  /\ headCommittee \in SUBSET Validators
  /\ selectedCommittee \in SUBSET Validators
  /\ finalityAuthorized \in BOOLEAN

Inv_MismatchedPairCannotAuthorize ==
  phase = "Bound" /\ ~ExactFloorIdentity => ~finalityAuthorized

Inv_AuthorizationIffExactPair ==
  phase = "Bound" => finalityAuthorized = ExactFloorIdentity

Inv_ExactPairSelectsActiveCommittee ==
  phase = "Bound" /\ ExactFloorIdentity =>
    finalityAuthorized /\ selectedCommittee = ActiveFloorCommittee

Inv_UnauthorizedPairSelectsNoCommittee ==
  phase = "Bound" /\ ~finalityAuthorized => selectedCommittee = {}

Inv_CertifiedCommitteeIgnoresAmbientViews ==
  phase = "Bound" /\ finalityAuthorized =>
    selectedCommittee = ActiveFloorCommittee
=============================================================================
