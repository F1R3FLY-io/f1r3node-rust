--------------------- MODULE DeployLifecycleFinalization ---------------------
EXTENDS Naturals

CONSTANT
  \* @type: Bool;
  RunOnFloorCommit,
  \* @type: Bool;
  UseFloorAsOccurrenceAnchor,
  \* @type: Bool;
  UseFinalityMarkerForSettlement,
  \* @type: Bool;
  UseFrozenFloorForSettlement

ASSUME
  /\ RunOnFloorCommit \in BOOLEAN
  /\ UseFloorAsOccurrenceAnchor \in BOOLEAN
  /\ UseFinalityMarkerForSettlement \in BOOLEAN
  /\ UseFrozenFloorForSettlement \in BOOLEAN

VARIABLES
  \* @type: Str;
  occurrence,
  \* @type: Bool;
  finalityMarked,
  \* @type: Bool;
  frozenFloorCoversCarrier,
  \* @type: Bool;
  floorCommitted,
  \* @type: Bool;
  effectInCommittedLfb,
  \* @type: Bool;
  readable,
  \* @type: Bool;
  beyondBound,
  \* @type: Str;
  terminal,
  \* @type: Bool;
  effectDone,
  \* @type: Bool;
  poolContains,
  \* @type: Bool;
  laterAdmission,
  \* @type: Bool;
  running,
  \* @type: Bool;
  crashed,
  \* @type: Int;
  terminalWrites,
  \* @type: Str;
  reportedOccurrenceAnchor,
  \* @type: Str;
  reportedStateAnchor

\* @type: <<Str, Bool, Bool, Bool, Bool, Bool, Bool, Str, Bool, Bool, Bool, Bool, Bool, Int, Str, Str>>;
vars ==
  <<occurrence, finalityMarked, frozenFloorCoversCarrier, floorCommitted,
    effectInCommittedLfb, readable, beyondBound, terminal, effectDone,
    poolContains, laterAdmission, running, crashed, terminalWrites,
    reportedOccurrenceAnchor, reportedStateAnchor>>

OccurrenceStates == {"None", "Succeeded", "Failed", "Absent"}
TerminalStates == {"Pending", "Finalized", "Failed", "Expired"}

SettlementEvidence ==
  \/ effectInCommittedLfb
  \/ UseFinalityMarkerForSettlement /\ finalityMarked
  \/ UseFrozenFloorForSettlement /\ frozenFloorCoversCarrier

Decision ==
  IF occurrence = "Succeeded" /\ SettlementEvidence THEN "Finalized"
  ELSE IF ~readable THEN "Pending"
  ELSE IF occurrence = "Failed" /\ SettlementEvidence THEN "Failed"
  ELSE IF beyondBound THEN "Expired"
  ELSE "Pending"

Init ==
  /\ occurrence = "None"
  /\ finalityMarked = FALSE
  /\ frozenFloorCoversCarrier = FALSE
  /\ floorCommitted = FALSE
  /\ effectInCommittedLfb = FALSE
  /\ readable = TRUE
  /\ beyondBound = FALSE
  /\ terminal = "Pending"
  /\ effectDone = FALSE
  /\ poolContains = TRUE
  /\ laterAdmission = FALSE
  /\ running = TRUE
  /\ crashed = FALSE
  /\ terminalWrites = 0
  /\ reportedOccurrenceAnchor = "None"
  /\ reportedStateAnchor = "None"

ObserveOccurrence(kind) ==
  /\ occurrence = "None"
  /\ kind \in {"Succeeded", "Failed", "Absent"}
  /\ occurrence' = kind
  /\ UNCHANGED <<finalityMarked, frozenFloorCoversCarrier, floorCommitted,
                  effectInCommittedLfb, readable, beyondBound, terminal,
                  effectDone, poolContains, laterAdmission, running, crashed,
                  terminalWrites, reportedOccurrenceAnchor, reportedStateAnchor>>

ObserveAnyOccurrence ==
  \E kind \in {"Succeeded", "Failed", "Absent"} : ObserveOccurrence(kind)

MarkFinalized ==
  /\ occurrence # "None"
  /\ ~finalityMarked
  /\ finalityMarked' = TRUE
  /\ UNCHANGED <<occurrence, frozenFloorCoversCarrier, floorCommitted,
                  effectInCommittedLfb, readable, beyondBound, terminal,
                  effectDone, poolContains, laterAdmission, running, crashed,
                  terminalWrites, reportedOccurrenceAnchor, reportedStateAnchor>>

ObserveFrozenFloorCoverage ==
  /\ occurrence # "None"
  /\ ~frozenFloorCoversCarrier
  /\ frozenFloorCoversCarrier' = TRUE
  /\ UNCHANGED <<occurrence, finalityMarked, floorCommitted,
                  effectInCommittedLfb, readable, beyondBound, terminal,
                  effectDone, poolContains, laterAdmission, running, crashed,
                  terminalWrites, reportedOccurrenceAnchor, reportedStateAnchor>>

LoseHistory ==
  /\ ~floorCommitted
  /\ readable
  /\ readable' = FALSE
  /\ UNCHANGED <<occurrence, finalityMarked, frozenFloorCoversCarrier,
                  floorCommitted, effectInCommittedLfb, beyondBound, terminal,
                  effectDone, poolContains, laterAdmission, running, crashed,
                  terminalWrites, reportedOccurrenceAnchor, reportedStateAnchor>>

CrossExpiryBound ==
  /\ ~beyondBound
  /\ beyondBound' = TRUE
  /\ UNCHANGED <<occurrence, finalityMarked, frozenFloorCoversCarrier,
                  floorCommitted, effectInCommittedLfb, readable, terminal,
                  effectDone, poolContains, laterAdmission, running, crashed,
                  terminalWrites, reportedOccurrenceAnchor, reportedStateAnchor>>

CommitFloor(effectPresent) ==
  /\ occurrence # "None"
  /\ ~floorCommitted
  /\ effectPresent \in BOOLEAN
  /\ floorCommitted' = TRUE
  /\ effectInCommittedLfb' =
       effectPresent /\ occurrence \in {"Succeeded", "Failed"}
  /\ UNCHANGED <<occurrence, finalityMarked, frozenFloorCoversCarrier,
                  readable, beyondBound, terminal, effectDone, poolContains,
                  laterAdmission, running, crashed, terminalWrites,
                  reportedOccurrenceAnchor, reportedStateAnchor>>

CommitAnyFloor == \E effectPresent \in BOOLEAN : CommitFloor(effectPresent)

CrashBeforeEffect ==
  /\ floorCommitted
  /\ ~effectDone
  /\ running
  /\ ~crashed
  /\ running' = FALSE
  /\ crashed' = TRUE
  /\ UNCHANGED <<occurrence, finalityMarked, frozenFloorCoversCarrier,
                  floorCommitted, effectInCommittedLfb, readable, beyondBound,
                  terminal, effectDone, poolContains, laterAdmission,
                  terminalWrites, reportedOccurrenceAnchor, reportedStateAnchor>>

Restart ==
  /\ ~running
  /\ running' = TRUE
  /\ UNCHANGED <<occurrence, finalityMarked, frozenFloorCoversCarrier,
                  floorCommitted, effectInCommittedLfb, readable, beyondBound,
                  terminal, effectDone, poolContains, laterAdmission, crashed,
                  terminalWrites, reportedOccurrenceAnchor, reportedStateAnchor>>

AdmitLaterBlock ==
  /\ floorCommitted
  /\ ~laterAdmission
  /\ laterAdmission' = TRUE
  /\ UNCHANGED <<occurrence, finalityMarked, frozenFloorCoversCarrier,
                  floorCommitted, effectInCommittedLfb, readable, beyondBound,
                  terminal, effectDone, poolContains, running, crashed,
                  terminalWrites, reportedOccurrenceAnchor, reportedStateAnchor>>

LifecycleEffect ==
  /\ floorCommitted
  /\ running
  /\ ~effectDone
  /\ (RunOnFloorCommit \/ laterAdmission)
  /\ LET decision == Decision IN
       /\ terminal' = decision
       /\ effectDone' = TRUE
       /\ poolContains' = IF decision = "Pending" THEN poolContains ELSE FALSE
       /\ terminalWrites' =
            IF decision = "Pending" THEN terminalWrites ELSE terminalWrites + 1
       /\ reportedOccurrenceAnchor' =
            IF decision \in {"Finalized", "Failed"}
            THEN IF UseFloorAsOccurrenceAnchor THEN "Floor" ELSE "Carrier"
            ELSE "None"
       /\ reportedStateAnchor' =
            IF decision = "Pending" THEN "None" ELSE "Floor"
  /\ UNCHANGED <<occurrence, finalityMarked, frozenFloorCoversCarrier,
                  floorCommitted, effectInCommittedLfb, readable, beyondBound,
                  laterAdmission, running, crashed>>

Next ==
  \/ ObserveAnyOccurrence
  \/ MarkFinalized
  \/ ObserveFrozenFloorCoverage
  \/ LoseHistory
  \/ CrossExpiryBound
  \/ CommitAnyFloor
  \/ CrashBeforeEffect
  \/ Restart
  \/ AdmitLaterBlock
  \/ LifecycleEffect

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(Restart)
  /\ WF_vars(LifecycleEffect)

TypeOK ==
  /\ occurrence \in OccurrenceStates
  /\ finalityMarked \in BOOLEAN
  /\ frozenFloorCoversCarrier \in BOOLEAN
  /\ floorCommitted \in BOOLEAN
  /\ effectInCommittedLfb \in BOOLEAN
  /\ readable \in BOOLEAN
  /\ beyondBound \in BOOLEAN
  /\ terminal \in TerminalStates
  /\ effectDone \in BOOLEAN
  /\ poolContains \in BOOLEAN
  /\ laterAdmission \in BOOLEAN
  /\ running \in BOOLEAN
  /\ crashed \in BOOLEAN
  /\ terminalWrites \in 0..1
  /\ reportedOccurrenceAnchor \in {"None", "Carrier", "Floor"}
  /\ reportedStateAnchor \in {"None", "Floor"}

Inv_NoTerminalBeforeFloor == terminal # "Pending" => floorCommitted

Inv_FinalizedHasSuccessfulFloorEffect ==
  terminal = "Finalized" => occurrence = "Succeeded" /\ effectInCommittedLfb

Inv_FailedHasReadableAdoptedFailure ==
  terminal = "Failed" =>
    occurrence = "Failed" /\ readable /\ effectInCommittedLfb

Inv_ExpiredHasReadableStableAbsence ==
  terminal = "Expired" => readable /\ beyondBound /\ ~effectInCommittedLfb

Inv_TerminalWriteOnce == terminalWrites <= 1

Inv_TerminalEffectCleansPool ==
  effectDone /\ terminal # "Pending" => ~poolContains

Inv_UnsettledOccurrenceRetainsPool ==
  floorCommitted /\ ~effectInCommittedLfb /\ ~beyondBound
    /\ occurrence \in {"Succeeded", "Failed"} =>
      terminal = "Pending" /\ poolContains

Inv_CommittedEffectSettlesAfterRun ==
  floorCommitted /\ effectInCommittedLfb /\ readable /\ effectDone =>
    \/ occurrence = "Succeeded" /\ terminal = "Finalized"
    \/ occurrence = "Failed" /\ terminal = "Failed"

Inv_CommittedDecisionHasLocalTrigger ==
  floorCommitted /\ effectInCommittedLfb /\ running /\ ~effectDone =>
    RunOnFloorCommit \/ laterAdmission

Inv_TerminalStateUsesFloorAnchor ==
  terminal # "Pending" => reportedStateAnchor = "Floor"

Inv_TerminalOccurrenceUsesCarrier ==
  terminal \in {"Finalized", "Failed"} => reportedOccurrenceAnchor = "Carrier"

Inv_StateFloorIsNeverAnOccurrenceCarrier ==
  reportedOccurrenceAnchor # "Floor"

Live_CommittedOccurrenceTerminalizes ==
  (floorCommitted /\ effectInCommittedLfb /\ readable) ~>
    terminal # "Pending"

=============================================================================
