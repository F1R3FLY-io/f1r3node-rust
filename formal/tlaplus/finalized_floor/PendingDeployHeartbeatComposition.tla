---------------- MODULE PendingDeployHeartbeatComposition ----------------
EXTENDS Naturals, FiniteSets

CONSTANTS
  \* @type: Int;
  NumValidators,
  \* @type: Int;
  NumDeploys,
  \* @type: Set(Int);
  OnlineValidators,
  \* @type: Int;
  MaxRound,
  \* @type: Int;
  MaxIngressPerPair,
  \* @type: Int;
  QueueBound,
  \* @type: Int;
  OrdinaryAttemptBound,
  \* @type: Int;
  SearchAttemptBound,
  \* @type: Int;
  DuplicateBound,
  \* @type: Int;
  MaxOccurrences,
  \* @type: Int;
  MaxTransientFailures,
  \* @type: Bool;
  PendingMasksRecovery,
  \* @type: Bool;
  ReserveRecovery,
  \* @type: Bool;
  AttemptClosesRound,
  \* @type: Bool;
  ClearOnStart,
  \* @type: Bool;
  RotateRecovery,
  \* @type: Bool;
  BoundDuplicateAdmission,
  \* @type: Bool;
  UseFinalizedCommittee,
  \* @type: Bool;
  UseFinalizedEligibility,
  \* @type: Bool;
  UseFinalizedJustifications

Validators == 1..NumValidators
Deploys == 1..NumDeploys
Rounds == 0..MaxRound
NoValidator == 0
NoDeploy == 0
NoRound == MaxRound + 1
Phases == {"Idle", "Queued", "Started"}
WorkKinds == {"None", "Ordinary", "PendingRecovery", "Recovery"}
Outcomes == {"None", "Empty", "Deferred", "Failed", "Started", "Success"}
RetryableOutcomes == {"Empty", "Deferred", "Failed"}
OccurrenceStatuses == {"Absent", "Undecided", "Winner", "Loser"}

ASSUME /\ NumValidators = 3
       /\ NumDeploys >= 1
       /\ OnlineValidators = {2, 3}
       /\ MaxRound >= 2
       /\ MaxIngressPerPair \in Nat
       /\ QueueBound = 1
       /\ OrdinaryAttemptBound > MaxTransientFailures
       /\ SearchAttemptBound > OrdinaryAttemptBound
       /\ DuplicateBound > 0
       /\ MaxOccurrences > DuplicateBound
       /\ MaxTransientFailures >= 0
       /\ PendingMasksRecovery \in BOOLEAN
       /\ ReserveRecovery \in BOOLEAN
       /\ AttemptClosesRound \in BOOLEAN
       /\ ClearOnStart \in BOOLEAN
       /\ RotateRecovery \in BOOLEAN
       /\ BoundDuplicateAdmission \in BOOLEAN
       /\ UseFinalizedCommittee \in BOOLEAN
       /\ UseFinalizedEligibility \in BOOLEAN
       /\ UseFinalizedJustifications \in BOOLEAN

(*
  @typeAlias: systemState = {
      pool: Int -> Set(Int),
      ingressCount: Int -> (Int -> Int),
      phase: Int -> Str,
      workKind: Int -> Str,
      workDeploy: Int -> Int,
      workRound: Int -> Int,
      lastOutcome: Int -> Str,
      transientFailures: Int -> Int,
      ordinaryAttempts: Int -> (Int -> Int),
      lfbHeightMod: Int,
      finalizedCommittee: Set(Int),
      headCommitteeMode: Str,
      nextOccurrence: Int,
      occValidator: Int -> Int,
      occDeploy: Int -> Int,
      occStatus: Int -> Str,
      completedRounds: Int -> Set(Int),
      successfulRounds: Int -> Set(Int),
      skippedRounds: Int -> Set(Int),
      nextRound: Int -> Int,
      recoverySupport: Set(Int),
      floorCertified: Bool,
      floorObserved: Int -> Bool,
      terminalDeploys: Set(Int),
      terminalEvidence: Int -> Set(Int),
      removedFromPool: Int -> Set(Int),
      ordinaryDuringRecovery: Set(Int)
  };
*)
module_typedefs == TRUE

VARIABLE
  \* @type: $systemState;
  state

\* @type: <<$systemState>>;
vars == <<state>>

ProducedOccurrences ==
  {occurrence \in 1..MaxOccurrences : occurrence < state.nextOccurrence}

OccurrencesOf(deploy) ==
  {occurrence \in ProducedOccurrences :
    state.occDeploy[occurrence] = deploy}

MinimumOccurrence(occurrences) ==
  CHOOSE occurrence \in occurrences :
    \A other \in occurrences : occurrence <= other

InFlightOrdinary(deploy) ==
  {validator \in OnlineValidators :
    /\ state.phase[validator] # "Idle"
    /\ state.workKind[validator] \in {"Ordinary", "PendingRecovery"}
    /\ state.workDeploy[validator] = deploy}

ReservedOccurrenceCount(deploy) ==
  Cardinality(OccurrencesOf(deploy)) + Cardinality(InFlightOrdinary(deploy))

TotalInFlightOrdinary ==
  Cardinality(
    {validator \in OnlineValidators :
      /\ state.phase[validator] # "Idle"
      /\ state.workKind[validator] = "Ordinary"})

\* @type: (Set(Int), Int) => Int;
LeaderFromCommittee(committee, round) ==
  LET leaderIndex ==
        ((state.lfbHeightMod + round) % Cardinality(committee)) + 1
  IN CHOOSE validator \in committee :
       Cardinality({other \in committee : other <= validator}) = leaderIndex

\* @type: Int => Set(Int);
HeadCommitteeOf(validator) ==
  IF state.headCommitteeMode = "Aligned"
  THEN state.finalizedCommittee
  ELSE IF state.headCommitteeMode = "SelfSelected"
  THEN IF validator = 2 THEN {2}
       ELSE IF validator = 3 THEN {3}
       ELSE state.finalizedCommittee
  ELSE IF validator = 2 THEN {3}
  ELSE IF validator = 3 THEN {2}
  ELSE state.finalizedCommittee

\* @type: Int => Set(Int);
RecoveryCommitteeOf(validator) ==
  IF UseFinalizedCommittee
  THEN state.finalizedCommittee
  ELSE HeadCommitteeOf(validator)

\* @type: Int => Set(Int);
EligibilityCommitteeOf(validator) ==
  IF UseFinalizedEligibility
  THEN state.finalizedCommittee
  ELSE HeadCommitteeOf(validator)

RecoveryEligible(validator) == validator \in EligibilityCommitteeOf(validator)

\* @type: Int => Set(Int);
JustificationCommitteeOf(validator) ==
  IF UseFinalizedJustifications
  THEN state.finalizedCommittee
  ELSE HeadCommitteeOf(validator)

CreatorJustificationVisible(validator) ==
  /\ validator \in state.finalizedCommittee
  /\ validator \in JustificationCommitteeOf(validator)

CreatorSequenceAvailable(validator) == CreatorJustificationVisible(validator)

RecoveryCanStartAndValidate(validator) ==
  /\ RecoveryEligible(validator)
  /\ CreatorJustificationVisible(validator)
  /\ CreatorSequenceAvailable(validator)

\* @type: (Int, Int) => Int;
RecoveryLeaderAt(validator, round) ==
  IF RotateRecovery
  THEN LeaderFromCommittee(RecoveryCommitteeOf(validator), round)
  ELSE 1

RecoveryDue(validator) ==
  /\ validator \in OnlineValidators
  /\ ~state.floorObserved[validator]
  /\ state.nextRound[validator] <= MaxRound

SelectedRecovery(validator) ==
  /\ RecoveryDue(validator)
  /\ RecoveryLeaderAt(validator, state.nextRound[validator]) = validator

RecoveryReservation(validator) ==
  /\ ReserveRecovery
  /\ SelectedRecovery(validator)

OrdinaryLimit ==
  IF BoundDuplicateAdmission
  THEN OrdinaryAttemptBound
  ELSE SearchAttemptBound

OccurrenceCapacityAvailable ==
  state.nextOccurrence - 1 + TotalInFlightOrdinary < MaxOccurrences

AdmissiblePending(validator) ==
  {deploy \in state.pool[validator] :
    /\ deploy \notin state.terminalDeploys
    /\ state.ordinaryAttempts[validator][deploy] < OrdinaryLimit
    /\ OccurrenceCapacityAvailable
    /\ (~BoundDuplicateAdmission
         \/ ReservedOccurrenceCount(deploy) < DuplicateBound)}

Init ==
  \E initialLfbHeightMod \in 0..(NumValidators - 1) :
    state =
    [lfbHeightMod |-> initialLfbHeightMod,
     finalizedCommittee |-> Validators,
     headCommitteeMode |-> "Aligned",
     pool |->
       [validator \in Validators |->
         IF validator = 2 THEN {1}
         ELSE IF validator = 3 THEN {NumDeploys}
         ELSE {}],
     ingressCount |->
       [validator \in Validators |-> [deploy \in Deploys |-> 0]],
     phase |-> [validator \in Validators |-> "Idle"],
     workKind |-> [validator \in Validators |-> "None"],
     workDeploy |-> [validator \in Validators |-> NoDeploy],
     workRound |-> [validator \in Validators |-> NoRound],
     lastOutcome |-> [validator \in Validators |-> "None"],
     transientFailures |-> [validator \in Validators |-> 0],
     ordinaryAttempts |->
       [validator \in Validators |-> [deploy \in Deploys |-> 0]],
     nextOccurrence |-> 1,
     occValidator |->
       [occurrence \in 1..MaxOccurrences |-> NoValidator],
     occDeploy |-> [occurrence \in 1..MaxOccurrences |-> NoDeploy],
     occStatus |->
       [occurrence \in 1..MaxOccurrences |-> "Absent"],
     completedRounds |-> [validator \in Validators |-> {}],
     successfulRounds |-> [validator \in Validators |-> {}],
     skippedRounds |-> [validator \in Validators |-> {}],
     nextRound |-> [validator \in Validators |-> 0],
     recoverySupport |-> {},
     floorCertified |-> FALSE,
     floorObserved |-> [validator \in Validators |-> FALSE],
     terminalDeploys |-> {},
     terminalEvidence |-> [validator \in Validators |-> {}],
     removedFromPool |-> [validator \in Validators |-> {}],
     ordinaryDuringRecovery |-> {}]

Submit(validator, deploy) ==
  /\ validator \in OnlineValidators
  /\ deploy \in Deploys
  /\ state.ingressCount[validator][deploy] < MaxIngressPerPair
  /\ deploy \notin state.terminalDeploys
  /\ state' =
       [state EXCEPT
         !.pool[validator] = @ \union {deploy},
         !.ingressCount[validator][deploy] = @ + 1]

QueueOrdinary(validator, deploy) ==
  /\ validator \in OnlineValidators
  /\ deploy \in state.pool[validator]
  /\ deploy \notin state.terminalDeploys
  /\ state.phase[validator] = "Idle"
  /\ state.ordinaryAttempts[validator][deploy] < OrdinaryLimit
  /\ OccurrenceCapacityAvailable
  /\ (~BoundDuplicateAdmission
       \/ ReservedOccurrenceCount(deploy) < DuplicateBound)
  /\ ~RecoveryReservation(validator)
  /\ state' =
       [state EXCEPT
         !.phase[validator] = "Queued",
         !.workKind[validator] = "Ordinary",
         !.workDeploy[validator] = deploy,
         !.workRound[validator] = NoRound,
         !.lastOutcome[validator] = "None",
         !.ordinaryAttempts[validator][deploy] = @ + 1,
         !.ordinaryDuringRecovery =
           IF SelectedRecovery(validator)
           THEN @ \union {validator}
           ELSE @]

QueueRecoveryWithDeploy(validator, deploy) ==
  /\ SelectedRecovery(validator)
  /\ RecoveryCanStartAndValidate(validator)
  /\ state.phase[validator] = "Idle"
  /\ (~PendingMasksRecovery \/ state.pool[validator] = {})
  /\ deploy \in AdmissiblePending(validator)
  /\ state' =
       [state EXCEPT
         !.phase[validator] = "Queued",
         !.workKind[validator] = "PendingRecovery",
         !.workDeploy[validator] = deploy,
         !.workRound[validator] = state.nextRound[validator],
         !.lastOutcome[validator] = "None",
         !.ordinaryAttempts[validator][deploy] = @ + 1]

QueueEmptyRecovery(validator) ==
  /\ SelectedRecovery(validator)
  /\ RecoveryCanStartAndValidate(validator)
  /\ state.phase[validator] = "Idle"
  /\ (~PendingMasksRecovery \/ state.pool[validator] = {})
  /\ AdmissiblePending(validator) = {}
  /\ state' =
       [state EXCEPT
         !.phase[validator] = "Queued",
         !.workKind[validator] = "Recovery",
         !.workDeploy[validator] = NoDeploy,
         !.workRound[validator] = state.nextRound[validator],
         !.lastOutcome[validator] = "None"]

SkipRecovery(validator) ==
  /\ RecoveryDue(validator)
  /\ RecoveryLeaderAt(validator, state.nextRound[validator]) # validator
  /\ state.phase[validator] = "Idle"
  /\ LET round == state.nextRound[validator]
     IN state' =
          [state EXCEPT
            !.completedRounds[validator] = @ \union {round},
            !.skippedRounds[validator] = @ \union {round},
            !.nextRound[validator] = @ + 1]

ReportRetryable(validator, outcome) ==
  /\ validator \in OnlineValidators
  /\ outcome \in RetryableOutcomes
  /\ state.phase[validator] = "Queued"
  /\ state.transientFailures[validator] < MaxTransientFailures
  /\ LET recovery ==
           state.workKind[validator] \in {"PendingRecovery", "Recovery"}
         round == state.workRound[validator]
     IN state' =
          [state EXCEPT
            !.phase[validator] = "Idle",
            !.workKind[validator] = "None",
            !.workDeploy[validator] = NoDeploy,
            !.workRound[validator] = NoRound,
            !.lastOutcome[validator] = outcome,
            !.transientFailures[validator] = @ + 1,
            !.completedRounds[validator] =
              IF recovery /\ AttemptClosesRound
              THEN @ \union {round}
              ELSE @,
            !.nextRound[validator] =
              IF recovery /\ AttemptClosesRound THEN @ + 1 ELSE @]

ReportStarted(validator) ==
  /\ validator \in OnlineValidators
  /\ state.phase[validator] = "Queued"
  /\ LET recovery ==
           state.workKind[validator] \in {"PendingRecovery", "Recovery"}
         ordinary ==
           state.workKind[validator] \in {"Ordinary", "PendingRecovery"}
         deploy == state.workDeploy[validator]
         round == state.workRound[validator]
     IN /\ ~recovery \/ RecoveryCanStartAndValidate(validator)
        /\ state' =
          [state EXCEPT
            !.phase[validator] = "Started",
            !.lastOutcome[validator] = "Started",
            !.completedRounds[validator] =
              IF recovery THEN @ \union {round} ELSE @,
            !.successfulRounds[validator] =
              IF recovery THEN @ \union {round} ELSE @,
            !.nextRound[validator] = IF recovery THEN @ + 1 ELSE @,
            !.pool[validator] =
              IF ordinary /\ ClearOnStart THEN @ \ {deploy} ELSE @,
            !.removedFromPool[validator] =
              IF ordinary /\ ClearOnStart
              THEN @ \union {deploy}
              ELSE @]

PublishQueuedSuccess(validator) ==
  /\ validator \in OnlineValidators
  /\ state.phase[validator] = "Queued"
  /\ LET recovery ==
           state.workKind[validator] \in {"PendingRecovery", "Recovery"}
         ordinary ==
           state.workKind[validator] \in {"Ordinary", "PendingRecovery"}
         deploy == state.workDeploy[validator]
         round == state.workRound[validator]
         occurrence == state.nextOccurrence
     IN /\ ~recovery \/ RecoveryCanStartAndValidate(validator)
        /\ ~ordinary \/ occurrence <= MaxOccurrences
        /\ state' =
             IF ordinary
             THEN [state EXCEPT
                    !.phase[validator] = "Idle",
                    !.workKind[validator] = "None",
                    !.workDeploy[validator] = NoDeploy,
                    !.workRound[validator] = NoRound,
                    !.lastOutcome[validator] = "Success",
                    !.completedRounds[validator] =
                      IF recovery THEN @ \union {round} ELSE @,
                    !.successfulRounds[validator] =
                      IF recovery THEN @ \union {round} ELSE @,
                    !.nextRound[validator] = IF recovery THEN @ + 1 ELSE @,
                    !.recoverySupport =
                      IF recovery THEN @ \union {validator} ELSE @,
                    !.nextOccurrence = @ + 1,
                    !.occValidator[occurrence] = validator,
                    !.occDeploy[occurrence] = deploy,
                    !.occStatus[occurrence] = "Undecided"]
             ELSE [state EXCEPT
                    !.phase[validator] = "Idle",
                    !.workKind[validator] = "None",
                    !.workDeploy[validator] = NoDeploy,
                    !.workRound[validator] = NoRound,
                    !.lastOutcome[validator] = "Success",
                    !.completedRounds[validator] = @ \union {round},
                    !.successfulRounds[validator] = @ \union {round},
                    !.nextRound[validator] = @ + 1,
                    !.recoverySupport = @ \union {validator}]

FinishStarted(validator) ==
  /\ validator \in OnlineValidators
  /\ state.phase[validator] = "Started"
  /\ LET recovery ==
           state.workKind[validator] \in {"PendingRecovery", "Recovery"}
         ordinary ==
           state.workKind[validator] \in {"Ordinary", "PendingRecovery"}
         deploy == state.workDeploy[validator]
         occurrence == state.nextOccurrence
     IN /\ ~recovery \/ RecoveryCanStartAndValidate(validator)
        /\ ~ordinary \/ occurrence <= MaxOccurrences
        /\ state' =
             IF ordinary
             THEN [state EXCEPT
                    !.phase[validator] = "Idle",
                    !.workKind[validator] = "None",
                    !.workDeploy[validator] = NoDeploy,
                    !.workRound[validator] = NoRound,
                    !.lastOutcome[validator] = "Success",
                    !.recoverySupport =
                      IF recovery THEN @ \union {validator} ELSE @,
                    !.nextOccurrence = @ + 1,
                    !.occValidator[occurrence] = validator,
                    !.occDeploy[occurrence] = deploy,
                    !.occStatus[occurrence] = "Undecided"]
             ELSE [state EXCEPT
                    !.phase[validator] = "Idle",
                    !.workKind[validator] = "None",
                    !.workDeploy[validator] = NoDeploy,
                    !.workRound[validator] = NoRound,
                    !.lastOutcome[validator] = "Success",
                    !.recoverySupport = @ \union {validator}]

ResolveDisposition(deploy) ==
  /\ deploy \in Deploys
  /\ OccurrencesOf(deploy) # {}
  /\ \E occurrence \in OccurrencesOf(deploy) :
       state.occStatus[occurrence] = "Undecided"
  /\ LET winner == MinimumOccurrence(OccurrencesOf(deploy))
     IN state' =
          [state EXCEPT
            !.occStatus =
              [occurrence \in 1..MaxOccurrences |->
                IF occurrence \in OccurrencesOf(deploy)
                THEN IF occurrence = winner THEN "Winner" ELSE "Loser"
                ELSE state.occStatus[occurrence]]]

CertifyFloor ==
  /\ ~state.floorCertified
  /\ 2 * Cardinality(state.recoverySupport) > Cardinality(OnlineValidators)
  /\ state' = [state EXCEPT !.floorCertified = TRUE]

ObserveFloor(validator) ==
  /\ validator \in OnlineValidators
  /\ state.floorCertified
  /\ ~state.floorObserved[validator]
  /\ state' = [state EXCEPT !.floorObserved[validator] = TRUE]

FinalizeDeploy(deploy) ==
  /\ deploy \in Deploys
  /\ state.floorCertified
  /\ deploy \notin state.terminalDeploys
  /\ OccurrencesOf(deploy) # {}
  /\ InFlightOrdinary(deploy) = {}
  /\ \A occurrence \in OccurrencesOf(deploy) :
       state.occStatus[occurrence] \in {"Winner", "Loser"}
  /\ state' =
       [state EXCEPT !.terminalDeploys = @ \union {deploy}]

ObserveTerminal(validator, deploy) ==
  /\ validator \in OnlineValidators
  /\ deploy \in state.terminalDeploys
  /\ deploy \notin state.terminalEvidence[validator]
  /\ state' =
       [state EXCEPT
         !.terminalEvidence[validator] = @ \union {deploy},
         !.pool[validator] = @ \ {deploy},
         !.removedFromPool[validator] = @ \union {deploy}]

DriftHeadCommitteeSelfSelected ==
  /\ state.headCommitteeMode = "Aligned"
  /\ state' = [state EXCEPT !.headCommitteeMode = "SelfSelected"]

DriftHeadCommitteeDisjoint ==
  /\ state.headCommitteeMode = "Aligned"
  /\ state' = [state EXCEPT !.headCommitteeMode = "Disjoint"]

SubmitAny ==
  \E validator \in Validators, deploy \in Deploys : Submit(validator, deploy)

QueueOrdinaryAny ==
  \E validator \in Validators, deploy \in Deploys :
    QueueOrdinary(validator, deploy)

QueueRecoveryAny ==
  \/ \E validator \in Validators, deploy \in Deploys :
       QueueRecoveryWithDeploy(validator, deploy)
  \/ \E validator \in Validators : QueueEmptyRecovery(validator)

SkipRecoveryAny ==
  \E validator \in Validators : SkipRecovery(validator)

ReportRetryableAny ==
  \E validator \in Validators, outcome \in RetryableOutcomes :
    ReportRetryable(validator, outcome)

ReportStartedAny ==
  \E validator \in Validators : ReportStarted(validator)

PublishQueuedSuccessAny ==
  \E validator \in Validators : PublishQueuedSuccess(validator)

FinishStartedAny ==
  \E validator \in Validators : FinishStarted(validator)

ResolveDispositionAny ==
  \E deploy \in Deploys : ResolveDisposition(deploy)

ObserveFloorAny ==
  \E validator \in Validators : ObserveFloor(validator)

FinalizeDeployAny ==
  \E deploy \in Deploys : FinalizeDeploy(deploy)

ObserveTerminalAny ==
  \E validator \in Validators, deploy \in Deploys :
    ObserveTerminal(validator, deploy)

Next ==
  \/ SubmitAny
  \/ QueueOrdinaryAny
  \/ QueueRecoveryAny
  \/ SkipRecoveryAny
  \/ ReportRetryableAny
  \/ ReportStartedAny
  \/ PublishQueuedSuccessAny
  \/ FinishStartedAny
  \/ ResolveDispositionAny
  \/ CertifyFloor
  \/ ObserveFloorAny
  \/ FinalizeDeployAny
  \/ ObserveTerminalAny
  \/ DriftHeadCommitteeSelfSelected
  \/ DriftHeadCommitteeDisjoint

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(SubmitAny)
  /\ WF_vars(QueueOrdinaryAny)
  /\ WF_vars(QueueRecoveryAny)
  /\ WF_vars(SkipRecoveryAny)
  /\ WF_vars(ReportRetryableAny)
  /\ WF_vars(ReportStartedAny)
  /\ WF_vars(PublishQueuedSuccessAny)
  /\ WF_vars(FinishStartedAny)
  /\ WF_vars(ResolveDispositionAny)
  /\ WF_vars(CertifyFloor)
  /\ WF_vars(ObserveFloorAny)
  /\ WF_vars(FinalizeDeployAny)
  /\ WF_vars(ObserveTerminalAny)

TypeOK ==
  /\ state.lfbHeightMod \in 0..(NumValidators - 1)
  /\ state.finalizedCommittee = Validators
  /\ state.headCommitteeMode \in {"Aligned", "SelfSelected", "Disjoint"}
  /\ state.pool \in [Validators -> SUBSET Deploys]
  /\ state.ingressCount \in
       [Validators -> [Deploys -> 0..MaxIngressPerPair]]
  /\ state.phase \in [Validators -> Phases]
  /\ state.workKind \in [Validators -> WorkKinds]
  /\ state.workDeploy \in [Validators -> (Deploys \union {NoDeploy})]
  /\ state.workRound \in [Validators -> (Rounds \union {NoRound})]
  /\ state.lastOutcome \in [Validators -> Outcomes]
  /\ state.transientFailures \in
       [Validators -> 0..MaxTransientFailures]
  /\ state.ordinaryAttempts \in
       [Validators -> [Deploys -> 0..SearchAttemptBound]]
  /\ state.nextOccurrence \in 1..(MaxOccurrences + 1)
  /\ state.occValidator \in
       [1..MaxOccurrences -> (Validators \union {NoValidator})]
  /\ state.occDeploy \in
       [1..MaxOccurrences -> (Deploys \union {NoDeploy})]
  /\ state.occStatus \in
       [1..MaxOccurrences -> OccurrenceStatuses]
  /\ state.completedRounds \in [Validators -> SUBSET Rounds]
  /\ state.successfulRounds \in [Validators -> SUBSET Rounds]
  /\ state.skippedRounds \in [Validators -> SUBSET Rounds]
  /\ state.nextRound \in [Validators -> 0..NoRound]
  /\ state.recoverySupport \subseteq OnlineValidators
  /\ state.floorCertified \in BOOLEAN
  /\ state.floorObserved \in [Validators -> BOOLEAN]
  /\ state.terminalDeploys \subseteq Deploys
  /\ state.terminalEvidence \in [Validators -> SUBSET Deploys]
  /\ state.removedFromPool \in [Validators -> SUBSET Deploys]
  /\ state.ordinaryDuringRecovery \subseteq OnlineValidators

Inv_QueueBounded ==
  \A validator \in Validators :
    (IF state.phase[validator] = "Idle" THEN 0 ELSE 1) <= QueueBound

Inv_OrdinaryAttemptsBounded ==
  \A validator \in Validators, deploy \in Deploys :
    state.ordinaryAttempts[validator][deploy] <= OrdinaryAttemptBound

Inv_DuplicateOccurrencesBounded ==
  \A deploy \in Deploys :
    Cardinality(OccurrencesOf(deploy)) <= DuplicateBound

Inv_DeterministicDisposition ==
  \A deploy \in Deploys :
    OccurrencesOf(deploy) = {}
    \/ LET winner == MinimumOccurrence(OccurrencesOf(deploy))
       IN \A occurrence \in OccurrencesOf(deploy) :
            /\ state.occStatus[occurrence] = "Winner"
               => occurrence = winner
            /\ state.occStatus[occurrence] = "Loser"
               => occurrence # winner

Inv_AtMostOneWinner ==
  \A deploy \in Deploys :
    Cardinality(
      {occurrence \in OccurrencesOf(deploy) :
        state.occStatus[occurrence] = "Winner"}) <= 1

Inv_TerminalHasDisposition ==
  \A deploy \in state.terminalDeploys :
    /\ OccurrencesOf(deploy) # {}
    /\ \A occurrence \in OccurrencesOf(deploy) :
         state.occStatus[occurrence] \in {"Winner", "Loser"}

Inv_PoolRemovalRequiresTerminalEvidence ==
  \A validator \in Validators :
    state.removedFromPool[validator]
      \subseteq state.terminalEvidence[validator]

Inv_RetryableOutcomeDoesNotCompleteRound ==
  \A validator \in Validators :
    state.completedRounds[validator]
      \subseteq
        state.successfulRounds[validator] \union state.skippedRounds[validator]

Inv_RecoveryReservationHonored ==
  state.ordinaryDuringRecovery = {}

Inv_TerminalEvidenceIsGlobal ==
  \A validator \in Validators :
    state.terminalEvidence[validator] \subseteq state.terminalDeploys

Inv_FinalizedCommitteeStable == state.finalizedCommittee = Validators

Inv_HeadCommitteeDriftCannotChangeRecoveryLeader ==
  UseFinalizedCommittee =>
    \A validator \in OnlineValidators, round \in Rounds :
      RecoveryLeaderAt(validator, round) =
        LeaderFromCommittee(state.finalizedCommittee, round)

Inv_AtMostOneSelectedRecoveryPerRound ==
  \A recoveryRound \in Rounds :
    Cardinality(
      {validator \in OnlineValidators :
        /\ state.nextRound[validator] = recoveryRound
        /\ RecoveryDue(validator)
        /\ RecoveryLeaderAt(validator, recoveryRound) = validator}) <= 1

Inv_SelectedRecoveryEligible ==
  \A validator \in OnlineValidators :
    SelectedRecovery(validator) => RecoveryEligible(validator)

Inv_SelectedRecoveryHasCreatorSequence ==
  \A validator \in OnlineValidators :
    SelectedRecovery(validator) =>
      /\ CreatorJustificationVisible(validator)
      /\ CreatorSequenceAvailable(validator)

Inv_QueuedRecoveryHasValidationContext ==
  \A validator \in OnlineValidators :
    (state.phase[validator] = "Queued"
      /\ state.workKind[validator] \in {"PendingRecovery", "Recovery"})
      => RecoveryCanStartAndValidate(validator)

SchedulerSemanticSafety ==
  /\ Inv_QueueBounded
  /\ Inv_OrdinaryAttemptsBounded
  /\ Inv_DuplicateOccurrencesBounded
  /\ Inv_DeterministicDisposition
  /\ Inv_AtMostOneWinner
  /\ Inv_TerminalHasDisposition
  /\ Inv_PoolRemovalRequiresTerminalEvidence
  /\ Inv_RetryableOutcomeDoesNotCompleteRound
  /\ Inv_RecoveryReservationHonored
  /\ Inv_TerminalEvidenceIsGlobal

RecoverySemanticSafety ==
  /\ Inv_FinalizedCommitteeStable
  /\ Inv_HeadCommitteeDriftCannotChangeRecoveryLeader
  /\ Inv_AtMostOneSelectedRecoveryPerRound
  /\ Inv_SelectedRecoveryEligible
  /\ Inv_SelectedRecoveryHasCreatorSequence
  /\ Inv_QueuedRecoveryHasValidationContext

SemanticSafety ==
  /\ SchedulerSemanticSafety
  /\ RecoverySemanticSafety

LivenessProjection == state.headCommitteeMode = "Aligned"

Live_FloorCertified == <>state.floorCertified

Live_OnlineValidatorsObserveFloor ==
  \A validator \in OnlineValidators : <>state.floorObserved[validator]

Live_TrackedDeploysBecomeTerminal ==
  \A validator \in OnlineValidators, deploy \in Deploys :
    (deploy \in state.pool[validator])
      ~> (deploy \in state.terminalEvidence[validator])
=============================================================================
