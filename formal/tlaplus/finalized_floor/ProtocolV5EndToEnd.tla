-------------------- MODULE ProtocolV5EndToEnd --------------------
EXTENDS Integers, FiniteSets

CONSTANT
  \* @type: Str;
  Defect

ASSUME Defect \in {
  "None",
  "PostStateCertificate",
  "IntrinsicAdmissionBypass",
  "OrderDependentEvidence",
  "GenerationBlindEvidence",
  "HeadCommitteeFinality",
  "UnfilteredFinalityVotes",
  "RetryWithoutRepair",
  "GenerationBlindSlash",
  "RestoreBonded",
  "LostResolutionReceipt",
  "ReplayDrift",
  "SplitSettlement"
}

\* @type: Set(Int);
Nodes == {1, 2, 3}
\* @type: Set(Int);
Validators == {1, 2, 3}
\* @type: Set(Str);
Blocks == {"A", "B", "C", "D", "X"}
\* @type: Set(Str);
ValidBlocks == {"A", "B", "C", "D"}
\* @type: Set(Str);
FinalityBlocks == {"C"}
\* @type: Set(Str);
Phases == {"Bonded", "Withdrawing", "Withdrawn", "Quarantined"}
\* @type: Set(Str);
RestorablePhases == {"Bonded", "Withdrawing"}
\* @type: Str;
NoBlock == "-"
\* @type: Str;
NoPhase == "NoPhase"
\* @type: Int;
NoGeneration == -1
\* @type: Int;
InitialStake == 3
\* @type: Int;
InitialWallet == 3
\* @type: Int;
InitialVault == 8
\* @type: Int;
MaxGeneration == 1

\* @type: (Str) => Int;
Sender(block) ==
  CASE block \in {"A", "B", "D"} -> 1
    [] block = "C" -> 2
    [] OTHER -> 3

\* @type: (Str) => Int;
SequenceNumber(block) ==
  IF block \in Blocks THEN 1 ELSE 0

\* @type: (Str) => Bool;
IntrinsicValid(block) == block \in ValidBlocks

\* @type: (Str) => Int;
MaxCost(block) ==
  CASE block \in {"A", "B"} -> 3
    [] block = "C" -> 2
    [] block = "D" -> 2
    [] OTHER -> 1

\* @type: (Str) => Int;
CanonicalCost(block) ==
  CASE block \in {"A", "B"} -> 2
    [] block = "C" -> 1
    [] block = "D" -> 1
    [] OTHER -> 1

\* @type: (Int -> Int) => Int;
SumValidators(function) == function[1] + function[2] + function[3]

\* @type: (Str -> Int) => Int;
SumBlocks(function) ==
  function["A"] + function["B"] + function["C"] +
  function["D"] + function["X"]

\* @typeAlias: protocolstate = {
\*   proposed: Set(Str),
\*   proposalGeneration: Str -> Int,
\*   proposalBond: Str -> Int,
\*   proposalPreVault: Str -> Int,
\*   delivered: Int -> Set(Str),
\*   stored: Int -> Set(Str),
\*   admitted: Int -> Set(Str),
\*   certificateGeneration: Int -> (Str -> Int),
\*   certificateBond: Int -> (Str -> Int),
\*   lastStored: Int -> Str,
\*   evidenceIndex: Int -> Set(Int),
\*   stableIndex: Int -> Bool,
\*   retryCompleted: Int -> Bool,
\*   online: Int -> Bool,
\*   voteChoice: Int -> Str,
\*   voteDelivery: Int -> Set(Int),
\*   floorCommittee: Set(Int),
\*   roundCommittee: Set(Int),
\*   roundGeneration: Int -> Int,
\*   floorBlock: Str,
\*   finalizedCommittee: Set(Int),
\*   finalizedVoters: Set(Int),
\*   finalizerNode: Int,
\*   phase: Int -> Str,
\*   quarantineOrigin: Int -> Str,
\*   generation: Int -> Int,
\*   stake: Int -> Int,
\*   wallet: Int -> Int,
\*   cooperativeStake: Int -> Int,
\*   custodyStake: Int -> Int,
\*   slashGeneration: Int -> Int,
\*   receiptGeneration: Int -> Int,
\*   retryResolutionChecked: Int -> Bool,
\*   retryResolutionMutated: Bool,
\*   lastRestoredOrigin: Str,
\*   lastRestoredPhase: Str,
\*   vault: Int -> (Int -> Int),
\*   reserved: Int -> (Str -> Int),
\*   replayCost: Int -> (Str -> Int),
\*   charged: Int -> (Str -> Int),
\*   replayed: Int -> Set(Str),
\*   settled: Int -> Set(Str),
\*   fees: Int -> Int
\* };
module_typedefs == TRUE

VARIABLE
  \* @type: $protocolstate;
  state

\* @type: <<$protocolstate>>;
vars == <<state>>

\* @type: (Int) => Set(Int);
ExpectedEvidence(node) ==
  IF {"A", "B"} \subseteq state.admitted[node] /\
     state.proposalGeneration["A"] = state.proposalGeneration["B"] /\
     state.proposalGeneration["A"] >= 0
  THEN {state.proposalGeneration["A"]}
  ELSE {}

\* @type: (Int) => Set(Int);
ReconciledEvidence(node) ==
  IF Defect = "OrderDependentEvidence"
  THEN IF state.lastStored[node] = "B" THEN ExpectedEvidence(node) ELSE {}
  ELSE IF Defect = "GenerationBlindEvidence" /\
          {"A", "D"} \subseteq state.admitted[node] /\
          state.proposalGeneration["A"] /= state.proposalGeneration["D"]
       THEN ExpectedEvidence(node) \cup {state.proposalGeneration["D"]}
       ELSE ExpectedEvidence(node)

\* @type: Set(Int);
HeadCommittee ==
  {validator \in Validators :
    state.stake[validator] > 0 /\
    state.phase[validator] \in {"Bonded", "Withdrawing", "Quarantined"}}

\* @type: (Int) => Set(Int);
ObjectiveOffenders(node) ==
  IF state.roundGeneration[1] \in state.evidenceIndex[node] THEN {1} ELSE {}

\* @type: (Int, Str, Set(Int)) => Set(Int);
SupportingVoters(node, block, committee) ==
  {validator \in committee :
    validator \in state.voteDelivery[node] /\
    state.voteChoice[validator] = block}

\* @type: (Set(Int), Set(Int)) => Bool;
HasMajority(committee, supporters) ==
  2 * Cardinality(supporters) > Cardinality(committee)

\* @type: (Int) => Int;
CostTotal(node) ==
  SumValidators(state.vault[node]) + state.fees[node] +
  SumBlocks(state.reserved[node])

Init ==
  state = [
    proposed |-> {},
    proposalGeneration |-> [block \in Blocks |-> NoGeneration],
    proposalBond |-> [block \in Blocks |-> 0],
    proposalPreVault |-> [block \in Blocks |-> 0],
    delivered |-> [node \in Nodes |-> {}],
    stored |-> [node \in Nodes |-> {}],
    admitted |-> [node \in Nodes |-> {}],
    certificateGeneration |->
      [node \in Nodes |-> [block \in Blocks |-> NoGeneration]],
    certificateBond |->
      [node \in Nodes |-> [block \in Blocks |-> 0]],
    lastStored |-> [node \in Nodes |-> NoBlock],
    evidenceIndex |-> [node \in Nodes |-> {}],
    stableIndex |-> [node \in Nodes |-> TRUE],
    retryCompleted |-> [node \in Nodes |-> FALSE],
    online |-> [node \in Nodes |-> TRUE],
    voteChoice |-> [validator \in Validators |-> NoBlock],
    voteDelivery |-> [node \in Nodes |-> {}],
    floorCommittee |-> Validators,
    roundCommittee |-> Validators,
    roundGeneration |-> [validator \in Validators |-> 0],
    floorBlock |-> NoBlock,
    finalizedCommittee |-> {},
    finalizedVoters |-> {},
    finalizerNode |-> 1,
    phase |-> [validator \in Validators |-> "Bonded"],
    quarantineOrigin |-> [validator \in Validators |-> NoPhase],
    generation |-> [validator \in Validators |-> 0],
    stake |-> [validator \in Validators |-> InitialStake],
    wallet |-> [validator \in Validators |-> InitialWallet],
    cooperativeStake |-> [validator \in Validators |-> 0],
    custodyStake |-> [validator \in Validators |-> 0],
    slashGeneration |-> [validator \in Validators |-> NoGeneration],
    receiptGeneration |-> [validator \in Validators |-> NoGeneration],
    retryResolutionChecked |-> [validator \in Validators |-> FALSE],
    retryResolutionMutated |-> FALSE,
    lastRestoredOrigin |-> NoPhase,
    lastRestoredPhase |-> NoPhase,
    vault |->
      [node \in Nodes |-> [validator \in Validators |-> InitialVault]],
    reserved |->
      [node \in Nodes |-> [block \in Blocks |-> 0]],
    replayCost |->
      [node \in Nodes |-> [block \in Blocks |-> 0]],
    charged |->
      [node \in Nodes |-> [block \in Blocks |-> 0]],
    replayed |-> [node \in Nodes |-> {}],
    settled |-> [node \in Nodes |-> {}],
    fees |-> [node \in Nodes |-> 0]
  ]

\* @type: (Str) => Bool;
Propose(block) ==
  LET validator == Sender(block) IN
  /\ block \in Blocks \ state.proposed
  /\ state.phase[validator] \in RestorablePhases
  /\ state.stake[validator] > 0
  /\ state' = [state EXCEPT
       !.proposed = @ \cup {block},
       !.proposalGeneration[block] = state.generation[validator],
       !.proposalBond[block] = state.stake[validator],
       !.proposalPreVault[block] = state.vault[1][validator]]

\* @type: (Int, Str) => Bool;
Deliver(node, block) ==
  /\ state.online[node]
  /\ block \in state.proposed
  /\ block \notin state.delivered[node]
  /\ state' = [state EXCEPT !.delivered[node] = @ \cup {block}]

\* @type: (Int, Str) => Bool;
Insert(node, block) ==
  LET validator == Sender(block) IN
  LET admissible ==
        (IntrinsicValid(block) \/ Defect = "IntrinsicAdmissionBypass") /\
        state.proposalPreVault[block] >= MaxCost(block) /\
        state.vault[node][validator] >= MaxCost(block)
  IN
  /\ state.online[node]
  /\ block \in state.delivered[node]
  /\ block \notin state.stored[node]
  /\ state' = [state EXCEPT
       !.stored[node] = @ \cup {block},
       !.admitted[node] = IF admissible THEN @ \cup {block} ELSE @,
       !.certificateGeneration[node][block] =
         IF admissible
         THEN IF Defect = "PostStateCertificate"
              THEN state.generation[validator]
              ELSE state.proposalGeneration[block]
         ELSE @,
       !.certificateBond[node][block] =
         IF admissible
         THEN IF Defect = "PostStateCertificate"
              THEN state.stake[validator]
              ELSE state.proposalBond[block]
         ELSE @,
       !.vault[node][validator] =
         IF admissible THEN @ - MaxCost(block) ELSE @,
       !.reserved[node][block] =
         IF admissible THEN MaxCost(block) ELSE @,
       !.lastStored[node] = block,
       !.stableIndex[node] = FALSE,
       !.retryCompleted[node] = FALSE]

\* @type: (Int) => Bool;
Reconcile(node) ==
  /\ state.online[node]
  /\ ~state.stableIndex[node]
  /\ state' = [state EXCEPT
       !.evidenceIndex[node] = ReconciledEvidence(node),
       !.stableIndex[node] = TRUE]

\* @type: (Int) => Bool;
Crash(node) ==
  /\ state.online[node]
  /\ state' = [state EXCEPT
       !.online[node] = FALSE,
       !.delivered[node] = {}]

\* @type: (Int) => Bool;
Restart(node) ==
  /\ ~state.online[node]
  /\ state' = [state EXCEPT !.online[node] = TRUE]

\* @type: (Int, Str) => Bool;
DuplicateRetry(node, block) ==
  /\ state.online[node]
  /\ block \in state.stored[node]
  /\ ~state.retryCompleted[node]
  /\ state' = [state EXCEPT
       !.evidenceIndex[node] =
         IF Defect = "RetryWithoutRepair"
         THEN @
         ELSE ReconciledEvidence(node),
       !.stableIndex[node] = TRUE,
       !.retryCompleted[node] = TRUE]

\* @type: (Int, Str) => Bool;
CastVote(validator, block) ==
  /\ block \in FinalityBlocks
  /\ state.voteChoice[validator] = NoBlock
  /\ \E node \in Nodes : block \in state.admitted[node]
  /\ state' = [state EXCEPT !.voteChoice[validator] = block]

\* @type: (Int, Int) => Bool;
DeliverVote(node, validator) ==
  /\ state.online[node]
  /\ state.voteChoice[validator] /= NoBlock
  /\ validator \notin state.voteDelivery[node]
  /\ state' = [state EXCEPT
       !.voteDelivery[node] = @ \cup {validator}]

\* @type: (Int, Str) => Bool;
Finalize(node, block) ==
  LET committee ==
        IF Defect = "HeadCommitteeFinality"
        THEN HeadCommittee
        ELSE state.roundCommittee IN
  LET eligible ==
        IF Defect = "UnfilteredFinalityVotes"
        THEN committee
        ELSE committee \ ObjectiveOffenders(node) IN
  LET supporters == SupportingVoters(node, block, eligible) IN
  /\ state.floorBlock = NoBlock
  /\ state.online[node]
  /\ state.stableIndex[node]
  /\ block \in state.settled[node]
  /\ HasMajority(committee, supporters)
  /\ state' = [state EXCEPT
       !.floorBlock = block,
       !.finalizedCommittee = committee,
       !.finalizedVoters = supporters,
       !.finalizerNode = node,
       !.floorCommittee = HeadCommittee]

\* @type: (Int) => Bool;
BeginWithdrawal(validator) ==
  /\ state.phase[validator] = "Bonded"
  /\ state' = [state EXCEPT !.phase[validator] = "Withdrawing"]

\* @type: (Int) => Bool;
CompleteWithdrawal(validator) ==
  /\ state.phase[validator] = "Withdrawing"
  /\ state.custodyStake[validator] = 0
  /\ state' = [state EXCEPT
       !.phase[validator] = "Withdrawn",
       !.wallet[validator] = @ + state.stake[validator],
       !.stake[validator] = 0]

\* @type: (Int) => Bool;
FreshBond(validator) ==
  /\ state.phase[validator] = "Withdrawn"
  /\ state.generation[validator] < MaxGeneration
  /\ state.wallet[validator] >= InitialStake
  /\ state' = [state EXCEPT
       !.phase[validator] = "Bonded",
       !.generation[validator] = @ + 1,
       !.wallet[validator] = @ - InitialStake,
       !.stake[validator] = InitialStake,
       !.receiptGeneration[validator] = NoGeneration,
       !.retryResolutionChecked[validator] = FALSE]

\* @type: (Int, Int) => Bool;
Slash(node, validator) ==
  LET authorizedGeneration ==
        IF Defect = "GenerationBlindSlash"
        THEN IF 0 \in state.evidenceIndex[node] THEN 0 ELSE NoGeneration
        ELSE state.generation[validator]
  IN
  /\ state.online[node]
  /\ state.stableIndex[node]
  /\ state.phase[validator] \in RestorablePhases
  /\ state.stake[validator] > 0
  /\ (state.generation[validator] \in state.evidenceIndex[node] \/
      (Defect = "GenerationBlindSlash" /\ state.evidenceIndex[node] /= {}))
  /\ state' = [state EXCEPT
       !.quarantineOrigin[validator] = state.phase[validator],
       !.phase[validator] = "Quarantined",
       !.custodyStake[validator] = state.stake[validator],
       !.slashGeneration[validator] = authorizedGeneration,
       !.lastRestoredOrigin = NoPhase,
       !.lastRestoredPhase = NoPhase]

\* @type: (Int, Str) => Bool;
Resolve(validator, outcome) ==
  /\ outcome \in {"Vindicated", "Guilty"}
  /\ state.phase[validator] = "Quarantined"
  /\ state.receiptGeneration[validator] = NoGeneration
  /\ state' = [state EXCEPT
       !.phase[validator] =
         IF Defect = "RestoreBonded"
         THEN "Bonded"
         ELSE state.quarantineOrigin[validator],
       !.stake[validator] =
         IF outcome = "Guilty" THEN @ - 1 ELSE @,
       !.cooperativeStake[validator] =
         IF outcome = "Guilty" THEN @ + 1 ELSE @,
       !.custodyStake[validator] = 0,
       !.receiptGeneration[validator] = state.slashGeneration[validator],
       !.lastRestoredOrigin = state.quarantineOrigin[validator],
       !.lastRestoredPhase =
         IF Defect = "RestoreBonded"
         THEN "Bonded"
         ELSE state.quarantineOrigin[validator],
       !.quarantineOrigin[validator] = NoPhase]

\* @type: (Int) => Bool;
RetryResolution(validator) ==
  LET mutate ==
        Defect = "LostResolutionReceipt" /\ state.stake[validator] > 0 IN
  /\ state.receiptGeneration[validator] >= 0
  /\ ~state.retryResolutionChecked[validator]
  /\ state' = [state EXCEPT
       !.stake[validator] = IF mutate THEN @ - 1 ELSE @,
       !.cooperativeStake[validator] = IF mutate THEN @ + 1 ELSE @,
       !.retryResolutionChecked[validator] = TRUE,
       !.retryResolutionMutated = state.retryResolutionMutated \/ mutate]

\* @type: (Int, Str) => Bool;
Replay(node, block) ==
  /\ block \in state.admitted[node]
  /\ block \notin state.replayed[node]
  /\ state' = [state EXCEPT
       !.replayed[node] = @ \cup {block},
       !.replayCost[node][block] =
         CanonicalCost(block) +
         IF Defect = "ReplayDrift" /\ node = 3 THEN 1 ELSE 0]

\* @type: (Int, Str) => Bool;
Settle(node, block) ==
  LET cost == state.replayCost[node][block] IN
  LET validator == Sender(block) IN
  /\ block \in state.replayed[node]
  /\ block \notin state.settled[node]
  /\ state.reserved[node][block] >= cost
  /\ state' = [state EXCEPT
       !.vault[node][validator] =
         @ + state.reserved[node][block] - cost,
       !.fees[node] =
         @ + cost + IF Defect = "SplitSettlement" THEN 1 ELSE 0,
       !.charged[node][block] =
         cost + IF Defect = "SplitSettlement" THEN 1 ELSE 0,
       !.reserved[node][block] = 0,
       !.settled[node] = @ \cup {block}]

FreeNext ==
  \/ \E block \in Blocks : Propose(block)
  \/ \E node \in Nodes, block \in Blocks : Deliver(node, block)
  \/ \E node \in Nodes, block \in Blocks : Insert(node, block)
  \/ \E node \in Nodes : Reconcile(node)
  \/ \E node \in Nodes : Crash(node)
  \/ \E node \in Nodes : Restart(node)
  \/ \E node \in Nodes, block \in Blocks : DuplicateRetry(node, block)
  \/ \E validator \in Validators, block \in FinalityBlocks :
       CastVote(validator, block)
  \/ \E node \in Nodes, validator \in Validators :
       DeliverVote(node, validator)
  \/ \E node \in Nodes, block \in FinalityBlocks : Finalize(node, block)
  \/ \E validator \in Validators : BeginWithdrawal(validator)
  \/ \E validator \in Validators : CompleteWithdrawal(validator)
  \/ \E validator \in Validators : FreshBond(validator)
  \/ \E node \in Nodes, validator \in Validators : Slash(node, validator)
  \/ \E validator \in Validators, outcome \in {"Vindicated", "Guilty"} :
       Resolve(validator, outcome)
  \/ \E validator \in Validators : RetryResolution(validator)
  \/ \E node \in Nodes, block \in Blocks : Replay(node, block)
  \/ \E node \in Nodes, block \in Blocks : Settle(node, block)

\* @type: (Int, Str) => Bool;
PrepareStoredBlock(node, block) ==
  IF block \notin state.proposed
  THEN Propose(block)
  ELSE IF block \notin state.delivered[node] /\
          block \notin state.stored[node]
       THEN Deliver(node, block)
       ELSE Insert(node, block)

\* @type: (Int, Bool) => Bool;
PrepareSiblingEvidence(node, reverseOrder) ==
  IF "A" \notin state.proposed
  THEN Propose("A")
  ELSE IF "B" \notin state.proposed
       THEN Propose("B")
       ELSE IF reverseOrder /\ "B" \notin state.stored[node]
            THEN PrepareStoredBlock(node, "B")
            ELSE IF "A" \notin state.stored[node]
                 THEN PrepareStoredBlock(node, "A")
                 ELSE IF "B" \notin state.stored[node]
                      THEN PrepareStoredBlock(node, "B")
                      ELSE Reconcile(node)

\* @type: (Int, Str) => Bool;
PrepareSettledBlock(node, block) ==
  IF block \notin state.stored[node]
  THEN PrepareStoredBlock(node, block)
  ELSE IF ~state.stableIndex[node]
       THEN Reconcile(node)
       ELSE IF block \notin state.replayed[node]
            THEN Replay(node, block)
            ELSE Settle(node, block)

PostStateCertificateControl ==
  IF "A" \notin state.proposed
  THEN Propose("A")
  ELSE IF state.proposalGeneration["A"] = state.generation[1]
       THEN IF state.phase[1] = "Bonded"
            THEN BeginWithdrawal(1)
            ELSE IF state.phase[1] = "Withdrawing"
                 THEN CompleteWithdrawal(1)
                 ELSE FreshBond(1)
       ELSE PrepareStoredBlock(1, "A")

IntrinsicAdmissionControl == PrepareStoredBlock(1, "X")

OrderDependentEvidenceControl == PrepareSiblingEvidence(1, TRUE)

GenerationBlindEvidenceControl ==
  IF "A" \notin state.stored[1]
  THEN PrepareStoredBlock(1, "A")
  ELSE IF state.generation[1] = 0
       THEN IF state.phase[1] = "Bonded"
            THEN BeginWithdrawal(1)
            ELSE IF state.phase[1] = "Withdrawing"
                 THEN CompleteWithdrawal(1)
                 ELSE FreshBond(1)
       ELSE IF "D" \notin state.stored[1]
            THEN PrepareStoredBlock(1, "D")
            ELSE Reconcile(1)

HeadCommitteeFinalityControl ==
  IF "C" \notin state.settled[1]
  THEN PrepareSettledBlock(1, "C")
  ELSE IF state.phase[3] = "Bonded"
       THEN BeginWithdrawal(3)
       ELSE IF state.phase[3] = "Withdrawing"
            THEN CompleteWithdrawal(3)
            ELSE IF state.voteChoice[1] = NoBlock
                 THEN CastVote(1, "C")
                 ELSE IF state.voteChoice[2] = NoBlock
                      THEN CastVote(2, "C")
                      ELSE IF 1 \notin state.voteDelivery[1]
                           THEN DeliverVote(1, 1)
                           ELSE IF 2 \notin state.voteDelivery[1]
                                THEN DeliverVote(1, 2)
                                ELSE Finalize(1, "C")

UnfilteredFinalityControl ==
  IF state.evidenceIndex[1] = {}
  THEN PrepareSiblingEvidence(1, FALSE)
  ELSE IF "C" \notin state.settled[1]
       THEN PrepareSettledBlock(1, "C")
       ELSE IF state.voteChoice[1] = NoBlock
            THEN CastVote(1, "C")
            ELSE IF state.voteChoice[2] = NoBlock
                 THEN CastVote(2, "C")
                 ELSE IF 1 \notin state.voteDelivery[1]
                      THEN DeliverVote(1, 1)
                      ELSE IF 2 \notin state.voteDelivery[1]
                           THEN DeliverVote(1, 2)
                           ELSE Finalize(1, "C")

RetryWithoutRepairControl ==
  IF ~({"A", "B"} \subseteq state.stored[1])
  THEN IF "A" \notin state.stored[1]
       THEN PrepareStoredBlock(1, "A")
       ELSE PrepareStoredBlock(1, "B")
  ELSE IF state.online[1] /\ state.delivered[1] /= {}
       THEN Crash(1)
       ELSE IF ~state.online[1]
            THEN Restart(1)
            ELSE DuplicateRetry(1, "B")

GenerationBlindSlashControl ==
  IF state.evidenceIndex[1] = {}
  THEN PrepareSiblingEvidence(1, FALSE)
  ELSE IF state.generation[1] = 0
       THEN IF state.phase[1] = "Bonded"
            THEN BeginWithdrawal(1)
            ELSE IF state.phase[1] = "Withdrawing"
                 THEN CompleteWithdrawal(1)
                 ELSE FreshBond(1)
       ELSE Slash(1, 1)

RestoreBondedControl ==
  IF state.evidenceIndex[1] = {}
  THEN PrepareSiblingEvidence(1, FALSE)
  ELSE IF state.phase[1] = "Bonded"
       THEN BeginWithdrawal(1)
       ELSE IF state.phase[1] = "Withdrawing"
            THEN Slash(1, 1)
            ELSE Resolve(1, "Vindicated")

LostReceiptControl ==
  IF state.receiptGeneration[1] >= 0
  THEN RetryResolution(1)
  ELSE IF state.evidenceIndex[1] = {}
  THEN PrepareSiblingEvidence(1, FALSE)
  ELSE IF state.phase[1] = "Bonded"
       THEN Slash(1, 1)
       ELSE IF state.phase[1] = "Quarantined"
            THEN Resolve(1, "Guilty")
            ELSE /\ state' = state
                 /\ FALSE

ReplayDriftControl ==
  IF "C" \notin state.stored[3]
  THEN PrepareStoredBlock(3, "C")
  ELSE Replay(3, "C")

SplitSettlementControl == PrepareSettledBlock(1, "C")

ControlNext ==
  CASE Defect = "PostStateCertificate" -> PostStateCertificateControl
    [] Defect = "IntrinsicAdmissionBypass" -> IntrinsicAdmissionControl
    [] Defect = "OrderDependentEvidence" -> OrderDependentEvidenceControl
    [] Defect = "GenerationBlindEvidence" -> GenerationBlindEvidenceControl
    [] Defect = "HeadCommitteeFinality" -> HeadCommitteeFinalityControl
    [] Defect = "UnfilteredFinalityVotes" -> UnfilteredFinalityControl
    [] Defect = "RetryWithoutRepair" -> RetryWithoutRepairControl
    [] Defect = "GenerationBlindSlash" -> GenerationBlindSlashControl
    [] Defect = "RestoreBonded" -> RestoreBondedControl
    [] Defect = "LostResolutionReceipt" -> LostReceiptControl
    [] Defect = "ReplayDrift" -> ReplayDriftControl
    [] Defect = "SplitSettlement" -> SplitSettlementControl
    [] OTHER -> /\ state' = state
                /\ FALSE

Next == IF Defect = "None" THEN FreeNext ELSE ControlNext

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ state.proposed \subseteq Blocks
  /\ state.proposalGeneration \in [Blocks -> NoGeneration..MaxGeneration]
  /\ state.proposalBond \in [Blocks -> 0..InitialStake]
  /\ state.proposalPreVault \in [Blocks -> 0..InitialVault]
  /\ state.delivered \in [Nodes -> SUBSET Blocks]
  /\ state.stored \in [Nodes -> SUBSET Blocks]
  /\ state.admitted \in [Nodes -> SUBSET Blocks]
  /\ state.certificateGeneration \in
       [Nodes -> [Blocks -> NoGeneration..MaxGeneration]]
  /\ state.certificateBond \in [Nodes -> [Blocks -> 0..InitialStake]]
  /\ state.lastStored \in [Nodes -> Blocks \cup {NoBlock}]
  /\ state.evidenceIndex \in [Nodes -> SUBSET (0..MaxGeneration)]
  /\ state.stableIndex \in [Nodes -> BOOLEAN]
  /\ state.retryCompleted \in [Nodes -> BOOLEAN]
  /\ state.online \in [Nodes -> BOOLEAN]
  /\ state.voteChoice \in [Validators -> FinalityBlocks \cup {NoBlock}]
  /\ state.voteDelivery \in [Nodes -> SUBSET Validators]
  /\ state.floorCommittee \subseteq Validators
  /\ state.roundCommittee \subseteq Validators
  /\ state.roundGeneration \in [Validators -> 0..MaxGeneration]
  /\ state.floorBlock \in FinalityBlocks \cup {NoBlock}
  /\ state.finalizedCommittee \subseteq Validators
  /\ state.finalizedVoters \subseteq Validators
  /\ state.finalizerNode \in Nodes
  /\ state.phase \in [Validators -> Phases]
  /\ state.quarantineOrigin \in [Validators -> RestorablePhases \cup {NoPhase}]
  /\ state.generation \in [Validators -> 0..MaxGeneration]
  /\ state.stake \in [Validators -> 0..InitialStake]
  /\ state.wallet \in [Validators -> 0..(InitialWallet + InitialStake)]
  /\ state.cooperativeStake \in [Validators -> 0..InitialStake]
  /\ state.custodyStake \in [Validators -> 0..InitialStake]
  /\ state.slashGeneration \in [Validators -> NoGeneration..MaxGeneration]
  /\ state.receiptGeneration \in [Validators -> NoGeneration..MaxGeneration]
  /\ state.retryResolutionChecked \in [Validators -> BOOLEAN]
  /\ state.retryResolutionMutated \in BOOLEAN
  /\ state.lastRestoredOrigin \in RestorablePhases \cup {NoPhase}
  /\ state.lastRestoredPhase \in RestorablePhases \cup {"Bonded", NoPhase}
  /\ state.vault \in [Nodes -> [Validators -> 0..InitialVault]]
  /\ state.reserved \in [Nodes -> [Blocks -> 0..InitialVault]]
  /\ state.replayCost \in [Nodes -> [Blocks -> 0..InitialVault]]
  /\ state.charged \in [Nodes -> [Blocks -> 0..InitialVault]]
  /\ state.replayed \in [Nodes -> SUBSET Blocks]
  /\ state.settled \in [Nodes -> SUBSET Blocks]
  /\ state.fees \in [Nodes -> 0..(3 * InitialVault)]

AtLeastThreeReplicasAndValidators ==
  Cardinality(Nodes) >= 3 /\ Cardinality(Validators) >= 3

IntrinsicAdmissionOnly ==
  \A node \in Nodes : state.admitted[node] \subseteq ValidBlocks

CertificatesUseExactProposalPreState ==
  \A node \in Nodes :
    \A block \in state.admitted[node] :
      /\ state.certificateGeneration[node][block] =
           state.proposalGeneration[block]
      /\ state.certificateBond[node][block] = state.proposalBond[block]

StableEvidenceIsGenerationAwareAndOrderIndependent ==
  \A node \in Nodes :
    state.stableIndex[node] =>
      state.evidenceIndex[node] = ExpectedEvidence(node)

CompletedRetryRepairsDurableEvidence ==
  \A node \in Nodes :
    state.retryCompleted[node] =>
      /\ state.stableIndex[node]
      /\ state.evidenceIndex[node] = ExpectedEvidence(node)

FloorCommitteeFrozenUntilFinalization ==
  state.floorBlock = NoBlock =>
    state.floorCommittee = state.roundCommittee

FinalizationUsesFrozenFloorCommittee ==
  state.floorBlock /= NoBlock =>
    state.finalizedCommittee = state.roundCommittee

ObjectiveEquivocatorsDoNotContributeFinalityVotes ==
  state.floorBlock /= NoBlock /\
  state.roundGeneration[1] \in
    state.evidenceIndex[state.finalizerNode] =>
      1 \notin state.finalizedVoters

FinalizedBlocksAreAdmittedReplayedAndSettled ==
  state.floorBlock /= NoBlock =>
    /\ state.floorBlock \in state.admitted[state.finalizerNode]
    /\ state.floorBlock \in state.replayed[state.finalizerNode]
    /\ state.floorBlock \in state.settled[state.finalizerNode]

SlashTargetsCurrentBondGeneration ==
  \A validator \in Validators :
    state.phase[validator] = "Quarantined" =>
      state.slashGeneration[validator] = state.generation[validator]

CustodyMatchesQuarantine ==
  \A validator \in Validators :
    /\ (state.phase[validator] = "Quarantined") <=>
         (state.custodyStake[validator] = state.stake[validator] /\
          state.custodyStake[validator] > 0)
    /\ (state.phase[validator] = "Quarantined") <=>
         (state.quarantineOrigin[validator] \in RestorablePhases)

RedemptionRestoresExactLifecycle ==
  state.lastRestoredOrigin = NoPhase \/
  state.lastRestoredPhase = state.lastRestoredOrigin

ResolutionRetriesAreIdempotent == ~state.retryResolutionMutated

StakeConserved ==
  \A validator \in Validators :
    state.stake[validator] + state.wallet[validator] +
    state.cooperativeStake[validator] = InitialStake + InitialWallet

CostConserved ==
  \A node \in Nodes : CostTotal(node) = 3 * InitialVault

ReplayMatchesCanonicalCost ==
  \A node \in Nodes :
    \A block \in state.replayed[node] :
      state.replayCost[node][block] = CanonicalCost(block)

SettlementChargesExactlyReplayCost ==
  \A node \in Nodes :
    \A block \in state.settled[node] :
      state.charged[node][block] = state.replayCost[node][block]

ReplicaReplayAgreement ==
  \A left \in Nodes, right \in Nodes, block \in Blocks :
    block \in state.replayed[left] /\ block \in state.replayed[right] =>
      state.replayCost[left][block] = state.replayCost[right][block]

=====================================================================
