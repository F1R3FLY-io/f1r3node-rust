--------------------------- MODULE EndToEndCostConsensus ---------------------------
(***************************************************************************)
(* Rust refinement map:                                                  *)
(* Admit/RejectUnprovable -> acceptance::admit_by_funding_with_logic      *)
(* Commit/FinishExecution -> RuntimeBudget + RuntimeOps deploy execution   *)
(* Settle -> recompute_realized_settlement_debits + CloseBlockDeploy       *)
(* Replay/FinishReplay -> ReplayRuntimeOps replay and exact cost check      *)
(* LocalFault/Recover -> BlockStatus::disposition + block_processor         *)
(* ObjectiveInvalid -> validation_dispatcher invalid-block evidence         *)
(* ReorderParents/FinalityUsesDAGAncestry -> finalizer full-DAG traversal    *)
(* TreatLocalFaultAsInvalid is the defect knob. The unsafe config sets it    *)
(* TRUE and must violate LocalFaultNeverCreatesSlashEvidence.               *)
(* DR-134 (DR-66): genesis runs in the trusted install scope and records    *)
(* cost 0 (RuntimeOps::evaluate_trusted_install); replay refuses a genesis  *)
(* that records a cost, as historical legacy-metered execution              *)
(* (offered_only::is_legacy_metered_genesis). AcceptLegacyMeteredGenesis is *)
(* the defect knob: its config must violate                                 *)
(* AdmissionRequiresTrustedInstallGenesis.                                  *)
(***************************************************************************)
EXTENDS Integers, FiniteSets, Sequences, TLC

CONSTANTS Authorities, Events, InitialSupply, ReplayInitialSupply,
          CostReservation, EventDebit, FeeDebit, FeeRecipient,
          ExecutionChoices, DeploymentKinds, HasFiniteProof, DAGDescendsLFB, ParentOrders,
          AllowUnfundedExecution, AllowMismatchedGenesis,
          AllowMismatchedGenesisAuthority,
          CreditGenesisFundingAgain,
          AcceptLegacyMeteredGenesis, LegacyGenesisCost,
          TreatLocalFaultAsInvalid, InjectObjectiveInvalid,
          ProposerRunsCheckpoint, PeerRunsCheckpoint,
          ProposerChecksBonds, PeerChecksBonds

ASSUME /\ Authorities # {}
       /\ Events # {}
       /\ ExecutionChoices \subseteq SUBSET Events
       /\ ExecutionChoices # {}
       /\ DeploymentKinds # {}
       /\ InitialSupply \in [Authorities -> Nat]
       /\ ReplayInitialSupply \in [Authorities -> Nat]
       /\ CostReservation \in [Authorities -> Nat]
       /\ EventDebit \in [Events -> [Authorities -> Nat]]
       /\ FeeDebit \in [Authorities -> Nat]
       /\ FeeRecipient \in Authorities
       /\ HasFiniteProof \in BOOLEAN
       /\ ParentOrders # {}
       /\ AllowUnfundedExecution \in BOOLEAN
       /\ AllowMismatchedGenesis \in BOOLEAN
       /\ AllowMismatchedGenesisAuthority \in BOOLEAN
       /\ CreditGenesisFundingAgain \in BOOLEAN
       /\ AcceptLegacyMeteredGenesis \in BOOLEAN
       /\ LegacyGenesisCost \in Nat \ {0}
       /\ TreatLocalFaultAsInvalid \in BOOLEAN
       /\ InjectObjectiveInvalid \in BOOLEAN
       /\ ProposerRunsCheckpoint \in BOOLEAN
       /\ PeerRunsCheckpoint \in BOOLEAN
       /\ ProposerChecksBonds \in BOOLEAN
       /\ PeerChecksBonds \in BOOLEAN

VARIABLES phase, supply, reserved, committed, replayed, status,
          slashEvidence, lfbAdvanced, parentOrder, localFaultPending,
          selectedEvents, deploymentKind, genesisCommitted, genesisReplayed,
          settlementCompleted, genesisCost

vars == <<phase, supply, reserved, committed, replayed, status,
          slashEvidence, lfbAdvanced, parentOrder, localFaultPending,
          selectedEvents, deploymentKind, genesisCommitted, genesisReplayed,
          settlementCompleted, genesisCost>>

Zero == [a \in Authorities |-> 0]

Add(left, right) ==
  [a \in Authorities |-> left[a] + right[a]]

RECURSIVE SumEventDebit(_, _)
RECURSIVE SumAuthorities(_, _)

SumEventDebit(eventSet, authority) ==
  IF eventSet = {}
  THEN 0
  ELSE LET event == CHOOSE e \in eventSet : TRUE
       IN EventDebit[event][authority]
          + SumEventDebit(eventSet \ {event}, authority)

SumDebit(eventSet) ==
  [a \in Authorities |-> SumEventDebit(eventSet, a)]

SumAuthorities(values, authoritySet) ==
  IF authoritySet = {}
  THEN 0
  ELSE LET authority == CHOOSE a \in authoritySet : TRUE
       IN values[authority]
          + SumAuthorities(values, authoritySet \ {authority})

Leq(left, right) ==
  \A a \in Authorities : left[a] <= right[a]

Subtract(left, right) ==
  [a \in Authorities |-> left[a] - right[a]]

RealizedDebit(eventSet) == Add(SumDebit(eventSet), FeeDebit)
TotalReservation == Add(CostReservation, FeeDebit)
FeeCredit ==
  [a \in Authorities |->
    IF a = FeeRecipient
    THEN SumAuthorities(FeeDebit, Authorities)
    ELSE 0]

GenesisExecutionAuthority == "Unit"
GenesisReplayAuthority ==
  IF AllowMismatchedGenesisAuthority THEN "Funders" ELSE "Unit"

Init ==
  /\ phase = "GenesisCommit"
  /\ supply = Zero
  /\ reserved = Zero
  /\ committed = {}
  /\ replayed = {}
  /\ status = "Pending"
  /\ slashEvidence = FALSE
  /\ lfbAdvanced = FALSE
  /\ parentOrder \in ParentOrders
  /\ localFaultPending = TRUE
  /\ selectedEvents \in ExecutionChoices
  /\ deploymentKind \in DeploymentKinds
  /\ genesisCommitted = Zero
  /\ genesisReplayed = Zero
  /\ settlementCompleted = FALSE
  /\ genesisCost = 0

CommitGenesis ==
  /\ phase = "GenesisCommit"
  /\ phase' = "GenesisReplay"
  /\ supply' = InitialSupply
  /\ genesisCommitted' = InitialSupply
  /\ genesisCost' = IF AcceptLegacyMeteredGenesis THEN LegacyGenesisCost ELSE 0
  /\ UNCHANGED <<reserved, committed, replayed, status, slashEvidence,
                  lfbAdvanced, parentOrder, localFaultPending, selectedEvents,
                  deploymentKind, genesisReplayed, settlementCompleted>>

ReplayGenesis ==
  /\ phase = "GenesisReplay"
  /\ genesisReplayed' = ReplayInitialSupply
  /\ IF /\ (ReplayInitialSupply = genesisCommitted \/ AllowMismatchedGenesis)
         /\ (GenesisReplayAuthority = GenesisExecutionAuthority \/
                AllowMismatchedGenesisAuthority)
         /\ (genesisCost = 0 \/ AcceptLegacyMeteredGenesis)
        THEN /\ phase' = "Admission"
             /\ status' = "Pending"
        ELSE /\ phase' = "Rejected"
             /\ status' = "GenesisRejected"
  /\ UNCHANGED <<supply, reserved, committed, replayed, slashEvidence,
                  lfbAdvanced, parentOrder, localFaultPending, selectedEvents,
                  deploymentKind, genesisCommitted, settlementCompleted, genesisCost>>

Admit ==
  /\ phase = "Admission"
  /\ HasFiniteProof
  /\ (Leq(TotalReservation, supply) \/ AllowUnfundedExecution)
  /\ phase' = "Execution"
  /\ reserved' = TotalReservation
  /\ UNCHANGED <<supply, committed, replayed, status, slashEvidence,
                  lfbAdvanced, parentOrder, localFaultPending, selectedEvents,
                  deploymentKind, genesisCommitted, genesisReplayed,
                  settlementCompleted, genesisCost>>

RejectUnprovable ==
  /\ phase = "Admission"
  /\ ~HasFiniteProof
  /\ phase' = "Rejected"
  /\ status' = "AdmissionRejected"
  /\ UNCHANGED <<supply, reserved, committed, replayed, slashEvidence,
                  lfbAdvanced, parentOrder, localFaultPending, selectedEvents,
                  deploymentKind, genesisCommitted, genesisReplayed,
                  settlementCompleted, genesisCost>>

RejectUnderfunded ==
  /\ phase = "Admission"
  /\ HasFiniteProof
  /\ ~Leq(TotalReservation, supply)
  /\ phase' = "Rejected"
  /\ status' = "AdmissionRejected"
  /\ UNCHANGED <<supply, reserved, committed, replayed, slashEvidence,
                  lfbAdvanced, parentOrder, localFaultPending, selectedEvents,
                  deploymentKind, genesisCommitted, genesisReplayed,
                  settlementCompleted, genesisCost>>

Commit(event) ==
  /\ phase = "Execution"
  /\ event \in selectedEvents \ committed
  /\ committed' = committed \cup {event}
  /\ UNCHANGED <<phase, supply, reserved, replayed, status, slashEvidence,
                  lfbAdvanced, parentOrder, localFaultPending, selectedEvents,
                  deploymentKind, genesisCommitted, genesisReplayed,
                  settlementCompleted, genesisCost>>

FinishExecution ==
  /\ phase = "Execution"
  /\ committed = selectedEvents
  /\ Leq(RealizedDebit(committed), reserved)
  /\ phase' = "Settlement"
  /\ UNCHANGED <<supply, reserved, committed, replayed, status,
                  slashEvidence, lfbAdvanced, parentOrder, localFaultPending,
                  selectedEvents, deploymentKind, genesisCommitted, genesisReplayed,
                  settlementCompleted, genesisCost>>

Settle ==
  /\ phase = "Settlement"
  /\ supply' = IF CreditGenesisFundingAgain
                  THEN Add(Add(Subtract(supply, RealizedDebit(committed)), FeeCredit), InitialSupply)
                  ELSE Add(Subtract(supply, RealizedDebit(committed)), FeeCredit)
  /\ phase' = "Replay"
  /\ replayed' = {}
  /\ settlementCompleted' = TRUE
  /\ UNCHANGED <<reserved, committed, status, slashEvidence,
                  lfbAdvanced, parentOrder, localFaultPending, selectedEvents,
                  deploymentKind, genesisCommitted, genesisReplayed, genesisCost>>

Replay(event) ==
  /\ phase = "Replay"
  /\ event \in committed \ replayed
  /\ replayed' = replayed \cup {event}
  /\ UNCHANGED <<phase, supply, reserved, committed, status,
                  slashEvidence, lfbAdvanced, parentOrder, localFaultPending,
                  selectedEvents, deploymentKind, genesisCommitted, genesisReplayed,
                  settlementCompleted, genesisCost>>

FinishReplay ==
  /\ phase = "Replay"
  /\ replayed = committed
  /\ SumDebit(replayed) = SumDebit(committed)
  /\ phase' = "Done"
  /\ status' = "Valid"
  /\ lfbAdvanced' = DAGDescendsLFB
  /\ UNCHANGED <<supply, reserved, committed, replayed,
                  slashEvidence, parentOrder, localFaultPending, selectedEvents,
                  deploymentKind, genesisCommitted, genesisReplayed,
                  settlementCompleted, genesisCost>>

LocalFault ==
  /\ phase = "Execution"
  /\ localFaultPending
  /\ phase' = "Recover"
  /\ status' = "LocalFault"
  /\ slashEvidence' = TreatLocalFaultAsInvalid
  /\ localFaultPending' = FALSE
  /\ UNCHANGED <<supply, reserved, committed, replayed,
                  lfbAdvanced, parentOrder, selectedEvents, deploymentKind,
                  genesisCommitted, genesisReplayed, settlementCompleted, genesisCost>>

Recover ==
  /\ phase = "Recover"
  /\ phase' = "Execution"
  /\ status' = "Pending"
  /\ slashEvidence' = FALSE
  /\ UNCHANGED <<supply, reserved, committed, replayed,
                  lfbAdvanced, parentOrder, localFaultPending, selectedEvents,
                  deploymentKind, genesisCommitted, genesisReplayed,
                  settlementCompleted, genesisCost>>

ObjectiveInvalid ==
  /\ phase = "Execution"
  /\ InjectObjectiveInvalid
  /\ phase' = "Rejected"
  /\ status' = "ObjectiveInvalid"
  /\ slashEvidence' = TRUE
  /\ UNCHANGED <<supply, reserved, committed, replayed,
                  lfbAdvanced, parentOrder, localFaultPending, selectedEvents,
                  deploymentKind, genesisCommitted, genesisReplayed,
                  settlementCompleted, genesisCost>>

ReorderParents ==
  /\ phase \in {"Execution", "Recover", "Replay"}
  /\ parentOrder' \in ParentOrders
  /\ UNCHANGED <<phase, supply, reserved, committed, replayed, status,
                  slashEvidence, lfbAdvanced, localFaultPending, selectedEvents,
                  deploymentKind, genesisCommitted, genesisReplayed,
                  settlementCompleted, genesisCost>>

Next ==
  \/ CommitGenesis
  \/ ReplayGenesis
  \/ Admit
  \/ RejectUnprovable
  \/ RejectUnderfunded
  \/ \E event \in Events : Commit(event)
  \/ FinishExecution
  \/ Settle
  \/ \E event \in Events : Replay(event)
  \/ FinishReplay
  \/ LocalFault
  \/ Recover
  \/ ObjectiveInvalid
  \/ ReorderParents

CommitAny == \E event \in Events : Commit(event)
ReplayAny == \E event \in Events : Replay(event)

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(Admit)
  /\ WF_vars(CommitGenesis)
  /\ WF_vars(ReplayGenesis)
  /\ WF_vars(RejectUnprovable)
  /\ WF_vars(RejectUnderfunded)
  /\ WF_vars(CommitAny)
  /\ WF_vars(FinishExecution)
  /\ WF_vars(Settle)
  /\ WF_vars(ReplayAny)
  /\ WF_vars(FinishReplay)
  /\ WF_vars(Recover)

TypeOK ==
  /\ phase \in {"GenesisCommit", "GenesisReplay", "Admission", "Execution", "Settlement", "Replay",
                 "Recover", "Rejected", "Done"}
  /\ supply \in [Authorities -> Nat]
  /\ reserved \in [Authorities -> Nat]
  /\ selectedEvents \in ExecutionChoices
  /\ deploymentKind \in DeploymentKinds
  /\ committed \subseteq selectedEvents
  /\ replayed \subseteq committed
  /\ status \in {"Pending", "GenesisRejected", "AdmissionRejected", "LocalFault", "ObjectiveInvalid", "Valid"}
  /\ slashEvidence \in BOOLEAN
  /\ lfbAdvanced \in BOOLEAN
  /\ parentOrder \in ParentOrders
  /\ localFaultPending \in BOOLEAN
  /\ genesisCommitted \in [Authorities -> Nat]
  /\ genesisReplayed \in [Authorities -> Nat]
  /\ settlementCompleted \in BOOLEAN
  /\ genesisCost \in Nat

GenesisCommitIsExact ==
  phase \in {"GenesisReplay", "Admission", "Execution", "Settlement", "Replay",
              "Recover", "Done"} =>
    /\ genesisCommitted = InitialSupply
    /\ (phase \notin {"Replay", "Done"} => supply = InitialSupply)

AdmissionRequiresGenesisAgreement ==
  phase \in {"Admission", "Execution", "Settlement", "Replay", "Recover", "Done"} =>
    genesisReplayed = genesisCommitted

AdmissionRequiresTrustedInstallGenesis ==
  phase \in {"Admission", "Execution", "Settlement", "Replay", "Recover", "Done"} =>
    genesisCost = 0

GenesisExecutionReplayAuthorityAgree ==
  phase \in {"Admission", "Execution", "Settlement", "Replay", "Recover", "Done"} =>
    GenesisReplayAuthority = GenesisExecutionAuthority

SettlementDoesNotReapplyGenesisFunding ==
  phase \in {"Replay", "Done"} =>
    /\ settlementCompleted
    /\ supply = Add(Subtract(InitialSupply, RealizedDebit(committed)), FeeCredit)

ReservationBacksRealized ==
  phase \in {"Settlement", "Replay", "Done"} =>
    Leq(RealizedDebit(committed), reserved)

CostReservationBacksEveryChoice ==
  \A choice \in ExecutionChoices : Leq(SumDebit(choice), CostReservation)

EveryExecutedDeploymentWasFunded ==
  phase \in {"Execution", "Settlement", "Replay", "Recover", "Done"} =>
    Leq(TotalReservation, InitialSupply)

SettlementIsExact ==
  phase \in {"Replay", "Done"} =>
    supply = Add(Subtract(InitialSupply, RealizedDebit(committed)), FeeCredit)

SettlementConserves ==
  phase \in {"Replay", "Done"} =>
    SumAuthorities(supply, Authorities)
      + SumAuthorities(SumDebit(committed), Authorities)
      = SumAuthorities(InitialSupply, Authorities)

FeeIsCanonicalTransfer ==
  phase \in {"Replay", "Done"} =>
    supply[FeeRecipient]
      = InitialSupply[FeeRecipient]
        - RealizedDebit(committed)[FeeRecipient]
        + SumAuthorities(FeeDebit, Authorities)

RefundIsUnusedReservation ==
  phase \in {"Settlement", "Replay", "Done"} =>
    Add(SumDebit(committed),
        Subtract(CostReservation, SumDebit(committed))) = CostReservation

ReplayUsesSameCommittedEvents ==
  phase = "Done" => replayed = committed

LocalFaultNeverCreatesSlashEvidence ==
  status = "LocalFault" => ~slashEvidence

ValidationOriginParity ==
  /\ ProposerRunsCheckpoint = PeerRunsCheckpoint
  /\ ProposerChecksBonds = PeerChecksBonds

EveryOriginRunsConsensusChecks ==
  /\ ProposerRunsCheckpoint
  /\ PeerRunsCheckpoint
  /\ ProposerChecksBonds
  /\ PeerChecksBonds

FinalityUsesDAGAncestry ==
  phase = "Done" => lfbAdvanced = DAGDescendsLFB

EventuallyDoneOrRejected == <>(phase \in {"Done", "Rejected"})

=============================================================================
