------------------------- MODULE RSpaceGcProtection -------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS MaxWalWindow, MaxWalEvents, OmitCoupledFacet, IgnoreExactPin,
          PrunePastWalStart, BlockWalOnOldPack

ASSUME MaxWalWindow \in Nat \ {0}
ASSUME MaxWalEvents \in Nat \ {0}
ASSUME OmitCoupledFacet \in BOOLEAN
ASSUME IgnoreExactPin \in BOOLEAN
ASSUME PrunePastWalStart \in BOOLEAN
ASSUME BlockWalOnOldPack \in BOOLEAN

Objects == {"manifest", "semantic", "compatibility", "history",
            "operationRoot", "replayRoot"}
Roles == {"current", "retained", "reader", "export", "import", "replay", "rollback"}
DurableRoles == {"current", "retained", "replay", "rollback"}
PinTokens == {"reader0", "reader1", "export0", "import0", "replay0", "rollback0"}
WalIds == {"replay", "rollback", "replay0", "rollback0"}
NoWalStart == MaxWalEvents + 1
CoupledFacets == {"semantic", "compatibility", "history"}

Edges(object) ==
  CASE object = "manifest" -> {"semantic", "compatibility"}
    [] object = "compatibility" -> {"history"}
    [] object = "operationRoot" -> {"manifest"}
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
    [] role = "reader" -> {"operationRoot"}
    [] role = "export" -> {"operationRoot"}
    [] role = "import" -> {"operationRoot"}
    [] role = "replay" -> {"replayRoot"}
    [] role = "rollback" -> {"replayRoot"}

NeedsExactGeneration(role) == role \in {"reader", "export", "import", "replay", "rollback"}
NeedsWal(role) == role \in {"replay", "rollback"}
ProtectionMatrix ==
  [role \in Roles |->
    [objects |-> Closure(Seed(role)),
     exactGeneration |-> NeedsExactGeneration(role),
     walBytes |-> NeedsWal(role)]]

PinRole(token) ==
  CASE token \in {"reader0", "reader1"} -> "reader"
    [] token = "export0" -> "export"
    [] token = "import0" -> "import"
    [] token = "replay0" -> "replay"
    [] token = "rollback0" -> "rollback"

VARIABLES phase, catalog, copied, oldPresent, oldPins, newPins,
          durable, reclaimIntent, walHead, walTail, walStarts

vars == <<phase, catalog, copied, oldPresent, oldPins, newPins,
          durable, reclaimIntent, walHead, walTail, walStarts>>

ActiveRoles == durable \cup {PinRole(token) : token \in oldPins \cup newPins}
RequiredObjects == UNION {ProtectionMatrix[role].objects : role \in ActiveRoles}
DurableRequiredObjects == UNION {ProtectionMatrix[role].objects : role \in durable}
RequiredForPublication == RequiredObjects \cup CoupledFacets
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
  /\ reclaimIntent = FALSE
  /\ walHead = 0
  /\ walTail = 0
  /\ walStarts = [id \in WalIds |-> NoWalStart]

StartCopy ==
  /\ phase = "idle"
  /\ phase' = "copying"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead, walTail, walStarts>>

CopyOne ==
  /\ phase = "copying"
  /\ \E object \in Objects \ copied : copied' = copied \cup {object}
  /\ UNCHANGED <<phase, catalog, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead, walTail, walStarts>>

AbortCopy ==
  /\ phase = "copying"
  /\ phase' = "aborted"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead, walTail, walStarts>>

RetryCopy ==
  /\ phase = "aborted"
  /\ phase' = "copying"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead, walTail, walStarts>>

Publish ==
  /\ phase = "copying"
  /\ catalog = "old"
  /\ (IF OmitCoupledFacet
      THEN RequiredForPublication \ {"compatibility"}
      ELSE RequiredForPublication) \subseteq copied
  /\ catalog' = "new"
  /\ reclaimIntent' = TRUE
  /\ phase' = "published"
  /\ UNCHANGED <<copied, oldPresent, oldPins, newPins,
                 durable, walHead, walTail, walStarts>>

AcquireOldPin ==
  /\ oldPresent
  /\ \E token \in PinTokens \ (oldPins \cup newPins) :
       /\ oldPins' = oldPins \cup {token}
       /\ \E start \in walHead..walTail :
            walStarts' = IF token \in WalIds
                         THEN [walStarts EXCEPT ![token] = start]
                         ELSE walStarts
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, newPins,
                 durable, reclaimIntent, walHead, walTail>>

AcquireNewPin ==
  /\ catalog = "new"
  /\ \E token \in PinTokens \ (oldPins \cup newPins) :
       /\ ProtectionMatrix[PinRole(token)].objects \subseteq copied
       /\ newPins' = newPins \cup {token}
       /\ \E start \in walHead..walTail :
            walStarts' = IF token \in WalIds
                         THEN [walStarts EXCEPT ![token] = start]
                         ELSE walStarts
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins,
                 durable, reclaimIntent, walHead, walTail>>

ReleaseOldPin ==
  /\ oldPins # {}
  /\ \E token \in oldPins :
       /\ oldPins' = oldPins \ {token}
       /\ walStarts' = IF token \in WalIds
                       THEN [walStarts EXCEPT ![token] = NoWalStart]
                       ELSE walStarts
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, newPins,
                 durable, reclaimIntent, walHead, walTail>>

ReleaseNewPin ==
  /\ newPins # {}
  /\ \E token \in newPins :
       /\ newPins' = newPins \ {token}
       /\ walStarts' = IF token \in WalIds
                       THEN [walStarts EXCEPT ![token] = NoWalStart]
                       ELSE walStarts
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins,
                 durable, reclaimIntent, walHead, walTail>>

RegisterDurable ==
  /\ \E role \in {"replay", "rollback"} \ durable :
       /\ (catalog = "old" \/ ProtectionMatrix[role].objects \subseteq copied)
       /\ durable' = durable \cup {role}
       /\ \E start \in walHead..walTail :
            walStarts' = [walStarts EXCEPT ![role] = start]
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 reclaimIntent, walHead, walTail>>

ReleaseDurable ==
  /\ \E role \in durable \ {"current", "retained"} :
       /\ durable' = durable \ {role}
       /\ walStarts' = [walStarts EXCEPT ![role] = NoWalStart]
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 reclaimIntent, walHead, walTail>>

Reclaim ==
  /\ phase = "published"
  /\ reclaimIntent
  /\ IF IgnoreExactPin THEN TRUE ELSE oldPins = {}
  /\ oldPresent' = FALSE
  /\ reclaimIntent' = FALSE
  /\ UNCHANGED <<phase, catalog, copied, oldPins, newPins,
                 durable, walHead, walTail, walStarts>>

AdvanceWal ==
  /\ walTail - walHead < MaxWalWindow
  /\ walTail < MaxWalEvents
  /\ walTail' = walTail + 1
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead, walStarts>>

RotateWal ==
  /\ catalog = "new"
  /\ (IF BlockWalOnOldPack THEN ~oldPresent ELSE TRUE)
  /\ walHead < ReclaimableWalFrontier
  /\ walHead' = IF PrunePastWalStart /\ ReclaimableWalFrontier < walTail
                 THEN ReclaimableWalFrontier + 1
                 ELSE ReclaimableWalFrontier
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walTail, walStarts>>

Crash ==
  /\ phase \in {"copying", "aborted", "published"}
  /\ phase' = "crashed"
  /\ oldPins' = {}
  /\ newPins' = {}
  /\ walStarts' = [walStarts EXCEPT !["replay0"] = NoWalStart,
                                   !["rollback0"] = NoWalStart]
  /\ UNCHANGED <<catalog, copied, oldPresent, durable,
                 reclaimIntent, walHead, walTail>>

Recover ==
  /\ phase = "crashed"
  /\ phase' = IF catalog = "new" THEN "published" ELSE "copying"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead, walTail, walStarts>>

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
  /\ walHead \in Nat
  /\ walTail \in Nat
  /\ walTail <= MaxWalEvents
  /\ walStarts \in [WalIds -> 0..NoWalStart]
  /\ \A id \in WalIds : (id \in ActiveWalIds) <=> (walStarts[id] # NoWalStart)

PublicationBeforeReclamation == ~oldPresent => catalog = "new"
ExactOldPinsProtected == oldPins # {} => oldPresent
PublishedClosureProtected == catalog = "new" => DurableRequiredObjects \subseteq copied
ActiveObjectProtection ==
  \A role \in ActiveRoles :
    (catalog = "old" /\ oldPresent) \/
    ProtectionMatrix[role].objects \subseteq copied \/
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

=============================================================================
