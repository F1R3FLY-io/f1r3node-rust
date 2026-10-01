------------------------- MODULE RSpaceGcProtection -------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS MaxWalWindow, MaxWalEvents, OmitCoupledFacet, IgnoreExactPin

ASSUME MaxWalWindow \in Nat \ {0}
ASSUME MaxWalEvents \in Nat \ {0}
ASSUME OmitCoupledFacet \in BOOLEAN
ASSUME IgnoreExactPin \in BOOLEAN

Objects == {"manifest", "semantic", "compatibility", "history",
            "operationRoot", "replayRoot"}
Roles == {"current", "retained", "reader", "export", "import", "replay", "rollback"}
DurableRoles == {"current", "retained", "replay", "rollback"}
PinTokens == {"reader0", "reader1", "export0", "import0", "replay0", "rollback0"}
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
          durable, reclaimIntent, walHead, walTail

vars == <<phase, catalog, copied, oldPresent, oldPins, newPins,
          durable, reclaimIntent, walHead, walTail>>

ActiveRoles == durable \cup {PinRole(token) : token \in oldPins \cup newPins}
RequiredObjects == UNION {ProtectionMatrix[role].objects : role \in ActiveRoles}
DurableRequiredObjects == UNION {ProtectionMatrix[role].objects : role \in durable}
RequiredForPublication == RequiredObjects \cup CoupledFacets
WalProtected == \E role \in ActiveRoles : ProtectionMatrix[role].walBytes

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

StartCopy ==
  /\ phase = "idle"
  /\ phase' = "copying"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead, walTail>>

CopyOne ==
  /\ phase = "copying"
  /\ \E object \in Objects \ copied : copied' = copied \cup {object}
  /\ UNCHANGED <<phase, catalog, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead, walTail>>

AbortCopy ==
  /\ phase = "copying"
  /\ phase' = "aborted"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead, walTail>>

RetryCopy ==
  /\ phase = "aborted"
  /\ phase' = "copying"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead, walTail>>

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
                 durable, walHead, walTail>>

AcquireOldPin ==
  /\ oldPresent
  /\ \E token \in PinTokens \ (oldPins \cup newPins) :
       /\ (~ProtectionMatrix[PinRole(token)].walBytes \/ walHead = 0)
       /\ oldPins' = oldPins \cup {token}
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, newPins,
                 durable, reclaimIntent, walHead, walTail>>

AcquireNewPin ==
  /\ catalog = "new"
  /\ \E token \in PinTokens \ (oldPins \cup newPins) :
       /\ ProtectionMatrix[PinRole(token)].objects \subseteq copied
       /\ (~ProtectionMatrix[PinRole(token)].walBytes \/ walHead = 0)
       /\ newPins' = newPins \cup {token}
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins,
                 durable, reclaimIntent, walHead, walTail>>

ReleaseOldPin ==
  /\ oldPins # {}
  /\ \E token \in oldPins : oldPins' = oldPins \ {token}
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, newPins,
                 durable, reclaimIntent, walHead, walTail>>

ReleaseNewPin ==
  /\ newPins # {}
  /\ \E token \in newPins : newPins' = newPins \ {token}
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins,
                 durable, reclaimIntent, walHead, walTail>>

RegisterDurable ==
  /\ \E role \in {"replay", "rollback"} \ durable :
       /\ walHead = 0
       /\ (catalog = "old" \/ ProtectionMatrix[role].objects \subseteq copied)
       /\ durable' = durable \cup {role}
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 reclaimIntent, walHead, walTail>>

ReleaseDurable ==
  /\ \E role \in durable \ {"current", "retained"} :
       durable' = durable \ {role}
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 reclaimIntent, walHead, walTail>>

Reclaim ==
  /\ phase = "published"
  /\ reclaimIntent
  /\ IF IgnoreExactPin THEN TRUE ELSE oldPins = {}
  /\ oldPresent' = FALSE
  /\ reclaimIntent' = FALSE
  /\ UNCHANGED <<phase, catalog, copied, oldPins, newPins,
                 durable, walHead, walTail>>

AdvanceWal ==
  /\ walTail - walHead < MaxWalWindow
  /\ walTail < MaxWalEvents
  /\ walTail' = walTail + 1
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead>>

RotateWal ==
  /\ catalog = "new"
  /\ ~oldPresent
  /\ ~WalProtected
  /\ walHead' = walTail
  /\ UNCHANGED <<phase, catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walTail>>

Crash ==
  /\ phase \in {"copying", "aborted", "published"}
  /\ phase' = "crashed"
  /\ oldPins' = {}
  /\ newPins' = {}
  /\ UNCHANGED <<catalog, copied, oldPresent, durable,
                 reclaimIntent, walHead, walTail>>

Recover ==
  /\ phase = "crashed"
  /\ phase' = IF catalog = "new" THEN "published" ELSE "copying"
  /\ UNCHANGED <<catalog, copied, oldPresent, oldPins, newPins,
                 durable, reclaimIntent, walHead, walTail>>

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

PublicationBeforeReclamation == ~oldPresent => catalog = "new"
ExactOldPinsProtected == oldPins # {} => oldPresent
PublishedClosureProtected == catalog = "new" => DurableRequiredObjects \subseteq copied
ActiveObjectProtection ==
  \A role \in ActiveRoles :
    (catalog = "old" /\ oldPresent) \/
    ProtectionMatrix[role].objects \subseteq copied \/
    (oldPresent /\ \E token \in oldPins : PinRole(token) = role)
CoupledAliasProtected == catalog = "new" => CoupledFacets \subseteq copied
WalFrontierProtected == WalProtected => walHead = 0
WalAdmissionBounded == walHead <= walTail /\ walTail - walHead <= MaxWalWindow
CrashRetryAuthority ==
  (phase = "crashed" /\ catalog = "old") => oldPresent

=============================================================================
