---------------------- MODULE MCLocatedAuthoritySettlement ----------------------
EXTENDS LocatedAuthoritySettlement

CONSTANTS
    client,
    gateway,
    sponsor,
    authSig,
    slotSig,
    wholeSig,
    recvSig,
    sendSig1,
    sendSig2,
    authLoc,
    slotLoc,
    wholeLoc,
    recvLoc,
    sendLoc1,
    sendLoc2,
    authRegion,
    continuationRegion,
    laterSlotRegion,
    wholeJoinRegion,
    splitRecvRegion,
    splitSendRegion1,
    splitSendRegion2,
    combinedRecvRegion,
    combinedSendRegion1,
    combinedSendRegion2,
    authPurse,
    slotFundingPurse,
    wholeJoinPurse,
    splitRecvPurse,
    splitSendPurse1,
    splitSendPurse2,
    combinedJoinPurse,
    ambientFundingPurse,
    alternateFundingPurse,
    envelopeFundingPurse,
    authorize,
    continue,
    later,
    wholeJoin,
    splitJoin,
    combinedJoin,
    authSurface,
    continuationSurface,
    laterSurface,
    wholeRecvSurface,
    wholeSendSurface1,
    wholeSendSurface2,
    splitRecvSurface,
    splitSendSurface1,
    splitSendSurface2,
    combinedRecvSurface,
    combinedSendSurface1,
    combinedSendSurface2,
    deployAuthorize,
    deployLater,
    deployWholeJoin,
    deploySplitJoin,
    deployCombinedJoin,
    none

ActorsDef == {client, gateway, sponsor}
SignaturesDef == {authSig, slotSig, wholeSig, recvSig, sendSig1, sendSig2}
LocationsDef == {authLoc, slotLoc, wholeLoc, recvLoc, sendLoc1, sendLoc2}
RegionsDef == {
    authRegion,
    continuationRegion,
    laterSlotRegion,
    wholeJoinRegion,
    splitRecvRegion,
    splitSendRegion1,
    splitSendRegion2,
    combinedRecvRegion,
    combinedSendRegion1,
    combinedSendRegion2
}
PursesDef == {
    authPurse,
    slotFundingPurse,
    wholeJoinPurse,
    splitRecvPurse,
    splitSendPurse1,
    splitSendPurse2,
    combinedJoinPurse,
    ambientFundingPurse,
    alternateFundingPurse,
    envelopeFundingPurse
}
EventsDef == {authorize, continue, later, wholeJoin, splitJoin, combinedJoin}
SurfacesDef == {
    authSurface,
    continuationSurface,
    laterSurface,
    wholeRecvSurface,
    wholeSendSurface1,
    wholeSendSurface2,
    splitRecvSurface,
    splitSendSurface1,
    splitSendSurface2,
    combinedRecvSurface,
    combinedSendSurface1,
    combinedSendSurface2
}
DeploymentsDef == {
    deployAuthorize,
    deployLater,
    deployWholeJoin,
    deploySplitJoin,
    deployCombinedJoin
}

RegionSignatureDef ==
    [region \in RegionsDef |->
      CASE region = authRegion -> authSig
        [] region \in {continuationRegion, laterSlotRegion} -> slotSig
        [] region = wholeJoinRegion -> wholeSig
        [] region \in {splitRecvRegion, combinedRecvRegion} -> recvSig
        [] region \in {splitSendRegion1, combinedSendRegion1} -> sendSig1
        [] OTHER -> sendSig2]

RegionLocationDef ==
    [region \in RegionsDef |->
      CASE region = authRegion -> authLoc
        [] region \in {continuationRegion, laterSlotRegion} -> slotLoc
        [] region = wholeJoinRegion -> wholeLoc
        [] region \in {splitRecvRegion, combinedRecvRegion} -> recvLoc
        [] region \in {splitSendRegion1, combinedSendRegion1} -> sendLoc1
        [] OTHER -> sendLoc2]

EventRegionsDef ==
    [event \in EventsDef |->
      CASE event = authorize -> {authRegion}
        [] event = continue -> {continuationRegion}
        [] event = later -> {laterSlotRegion}
        [] event = wholeJoin -> {wholeJoinRegion}
        [] event = splitJoin -> {splitRecvRegion, splitSendRegion1, splitSendRegion2}
        [] OTHER -> {combinedRecvRegion, combinedSendRegion1, combinedSendRegion2}]

EventSurfacesDef ==
    [event \in EventsDef |->
      CASE event = authorize -> {authSurface}
        [] event = continue -> {continuationSurface}
        [] event = later -> {laterSurface}
        [] event = wholeJoin -> {wholeRecvSurface, wholeSendSurface1, wholeSendSurface2}
        [] event = splitJoin -> {splitRecvSurface, splitSendSurface1, splitSendSurface2}
        [] OTHER -> {combinedRecvSurface, combinedSendSurface1, combinedSendSurface2}]

EventDependenciesDef ==
    [event \in EventsDef |-> IF event = continue THEN {authorize} ELSE {}]

EventDeploymentDef ==
    [event \in EventsDef |->
      CASE event \in {authorize, continue} -> deployAuthorize
        [] event = later -> deployLater
        [] event = wholeJoin -> deployWholeJoin
        [] event = splitJoin -> deploySplitJoin
        [] OTHER -> deployCombinedJoin]

FundingPurseDef ==
    [event \in EventsDef |->
      [region \in EventRegionsDef[event] |->
        CASE event = authorize -> authPurse
          [] event \in {continue, later} -> slotFundingPurse
          [] event = wholeJoin -> wholeJoinPurse
          [] event = splitJoin /\ region = splitRecvRegion -> splitRecvPurse
          [] event = splitJoin /\ region = splitSendRegion1 -> splitSendPurse1
          [] event = splitJoin -> splitSendPurse2
          [] OTHER -> combinedJoinPurse]]

PurseAtomsDef ==
    [purse \in PursesDef |->
      [signature \in SignaturesDef |->
        CASE purse = authPurse /\ signature = authSig -> 1
          [] purse = slotFundingPurse /\ signature = slotSig -> 1
          [] purse = wholeJoinPurse /\ signature = wholeSig -> 1
          [] purse = splitRecvPurse /\ signature = recvSig -> 1
          [] purse = splitSendPurse1 /\ signature = sendSig1 -> 1
          [] purse = splitSendPurse2 /\ signature = sendSig2 -> 1
          [] purse = combinedJoinPurse /\ signature \in {recvSig, sendSig1, sendSig2} -> 1
          [] OTHER -> 0]]

NearDef ==
    [region \in RegionsDef |->
      [purse \in PursesDef |->
        \/ region = authRegion /\ purse = authPurse
        \/ region \in {continuationRegion, laterSlotRegion} /\ purse = slotFundingPurse
        \/ region = wholeJoinRegion /\ purse = wholeJoinPurse
        \/ region = splitRecvRegion /\ purse = splitRecvPurse
        \/ region = splitSendRegion1 /\ purse = splitSendPurse1
        \/ region = splitSendRegion2 /\ purse = splitSendPurse2
        \/ region \in {combinedRecvRegion, combinedSendRegion1, combinedSendRegion2}
             /\ purse = combinedJoinPurse]]

InitialBalanceDef == [purse \in PursesDef |-> 4]

DeployBoundDef ==
    [deployment \in DeploymentsDef |->
      [purse \in PursesDef |->
        CASE deployment = deployAuthorize /\ purse \in {authPurse, slotFundingPurse} -> 1
          [] deployment = deployLater /\ purse = slotFundingPurse -> 1
          [] deployment = deployWholeJoin /\ purse = wholeJoinPurse -> 1
          [] deployment = deploySplitJoin
               /\ purse \in {splitRecvPurse, splitSendPurse1, splitSendPurse2} -> 1
          [] deployment = deployCombinedJoin /\ purse = combinedJoinPurse -> 1
          [] OTHER -> 0]]

DeployOrderDef ==
    <<deployAuthorize, deployLater, deployWholeJoin, deploySplitJoin, deployCombinedJoin>>

ReplayOrderDef == <<authorize, continue, later, wholeJoin, splitJoin, combinedJoin>>

=============================================================================
