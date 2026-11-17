------------------------- MODULE RSpaceGcProtection -------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS MaxWalWindow, MaxWalEvents, KnownMode, OmitCoupledFacet, IgnoreExactPin,
          IgnoreDurableExactPin, OmitKnownClosure, PrunePastWalStart, BlockWalOnOldPack
CONSTANT AdmitPastWalWindow

ASSUME MaxWalWindow \in Nat \ {0}
ASSUME MaxWalEvents \in Nat \ {0}
ASSUME KnownMode \in BOOLEAN
ASSUME OmitCoupledFacet \in BOOLEAN
ASSUME IgnoreExactPin \in BOOLEAN
ASSUME IgnoreDurableExactPin \in BOOLEAN
ASSUME OmitKnownClosure \in BOOLEAN
ASSUME PrunePastWalStart \in BOOLEAN
ASSUME BlockWalOnOldPack \in BOOLEAN
ASSUME AdmitPastWalWindow \in BOOLEAN

BaseObjects == {"manifest", "semantic", "compatibility", "history",
                "operationRoot", "replayRoot"}
Objects == IF KnownMode THEN BaseObjects \cup {"knownRoot"} ELSE BaseObjects
BaseRoles == {"current", "retained", "reader", "export", "import", "replay", "rollback"}
Roles == IF KnownMode THEN BaseRoles \cup {"known"} ELSE BaseRoles
DurableRoles == {"current", "retained", "replay", "rollback"}
PinTokens ==
  IF KnownMode
  THEN {"known0", "reader0", "replay0", "rollback0"}
  ELSE {"reader0", "reader1", "export0", "import0", "replay0", "rollback0"}
WalIds == {"replay", "rollback", "replay0", "rollback0"}
NoWalStart == MaxWalEvents + 1
CoupledFacets == {"semantic", "compatibility", "history"}

Edges(object) ==
  CASE object = "manifest" -> {"semantic", "compatibility"}
    [] object = "compatibility" -> {"history"}
    [] object = "operationRoot" -> {"manifest"}
    [] object = "knownRoot" -> {"operationRoot"}
    [] object = "replayRoot" -> {"history"}
    [] OTHER -> {}

Closed(objects) == \A object \in objects : Edges(object) \subseteq objects
Closure(seeds) ==
  CHOOSE objects \in SUBSET Objects :
    seeds \subseteq objects /\ Closed(objects) /\
    \A other \in SUBSET Objects :
      (seeds \subseteq other /\ Closed(other)) => objects \subseteq other

Seed(role) ==
  CASE role = "current" -> {"manifest"}
    [] role = "retained" -> {"manifest"}
    [] role = "known" -> {"knownRoot"}
    [] role = "reader" -> {"operationRoot"}
    [] role = "export" -> {"operationRoot"}
    [] role = "import" -> {"operationRoot"}
    [] role = "replay" -> {"replayRoot"}
    [] role = "rollback" -> {"replayRoot"}

NeedsExactGeneration(role) == role \in {"known", "reader", "export", "import", "replay", "rollback"}
NeedsWal(role) == role \in {"replay", "rollback"}
ProtectionMatrix ==
  [role \in Roles |->
    [objects |-> Closure(Seed(role)),
     exactGeneration |-> NeedsExactGeneration(role),
     walBytes |-> NeedsWal(role)]]

PinRole(token) ==
  CASE token = "known0" -> "known"
    [] token \in {"reader0", "reader1"} -> "reader"
    [] token = "export0" -> "export"
    [] token = "import0" -> "import"
    [] token = "replay0" -> "replay"
    [] token = "rollback0" -> "rollback"

VARIABLES phase, catalog, copied, oldPresent, oldPins, newPins,
          durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
          reclaimIntent, walHead, walTail, walStarts

vars == <<phase, catalog, copied, oldPresent, oldPins, newPins,
          durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
          reclaimIntent, walHead, walTail, walStarts>>

ActiveRoles == durable \cup {PinRole(token) : token \in oldPins \cup newPins}
RequiredObjects == UNION {ProtectionMatrix[role].objects : role \in ActiveRoles}
NewDurableRequiredObjects ==
  UNION {ProtectionMatrix[role].objects : role \in durable \ oldDurable}
RequiredForPublication == RequiredObjects \cup CoupledFacets
RequiredWithoutKnown ==
  (UNION {ProtectionMatrix[role].objects : role \in ActiveRoles \ {"known"}}) \cup CoupledFacets
PublishRequiredObjects == IF OmitKnownClosure THEN RequiredWithoutKnown ELSE RequiredForPublication
ActiveWalIds == (durable \cap {"replay", "rollback"}) \cup
                ((oldPins \cup newPins) \cap {"replay0", "rollback0"})
Minimum(values) == CHOOSE value \in values : \A other \in values : value <= other
ReclaimableWalFrontier ==
  IF ActiveWalIds = {} THEN walTail
  ELSE Minimum({walStarts[id] : id \in ActiveWalIds})

Init ==
  /\ phase = "idle"
  /\ catalog = "old"
  /\ copied = {}
  /\ oldPresent = TRUE
  /\ oldPins = {}
  /\ newPins = {}
  /\ durable = {"current", "retained"}
  /\ oldDurable = {}
  /\ knownAtPublish = FALSE
  /\ overlapAtPublish = FALSE
  /\ crashedAfterOverlap = FALSE
  /\ knownAfterReclaim = FALSE
  /\ reclaimIntent = FALSE
  /\ walHead = 0
  /\ walTail = 0
  /\ walStarts = [id \in WalIds |-> NoWalStart]

StartCopy ==
  /\ phase = "idle"
  /\ phase' = "copying"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walHead, walTail, walStarts>>

CopyOne ==
  /\ phase = "copying"
  /\ \E object \in Objects \ copied : copied' = copied \cup {object}
  /\ UNCHANGED <<phase, catalog, oldPresent, oldPins, newPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walHead, walTail, walStarts>>

AbortCopy ==
  /\ phase = "copying"
  /\ phase' = "aborted"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walHead, walTail, walStarts>>

RetryCopy ==
  /\ phase = "aborted"
  /\ phase' = "copying"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walHead, walTail, walStarts>>

Publish ==
  /\ phase = "copying"
  /\ catalog = "old"
  /\ (IF OmitCoupledFacet
      THEN PublishRequiredObjects \ {"compatibility"}
      ELSE PublishRequiredObjects) \subseteq copied
  /\ catalog' = "new"
  /\ knownAtPublish' = ("known0" \in (oldPins \cup newPins))
  /\ overlapAtPublish' =
       ("known0" \in (oldPins \cup newPins) /\ "reader0" \in (oldPins \cup newPins))
  /\ reclaimIntent' = TRUE
  /\ phase' = "published"
  /\ UNCHANGED <<copied, oldPresent, oldPins, newPins,
                 durable, oldDurable, crashedAfterOverlap, knownAfterReclaim, walHead, walTail, walStarts>>

AcquireOldPin ==
  /\ phase # "crashed"
  /\ oldPresent
  /\ \E token \in PinTokens \ (oldPins \cup newPins) :
       /\ oldPins' = oldPins \cup {token}
       /\ \E start \in walHead..walTail :
            walStarts' = IF token \in WalIds
                         THEN [walStarts EXCEPT ![token] = start]
                         ELSE walStarts
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, newPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walHead, walTail>>

AcquireNewPin ==
  /\ phase # "crashed"
  /\ catalog = "new"
  /\ \E token \in PinTokens \ (oldPins \cup newPins) :
       /\ ProtectionMatrix[PinRole(token)].objects \subseteq copied
       /\ newPins' = newPins \cup {token}
       /\ knownAfterReclaim' =
            (knownAfterReclaim \/
             (token = "known0" /\ crashedAfterOverlap /\ ~oldPresent))
       /\ \E start \in walHead..walTail :
            walStarts' = IF token \in WalIds
                         THEN [walStarts EXCEPT ![token] = start]
                         ELSE walStarts
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap,
                 reclaimIntent, walHead, walTail>>

ReleaseOldPin ==
  /\ phase # "crashed"
  /\ oldPins # {}
  /\ \E token \in oldPins :
       /\ oldPins' = oldPins \ {token}
       /\ walStarts' = IF token \in WalIds
                       THEN [walStarts EXCEPT ![token] = NoWalStart]
                       ELSE walStarts
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, newPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walHead, walTail>>

ReleaseNewPin ==
  /\ phase # "crashed"
  /\ newPins # {}
  /\ \E token \in newPins :
       /\ newPins' = newPins \ {token}
       /\ walStarts' = IF token \in WalIds
                       THEN [walStarts EXCEPT ![token] = NoWalStart]
                       ELSE walStarts
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walHead, walTail>>

RegisterDurable ==
  /\ phase # "crashed"
  /\ \E role \in {"replay", "rollback"} \ durable :
       /\ \E generation \in {"old", "new"} :
            /\ (IF generation = "old" THEN oldPresent
                ELSE catalog = "new" /\ ProtectionMatrix[role].objects \subseteq copied)
            /\ oldDurable' = IF generation = "old" THEN oldDurable \cup {role}
                              ELSE oldDurable
       /\ durable' = durable \cup {role}
       /\ \E start \in walHead..walTail :
            walStarts' = [walStarts EXCEPT ![role] = start]
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walHead, walTail>>

ReleaseDurable ==
  /\ phase # "crashed"
  /\ \E role \in durable \ {"current", "retained"} :
       /\ durable' = durable \ {role}
       /\ oldDurable' = oldDurable \ {role}
       /\ walStarts' = [walStarts EXCEPT ![role] = NoWalStart]
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walHead, walTail>>

Reclaim ==
  /\ phase = "published"
  /\ reclaimIntent
  /\ IF IgnoreExactPin THEN TRUE ELSE oldPins = {}
  /\ IF IgnoreDurableExactPin THEN TRUE ELSE oldDurable = {}
  /\ oldPresent' = FALSE
  /\ reclaimIntent' = FALSE
  /\ UNCHANGED <<phase, catalog, copied, oldPins, newPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 walHead, walTail, walStarts>>

AdvanceWal ==
  /\ phase # "crashed"
  /\ (IF AdmitPastWalWindow THEN TRUE ELSE walTail - walHead < MaxWalWindow)
  /\ walTail < MaxWalEvents
  /\ walTail' = walTail + 1
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walHead, walStarts>>

RotateWal ==
  /\ phase = "published"
  /\ catalog = "new"
  /\ (IF BlockWalOnOldPack THEN ~oldPresent ELSE TRUE)
  /\ walHead < ReclaimableWalFrontier
  /\ walHead' = IF PrunePastWalStart /\ ReclaimableWalFrontier < walTail
                 THEN ReclaimableWalFrontier + 1
                 ELSE ReclaimableWalFrontier
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walTail, walStarts>>

Crash ==
  /\ phase \in {"copying", "aborted", "published"}
  /\ phase' = "crashed"
  /\ oldPins' = {}
  /\ newPins' = {}
  /\ walStarts' = [walStarts EXCEPT !["replay0"] = NoWalStart,
                                   !["rollback0"] = NoWalStart]
  /\ crashedAfterOverlap' = (crashedAfterOverlap \/ (overlapAtPublish /\ oldPresent))
  /\ UNCHANGED <<catalog, copied, oldPresent, durable, oldDurable, knownAtPublish,
                 overlapAtPublish, knownAfterReclaim,
                 reclaimIntent, walHead, walTail>>

Recover ==
  /\ phase = "crashed"
  /\ phase' = IF catalog = "new" THEN "published" ELSE "copying"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, oldDurable, knownAtPublish, overlapAtPublish, crashedAfterOverlap, knownAfterReclaim,
                 reclaimIntent, walHead, walTail, walStarts>>

Next == StartCopy \/ CopyOne \/ AbortCopy \/ RetryCopy \/ Publish \/
        AcquireOldPin \/ AcquireNewPin \/ ReleaseOldPin \/ ReleaseNewPin \/
        RegisterDurable \/ ReleaseDurable \/ Reclaim \/ AdvanceWal \/
        RotateWal \/ Crash \/ Recover

Spec == Init /\ [][Next]_vars

TypeInvariant ==
  /\ phase \in {"idle", "copying", "aborted", "published", "crashed"}
  /\ catalog \in {"old", "new"}
  /\ copied \subseteq Objects
  /\ oldPins \subseteq PinTokens
  /\ newPins \subseteq PinTokens
  /\ oldPins \cap newPins = {}
  /\ durable \subseteq DurableRoles
  /\ oldDurable \subseteq durable \cap {"replay", "rollback"}
  /\ knownAtPublish \in BOOLEAN
  /\ overlapAtPublish \in BOOLEAN
  /\ crashedAfterOverlap \in BOOLEAN
  /\ knownAfterReclaim \in BOOLEAN
  /\ walHead \in Nat
  /\ walTail \in Nat
  /\ walTail <= MaxWalEvents
  /\ walStarts \in [WalIds -> 0..NoWalStart]
  /\ \A id \in WalIds : (id \in ActiveWalIds) <=> (walStarts[id] # NoWalStart)

PublicationBeforeReclamation == ~oldPresent => catalog = "new"
ExactOldPinsProtected == oldPins # {} => oldPresent
DurableExactGenerationProtected == oldDurable # {} => oldPresent
PublishedClosureProtected == catalog = "new" => NewDurableRequiredObjects \subseteq copied
KnownPublicationClosureProtected ==
  IF KnownMode
  THEN knownAtPublish => catalog = "new" /\ ProtectionMatrix["known"].objects \subseteq copied
  ELSE ~knownAtPublish
KnownExactGenerationProtected ==
  IF KnownMode
  THEN ("known0" \in oldPins => oldPresent) /\
       ("known0" \in newPins => ProtectionMatrix["known"].objects \subseteq copied)
  ELSE TRUE
OperationRootRoleQuotient ==
  /\ PinRole("reader0") = "reader"
  /\ PinRole("export0") = "export"
  /\ PinRole("import0") = "import"
  /\ {"reader0", "export0", "import0"} \cap WalIds = {}
  /\ ProtectionMatrix["reader"].objects = ProtectionMatrix["export"].objects
  /\ ProtectionMatrix["reader"].objects = ProtectionMatrix["import"].objects
  /\ ProtectionMatrix["reader"].exactGeneration
  /\ ProtectionMatrix["export"].exactGeneration
  /\ ProtectionMatrix["import"].exactGeneration
  /\ ~ProtectionMatrix["reader"].walBytes
  /\ ~ProtectionMatrix["export"].walBytes
  /\ ~ProtectionMatrix["import"].walBytes
KnownWalOrthogonal ==
  IF KnownMode
  THEN ~ProtectionMatrix["known"].walBytes /\ "known0" \notin WalIds /\
       "known0" \notin ActiveWalIds
  ELSE TRUE
OverlapWitnessHistorySound ==
  /\ (overlapAtPublish => knownAtPublish /\ catalog = "new")
  /\ (crashedAfterOverlap => overlapAtPublish)
  /\ (knownAfterReclaim => crashedAfterOverlap /\ ~oldPresent /\ catalog = "new")
KnownOverlapLifecycleAbsent ==
  ~(KnownMode /\ knownAfterReclaim)
ActiveObjectProtection ==
  \A role \in ActiveRoles :
    (catalog = "old" /\ oldPresent) \/
    ProtectionMatrix[role].objects \subseteq copied \/
    (oldPresent /\ role \in oldDurable) \/
    (oldPresent /\ \E token \in oldPins : PinRole(token) = role)
CoupledAliasProtected == catalog = "new" => CoupledFacets \subseteq copied
WalFrontierProtected ==
  \A id \in ActiveWalIds : walHead <= walStarts[id] /\ walStarts[id] <= walTail
WalReclaimableMinimum == walHead <= ReclaimableWalFrontier
ObjectOnlyPinDoesNotBlockWal ==
  (catalog = "new" /\ oldPresent /\ oldPins # {} /\
   ActiveWalIds = {} /\ walHead < walTail) => ENABLED RotateWal
WalAdmissionBounded == walHead <= walTail /\ walTail - walHead <= MaxWalWindow
CrashRetryAuthority ==
  (phase = "crashed" /\ catalog = "old") => oldPresent
CrashPhaseQuiescence ==
  (phase = "crashed") =>
    ~ENABLED (AcquireOldPin \/ AcquireNewPin \/ ReleaseOldPin \/ ReleaseNewPin \/
              RegisterDurable \/ ReleaseDurable \/ AdvanceWal \/ RotateWal)

=============================================================================
