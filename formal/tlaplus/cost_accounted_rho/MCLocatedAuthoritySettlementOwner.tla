------------------- MODULE MCLocatedAuthoritySettlementOwner -------------------
(***************************************************************************)
(* G3 (DR-135): the owner scenario of LocatedAuthoritySettlement.tla.      *)
(*                                                                         *)
(* An installer stored a process earlier. Its region carries the stored    *)
(* seal installerSig, and its located purse is installerPurse, an owner    *)
(* purse. Two deployments fire the stored process in the same run          *)
(* (ownerDraw1 and ownerDraw2). Neither deployment reserves the installer  *)
(* purse. The owner budget of the installer purse is 1, so one draw fires  *)
(* and the other is rejected with no charge (Option 2 of G3), in either    *)
(* order. A third deployment runs unsealedTrigger, a trigger whose only    *)
(* region is its own client region (triggerSig). It pays from its own      *)
(* reserved purse triggerPurse, and it never draws the installer purse.    *)
(*                                                                         *)
(* The lollipop, join and slot constants of the module must name events    *)
(* and purses of the scenario. They name scenario members, and the owner   *)
(* configs do not check the invariants that are about those constants.    *)
(***************************************************************************)
EXTENDS LocatedAuthoritySettlement

CONSTANTS
    installer,
    client,
    installerSig,
    triggerSig,
    installerLoc,
    triggerLoc,
    ownerRegion1,
    ownerRegion2,
    triggerRegion,
    installerPurse,
    triggerPurse,
    ownerDraw1,
    ownerDraw2,
    unsealedTrigger,
    ownerSurface1,
    ownerSurface2,
    triggerSurface,
    deployOwner1,
    deployOwner2,
    deployTrigger,
    none

OwnerActorsDef == {installer, client}
OwnerSignaturesDef == {installerSig, triggerSig}
OwnerLocationsDef == {installerLoc, triggerLoc}
OwnerRegionsDef == {ownerRegion1, ownerRegion2, triggerRegion}
OwnerScenarioPursesDef == {installerPurse, triggerPurse}
OwnerEventsDef == {ownerDraw1, ownerDraw2, unsealedTrigger}
OwnerSurfacesDef == {ownerSurface1, ownerSurface2, triggerSurface}
OwnerDeploymentsDef == {deployOwner1, deployOwner2, deployTrigger}

OwnerRegionSignatureDef ==
    [region \in OwnerRegionsDef |->
      IF region = triggerRegion THEN triggerSig ELSE installerSig]

OwnerRegionLocationDef ==
    [region \in OwnerRegionsDef |->
      IF region = triggerRegion THEN triggerLoc ELSE installerLoc]

OwnerEventRegionsDef ==
    [event \in OwnerEventsDef |->
      CASE event = ownerDraw1 -> {ownerRegion1}
        [] event = ownerDraw2 -> {ownerRegion2}
        [] OTHER -> {triggerRegion}]

OwnerEventSurfacesDef ==
    [event \in OwnerEventsDef |->
      CASE event = ownerDraw1 -> {ownerSurface1}
        [] event = ownerDraw2 -> {ownerSurface2}
        [] OTHER -> {triggerSurface}]

OwnerEventDependenciesDef == [event \in OwnerEventsDef |-> {}]

OwnerEventDeploymentDef ==
    [event \in OwnerEventsDef |->
      CASE event = ownerDraw1 -> deployOwner1
        [] event = ownerDraw2 -> deployOwner2
        [] OTHER -> deployTrigger]

OwnerFundingPurseDef ==
    [event \in OwnerEventsDef |->
      [region \in OwnerEventRegionsDef[event] |->
        IF event = unsealedTrigger THEN triggerPurse ELSE installerPurse]]

OwnerPurseAtomsDef ==
    [purse \in OwnerScenarioPursesDef |->
      [signature \in OwnerSignaturesDef |->
        CASE purse = installerPurse /\ signature = installerSig -> 1
          [] purse = triggerPurse /\ signature = triggerSig -> 1
          [] OTHER -> 0]]

OwnerNearDef ==
    [region \in OwnerRegionsDef |->
      [purse \in OwnerScenarioPursesDef |->
        \/ region \in {ownerRegion1, ownerRegion2} /\ purse = installerPurse
        \/ region = triggerRegion /\ purse = triggerPurse]]

OwnerInitialBalanceDef == [purse \in OwnerScenarioPursesDef |-> 4]

\* No deployment reserves the owner purse.
OwnerDeployBoundDef ==
    [deployment \in OwnerDeploymentsDef |->
      [purse \in OwnerScenarioPursesDef |->
        IF deployment = deployTrigger /\ purse = triggerPurse THEN 1 ELSE 0]]

OwnerDeployOrderDef == <<deployOwner1, deployOwner2, deployTrigger>>

OwnerReplayOrderDef == <<ownerDraw1, ownerDraw2, unsealedTrigger>>

OwnerPursesDef == {installerPurse}

\* The owner budget: the balance at R_{i-1} minus the later and own holds.
OwnerBudgetDef ==
    [purse \in OwnerScenarioPursesDef |->
      IF purse = installerPurse THEN 1 ELSE 0]

=============================================================================
