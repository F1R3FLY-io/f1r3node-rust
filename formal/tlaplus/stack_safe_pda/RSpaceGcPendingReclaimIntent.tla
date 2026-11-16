------------------ MODULE RSpaceGcPendingReclaimIntent ------------------
EXTENDS FiniteSets, TLC

CONSTANTS DropOnOrdinaryAdvance, OverwritePendingIntent, AtomicIntentPublication,
          RemoveIntentTempOnRecover, RefuseWhilePending
ASSUME DropOnOrdinaryAdvance \in BOOLEAN
ASSUME OverwritePendingIntent \in BOOLEAN
ASSUME AtomicIntentPublication \in BOOLEAN
ASSUME RemoveIntentTempOnRecover \in BOOLEAN
ASSUME RefuseWhilePending \in BOOLEAN

AParts == {"aPack", "aIndex"}
BParts == {"bPack", "bIndex"}
CParts == {"cPack", "cIndex"}
IntentTemp == "intentTemp"
Parts == AParts \cup BParts \cup CParts \cup {IntentTemp}
Generations == {"A", "B"}
SyncMarkers == {"aUnlinksSynced", "bUnlinksSynced"}
Catalogs == {"A", "B", "B1", "C", "C1"}
Intents == {"none", "AB", "BC", "torn"}
Stages == {"idle", "ABFiles", "ABIntent", "ABSelected",
           "BCFiles", "BCIntent", "BCSelected"}

VARIABLES live, catalog, intent, stage, files, pins, retired,
          ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
          retryWitness
vars == <<live, catalog, intent, stage, files, pins, retired,
          ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
          retryWitness>>
SelectedB == catalog \in {"B", "B1"}
SelectedC == catalog \in {"C", "C1"}
OldPartsRemain == AParts \cap files # {}
MidPartsRemain == BParts \cap files # {}

Init ==
  /\ live = TRUE
  /\ catalog = "A"
  /\ intent = "none"
  /\ stage = "idle"
  /\ files = AParts
  /\ pins = {}
  /\ retired = {}
  /\ ackAB = FALSE
  /\ ackBC = FALSE
  /\ pinnedCheckpoint = FALSE
  /\ pendingAdvance = FALSE
  /\ restartedPending = FALSE
  /\ retryWitness = FALSE

AcquireA ==
  /\ live /\ AParts \subseteq files /\ "A" \notin retired
  /\ "A" \notin pins /\ catalog \in {"A", "B", "B1"}
  /\ pins' = pins \cup {"A"}
  /\ UNCHANGED <<live, catalog, intent, stage, files, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

AcquireB ==
  /\ live /\ BParts \subseteq files /\ "B" \notin retired
  /\ "B" \notin pins /\ catalog \in {"B", "B1", "C", "C1"}
  /\ pins' = pins \cup {"B"}
  /\ UNCHANGED <<live, catalog, intent, stage, files, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

Release(generation) ==
  /\ generation \in pins
  /\ pins' = pins \ {generation}
  /\ UNCHANGED <<live, catalog, intent, stage, files, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

CleanOrphanB ==
  /\ live /\ catalog = "A" /\ stage = "idle" /\ intent = "none"
  /\ BParts \cap files # {}
  /\ files' = files \ BParts
  /\ UNCHANGED <<live, catalog, intent, stage, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

CleanOrphanC ==
  /\ live /\ SelectedB /\ stage = "idle" /\ intent \in {"none", "AB"}
  /\ CParts \cap files # {}
  /\ files' = files \ CParts
  /\ UNCHANGED <<live, catalog, intent, stage, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

StageAB ==
  /\ live /\ catalog = "A" /\ stage = "idle" /\ intent = "none"
  /\ BParts \cap files = {}
  /\ stage' = "ABFiles"
  /\ files' = files \cup BParts
  /\ UNCHANGED <<live, catalog, intent, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

PersistAB ==
  /\ live /\ catalog = "A" /\ stage = "ABFiles" /\ intent = "none"
  /\ IntentTemp \notin files
  /\ intent' = "AB"
  /\ stage' = "ABIntent"
  /\ UNCHANGED <<live, catalog, files, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

SelectAB ==
  /\ live /\ catalog = "A" /\ stage = "ABIntent" /\ intent = "AB"
  /\ catalog' = "B"
  /\ stage' = "ABSelected"
  /\ UNCHANGED <<live, intent, files, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

AckAB ==
  /\ live /\ catalog = "B" /\ stage = "ABSelected"
  /\ stage' = "idle"
  /\ ackAB' = TRUE
  /\ UNCHANGED <<live, catalog, intent, files, pins, retired,
                 ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

AdvanceB ==
  /\ live /\ catalog = "B" /\ stage = "idle"
  /\ catalog' = "B1"
  /\ intent' = IF DropOnOrdinaryAdvance THEN "none" ELSE intent
  /\ pendingAdvance' = (pendingAdvance \/ (intent = "AB" /\ OldPartsRemain))
  /\ UNCHANGED <<live, stage, files, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, restartedPending, retryWitness>>

StageBC ==
  /\ live /\ SelectedB /\ stage = "idle"
  /\ IF RefuseWhilePending
       THEN /\ (intent = "none" \/ OverwritePendingIntent)
            /\ (AParts \cap files = {} \/ OverwritePendingIntent)
       ELSE intent \in {"none", "AB"}
  /\ CParts \cap files = {}
  /\ stage' = "BCFiles"
  /\ files' = files \cup CParts
  /\ UNCHANGED <<live, catalog, intent, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

PersistBC ==
  /\ live /\ SelectedB /\ stage = "BCFiles"
  /\ IntentTemp \notin files
  /\ IF RefuseWhilePending
       THEN (intent = "none" \/ OverwritePendingIntent)
       ELSE intent \in {"none", "AB"}
  /\ intent' = IF intent = "AB" /\ ~OverwritePendingIntent THEN "AB" ELSE "BC"
  /\ stage' = "BCIntent"
  /\ UNCHANGED <<live, catalog, files, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

SelectBC ==
  /\ live /\ SelectedB /\ stage = "BCIntent" /\ intent \in {"AB", "BC"}
  /\ catalog' = "C"
  /\ stage' = "BCSelected"
  /\ pinnedCheckpoint' = (pinnedCheckpoint \/
       (pendingAdvance /\ intent = "AB" /\ "A" \in pins /\ OldPartsRemain))
  /\ UNCHANGED <<live, intent, files, pins, retired,
                 ackAB, ackBC, pendingAdvance, restartedPending, retryWitness>>

AckBC ==
  /\ live /\ catalog = "C" /\ stage = "BCSelected"
  /\ stage' = "idle"
  /\ ackBC' = TRUE
  /\ UNCHANGED <<live, catalog, intent, files, pins, retired,
                 ackAB, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

AdvanceC ==
  /\ live /\ catalog = "C" /\ stage = "idle"
  /\ catalog' = "C1"
  /\ intent' = IF DropOnOrdinaryAdvance THEN "none" ELSE intent
  /\ UNCHANGED <<live, stage, files, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

RetireA ==
  /\ live /\ (SelectedB \/ SelectedC) /\ intent = "AB"
  /\ "A" \notin pins /\ "A" \notin retired
  /\ retired' = retired \cup {"A"}
  /\ UNCHANGED <<live, catalog, intent, stage, files, pins,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

DeleteAPart(part) ==
  /\ live /\ (SelectedB \/ SelectedC) /\ intent = "AB" /\ "A" \in retired
  /\ part \in AParts \cap files
  /\ files' = files \ {part}
  /\ UNCHANGED <<live, catalog, intent, stage, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

SyncAUnlinks ==
  /\ live /\ (SelectedB \/ SelectedC) /\ intent = "AB" /\ "A" \in retired
  /\ AParts \cap files = {} /\ "aUnlinksSynced" \notin retired
  /\ retired' = retired \cup {"aUnlinksSynced"}
  /\ UNCHANGED <<live, catalog, intent, stage, files, pins,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

FinishA ==
  /\ live /\ (SelectedB \/ SelectedC) /\ intent = "AB" /\ AParts \cap files = {}
  /\ "aUnlinksSynced" \in retired
  /\ SelectedC => (BParts \cap files = {} /\ "bUnlinksSynced" \in retired)
  /\ intent' = "none"
  /\ retryWitness' = (retryWitness \/ (pinnedCheckpoint /\ restartedPending))
  /\ UNCHANGED <<live, catalog, stage, files, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending>>

RetireB ==
  /\ live /\ SelectedC /\ intent \in {"AB", "BC"}
  /\ "B" \notin pins /\ "B" \notin retired
  /\ retired' = retired \cup {"B"}
  /\ UNCHANGED <<live, catalog, intent, stage, files, pins,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

DeleteBPart(part) ==
  /\ live /\ SelectedC /\ intent \in {"AB", "BC"} /\ "B" \in retired
  /\ part \in BParts \cap files
  /\ files' = files \ {part}
  /\ UNCHANGED <<live, catalog, intent, stage, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

SyncBUnlinks ==
  /\ live /\ SelectedC /\ intent \in {"AB", "BC"} /\ "B" \in retired
  /\ BParts \cap files = {} /\ "bUnlinksSynced" \notin retired
  /\ retired' = retired \cup {"bUnlinksSynced"}
  /\ UNCHANGED <<live, catalog, intent, stage, files, pins,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

FinishB ==
  /\ live /\ SelectedC /\ intent = "BC" /\ BParts \cap files = {}
  /\ "bUnlinksSynced" \in retired
  /\ intent' = "none"
  /\ UNCHANGED <<live, catalog, stage, files, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, restartedPending,
                 retryWitness>>

Crash ==
  /\ live
  /\ live' = FALSE
  /\ stage' = "idle"
  /\ pins' = {}
  /\ retired' = {}
  /\ UNCHANGED <<catalog, intent, files, ackAB, ackBC, pinnedCheckpoint,
                 pendingAdvance, restartedPending, retryWitness>>

TearIntent ==
  /\ live
  /\ \/ stage = "ABFiles" /\ intent = "none"
     \/ stage = "BCFiles" /\ (intent = "none" \/ OverwritePendingIntent)
  /\ live' = FALSE
  /\ stage' = "idle"
  /\ pins' = {}
  /\ retired' = {}
  /\ IF AtomicIntentPublication
       THEN /\ files' = files \cup {IntentTemp}
            /\ UNCHANGED intent
       ELSE /\ intent' = "torn"
            /\ UNCHANGED files
  /\ UNCHANGED <<catalog, ackAB, ackBC, pinnedCheckpoint,
                 pendingAdvance, restartedPending, retryWitness>>

Recover ==
  /\ ~live
  /\ intent # "torn"
  /\ live' = TRUE
  /\ files' = IF RemoveIntentTempOnRecover THEN files \ {IntentTemp} ELSE files
  /\ intent' = IF (catalog = "A") \/ (SelectedB /\ intent = "BC")
                   THEN "none" ELSE intent
  /\ restartedPending' = (restartedPending \/
       (SelectedC /\ intent = "AB" /\ OldPartsRemain /\ pinnedCheckpoint))
  /\ UNCHANGED <<catalog, stage, pins, retired,
                 ackAB, ackBC, pinnedCheckpoint, pendingAdvance, retryWitness>>

Next ==
  AcquireA \/ AcquireB \/ (\E g \in Generations : Release(g)) \/
  CleanOrphanB \/ CleanOrphanC \/ StageAB \/ PersistAB \/ SelectAB \/ AckAB \/
  AdvanceB \/ StageBC \/ PersistBC \/ SelectBC \/
  AckBC \/ AdvanceC \/ RetireA \/
  (\E part \in AParts : DeleteAPart(part)) \/ SyncAUnlinks \/ FinishA \/
  RetireB \/ (\E part \in BParts : DeleteBPart(part)) \/
  SyncBUnlinks \/ FinishB \/
  Crash \/ TearIntent \/ Recover

Spec == Init /\ [][Next]_vars

TypeInvariant ==
  /\ live \in BOOLEAN /\ catalog \in Catalogs /\ intent \in Intents
  /\ stage \in Stages /\ files \subseteq Parts
  /\ pins \subseteq Generations
  /\ retired \subseteq (Generations \cup SyncMarkers)
  /\ ackAB \in BOOLEAN /\ ackBC \in BOOLEAN
  /\ pinnedCheckpoint \in BOOLEAN
  /\ pendingAdvance \in BOOLEAN /\ restartedPending \in BOOLEAN
  /\ retryWitness \in BOOLEAN

CurrentAuthorityProtected ==
  /\ (catalog = "A" => AParts \subseteq files)
  /\ (SelectedB => BParts \subseteq files)
  /\ (SelectedC => CParts \subseteq files)

SelectedDeletionObligationCovered ==
  /\ ((SelectedB \/ SelectedC) /\ OldPartsRemain => intent = "AB")
  /\ (SelectedC /\ MidPartsRemain => intent \in {"AB", "BC"})

PendingIntentDoesNotBlockCheckpoint ==
  (live /\ SelectedB /\ stage = "idle" /\ intent = "AB" /\ OldPartsRemain /\
   CParts \cap files = {}) => ENABLED StageBC

NoReaderRetired == pins \cap retired = {}
NoSecondAcknowledgementWhileOldPending ==
  ackBC => ~OldPartsRemain

NoRetryAfterPendingCheckpointAndRestart == ~retryWitness
NoMidReclaimWhileOldPinned ==
  ~(SelectedC /\ "A" \in pins /\ OldPartsRemain /\ BParts \cap files = {})
RecoveryAvailable == ~live => ENABLED Recover
TornIntentNeverPublished == intent # "torn"
IntentTempNeverLive == live => IntentTemp \notin files

NoPreCasRecoveryOrphan ==
  ~(live /\ catalog = "A" /\ stage = "idle" /\ intent = "none" /\
    BParts \subseteq files)
=============================================================================
