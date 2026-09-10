-------------------------- MODULE EpochMintAtomicity --------------------------
EXTENDS Naturals, FiniteSets, TLC, Apalache

CONSTANTS
    \* @type: Str;
    Defect,
    \* @type: Int;
    Amount

ASSUME
    /\ Defect \in {
         "None",
         "SwallowFailure",
         "EarlyBalance",
         "EarlyReceipt",
         "MissingRuntimeRollback",
         "ReplayPartial",
         "ZeroCallsStrictMint"
       }
    /\ Amount \in 0..2

Validators == {"v1", "v2", "v3"}
Epoch == 1
InitialBalance == [v \in Validators |-> IF v = "v1" THEN 2 ELSE 3]
InitialReceipts == {}
InitialRewards == 5
InitialWithdrawals == 7
InitialActive == Validators
EpochReceipts == {<<v, Epoch>> : v \in Validators}

VARIABLES
    \* @type: Str;
    phase,
    \* @type: Str -> Int;
    committedBalance,
    \* @type: Set(<<Str, Int>>);
    committedReceipts,
    \* @type: Int;
    committedRewards,
    \* @type: Int;
    committedWithdrawals,
    \* @type: Set(Str);
    committedActive,
    \* @type: Str -> Int;
    candidateBalance,
    \* @type: Set(<<Str, Int>>);
    candidateReceipts,
    \* @type: Int;
    candidateRewards,
    \* @type: Int;
    candidateWithdrawals,
    \* @type: Set(Str);
    candidateActive,
    \* @type: Set(Str);
    completed,
    \* @type: Set(Str);
    succeeded,
    \* @type: Bool;
    failureSeen,
    \* @type: Str -> Int;
    playBalance,
    \* @type: Str -> Int;
    replayBalance,
    \* @type: Set(<<Str, Int>>);
    playReceipts,
    \* @type: Set(<<Str, Int>>);
    replayReceipts

vars ==
    <<phase, committedBalance, committedReceipts, committedRewards,
      committedWithdrawals, committedActive, candidateBalance,
      candidateReceipts, candidateRewards, candidateWithdrawals,
      candidateActive, completed, succeeded, failureSeen, playBalance,
      replayBalance, playReceipts, replayReceipts>>

\* @type: (Str -> Int) => Int;
SumBalance(balance) ==
    LET AddBalance(total, validator) == total + balance[validator]
    IN ApaFoldSet(AddBalance, 0, Validators)

Init ==
    /\ phase = "Idle"
    /\ committedBalance = InitialBalance
    /\ committedReceipts = InitialReceipts
    /\ committedRewards = InitialRewards
    /\ committedWithdrawals = InitialWithdrawals
    /\ committedActive = InitialActive
    /\ candidateBalance = InitialBalance
    /\ candidateReceipts = InitialReceipts
    /\ candidateRewards = InitialRewards
    /\ candidateWithdrawals = InitialWithdrawals
    /\ candidateActive = InitialActive
    /\ completed = {}
    /\ succeeded = {}
    /\ failureSeen = FALSE
    /\ playBalance = InitialBalance
    /\ replayBalance = InitialBalance
    /\ playReceipts = InitialReceipts
    /\ replayReceipts = InitialReceipts

Start ==
    /\ phase = "Idle"
    /\ phase' = "Minting"
    /\ candidateBalance' = committedBalance
    /\ candidateReceipts' = committedReceipts
    /\ candidateRewards' = committedRewards + 1
    /\ candidateWithdrawals' = committedWithdrawals + 1
    /\ candidateActive' = committedActive
    /\ completed' = {}
    /\ succeeded' = {}
    /\ failureSeen' = FALSE
    /\ UNCHANGED <<committedBalance, committedReceipts, committedRewards,
                    committedWithdrawals, committedActive, playBalance,
                    replayBalance, playReceipts, replayReceipts>>

MintSuccess(validator) ==
    /\ phase = "Minting"
    /\ ~failureSeen
    /\ validator \in Validators \ completed
    /\ LET nextBalance == [candidateBalance EXCEPT ![validator] = @ + Amount]
           nextReceipts == candidateReceipts \cup {<<validator, Epoch>>}
       IN /\ candidateBalance' = nextBalance
          /\ candidateReceipts' = nextReceipts
          /\ committedBalance' =
               IF Defect = "EarlyBalance" THEN nextBalance ELSE committedBalance
          /\ committedReceipts' =
               IF Defect = "EarlyReceipt" THEN nextReceipts ELSE committedReceipts
    /\ completed' = completed \cup {validator}
    /\ succeeded' = succeeded \cup {validator}
    /\ UNCHANGED <<phase, committedRewards, committedWithdrawals,
                    committedActive, candidateRewards, candidateWithdrawals,
                    candidateActive, failureSeen, playBalance, replayBalance,
                    playReceipts, replayReceipts>>

MintFailure(validator) ==
    /\ phase = "Minting"
    /\ ~failureSeen
    /\ validator \in Validators \ completed
    /\ (Amount > 0 \/ Defect = "ZeroCallsStrictMint")
    /\ completed' =
         IF Defect = "SwallowFailure" THEN completed \cup {validator}
         ELSE completed
    /\ failureSeen' = IF Defect = "SwallowFailure" THEN FALSE ELSE TRUE
    /\ UNCHANGED <<phase, committedBalance, committedReceipts,
                    committedRewards, committedWithdrawals, committedActive,
                    candidateBalance, candidateReceipts, candidateRewards,
                    candidateWithdrawals, candidateActive, succeeded,
                    playBalance, replayBalance, playReceipts, replayReceipts>>

FinishMint ==
    /\ phase = "Minting"
    /\ ~failureSeen
    /\ completed = Validators
    /\ phase' = "Ready"
    /\ UNCHANGED <<committedBalance, committedReceipts, committedRewards,
                    committedWithdrawals, committedActive, candidateBalance,
                    candidateReceipts, candidateRewards, candidateWithdrawals,
                    candidateActive, completed, succeeded, failureSeen,
                    playBalance, replayBalance, playReceipts, replayReceipts>>

Abort ==
    /\ phase = "Minting"
    /\ failureSeen
    /\ phase' = "Aborted"
    /\ committedBalance' =
         IF Defect = "MissingRuntimeRollback" THEN candidateBalance
         ELSE committedBalance
    /\ committedReceipts' =
         IF Defect = "MissingRuntimeRollback" THEN candidateReceipts
         ELSE committedReceipts
    /\ committedRewards' =
         IF Defect = "MissingRuntimeRollback" THEN candidateRewards
         ELSE committedRewards
    /\ committedWithdrawals' =
         IF Defect = "MissingRuntimeRollback" THEN candidateWithdrawals
         ELSE committedWithdrawals
    /\ committedActive' =
         IF Defect = "MissingRuntimeRollback" THEN candidateActive
         ELSE committedActive
    /\ candidateBalance' = InitialBalance
    /\ candidateReceipts' = InitialReceipts
    /\ candidateRewards' = InitialRewards
    /\ candidateWithdrawals' = InitialWithdrawals
    /\ candidateActive' = InitialActive
    /\ UNCHANGED <<completed, succeeded, failureSeen, playBalance,
                    replayBalance, playReceipts, replayReceipts>>

Retry ==
    /\ phase = "Aborted"
    /\ phase' = "Minting"
    /\ candidateBalance' = committedBalance
    /\ candidateReceipts' = committedReceipts
    /\ candidateRewards' = committedRewards + 1
    /\ candidateWithdrawals' = committedWithdrawals + 1
    /\ candidateActive' = committedActive
    /\ completed' = {}
    /\ succeeded' = {}
    /\ failureSeen' = FALSE
    /\ UNCHANGED <<committedBalance, committedReceipts, committedRewards,
                    committedWithdrawals, committedActive, playBalance,
                    replayBalance, playReceipts, replayReceipts>>

Commit ==
    /\ phase = "Ready"
    /\ phase' = "Committed"
    /\ committedBalance' = candidateBalance
    /\ committedReceipts' = candidateReceipts
    /\ committedRewards' = candidateRewards
    /\ committedWithdrawals' = candidateWithdrawals
    /\ committedActive' = candidateActive
    /\ playBalance' = candidateBalance
    /\ playReceipts' = candidateReceipts
    /\ replayBalance' =
         IF Defect = "ReplayPartial" THEN InitialBalance ELSE candidateBalance
    /\ replayReceipts' =
         IF Defect = "ReplayPartial" THEN InitialReceipts ELSE candidateReceipts
    /\ UNCHANGED <<candidateBalance, candidateReceipts, candidateRewards,
                    candidateWithdrawals, candidateActive, completed,
                    succeeded, failureSeen>>

RemainCommitted ==
    /\ phase = "Committed"
    /\ UNCHANGED vars

Next ==
    \/ Start
    \/ \E validator \in Validators : MintSuccess(validator)
    \/ \E validator \in Validators : MintFailure(validator)
    \/ FinishMint
    \/ Abort
    \/ Retry
    \/ Commit
    \/ RemainCommitted

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"Idle", "Minting", "Ready", "Aborted", "Committed"}
    /\ committedBalance \in [Validators -> Nat]
    /\ candidateBalance \in [Validators -> Nat]
    /\ playBalance \in [Validators -> Nat]
    /\ replayBalance \in [Validators -> Nat]
    /\ committedReceipts \subseteq EpochReceipts
    /\ candidateReceipts \subseteq EpochReceipts
    /\ playReceipts \subseteq EpochReceipts
    /\ replayReceipts \subseteq EpochReceipts
    /\ committedRewards \in Nat
    /\ committedWithdrawals \in Nat
    /\ candidateRewards \in Nat
    /\ candidateWithdrawals \in Nat
    /\ committedActive \subseteq Validators
    /\ candidateActive \subseteq Validators
    /\ completed \subseteq Validators
    /\ succeeded \subseteq Validators
    /\ failureSeen \in BOOLEAN

NoPartialBalancePublication ==
    phase \in {"Idle", "Minting", "Ready"} =>
      committedBalance = InitialBalance

NoPartialReceiptPublication ==
    phase \in {"Idle", "Minting", "Ready"} =>
      committedReceipts = InitialReceipts

NoPartialAuxiliaryPublication ==
    phase \in {"Idle", "Minting", "Ready"} =>
      /\ committedRewards = InitialRewards
      /\ committedWithdrawals = InitialWithdrawals
      /\ committedActive = InitialActive

AbortRestoresCommittedState ==
    phase = "Aborted" =>
      /\ committedBalance = InitialBalance
      /\ committedReceipts = InitialReceipts
      /\ committedRewards = InitialRewards
      /\ committedWithdrawals = InitialWithdrawals
      /\ committedActive = InitialActive

CommitRequiresAllEligibleMints ==
    phase = "Committed" => succeeded = Validators

ExactAuthorizedSupplyDelta ==
    phase = "Committed" =>
      SumBalance(committedBalance) =
        SumBalance(InitialBalance) + Cardinality(Validators) * Amount

AtMostOneCreditPerValidatorEpoch ==
    \A validator \in Validators :
      committedBalance[validator] <= InitialBalance[validator] + Amount

ReceiptsFollowSuccessfulMints ==
    phase = "Committed" => committedReceipts = EpochReceipts

ZeroIssuanceCompletesWithoutFailure ==
    Amount = 0 => phase # "Aborted"

OrderIndependentCommittedState ==
    phase = "Committed" =>
      committedBalance =
        [validator \in Validators |-> InitialBalance[validator] + Amount]

PlayReplayCommittedStateEqual ==
    /\ playBalance = replayBalance
    /\ playReceipts = replayReceipts

=============================================================================
