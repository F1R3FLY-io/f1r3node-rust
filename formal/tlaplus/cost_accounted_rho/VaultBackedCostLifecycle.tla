---------------------- MODULE VaultBackedCostLifecycle ----------------------
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS
    Payers,
    Deployments,
    DeployOrder,
    InitialBalance,
    EpochMint,
    CertifiedBound,
    RealizedCost,
    RealizedFee,
    FeeRecipient,
    AuthorizedPayers,
    UnauthorizedPayer,
    NoDeployment,
    AllowExecuteBeforeReserve,
    AllowNonAtomicReservation,
    AllowUnauthorizedDraw,
    AllowRefundLoss,
    AllowReplayOmission,
    AllowDoubleMint,
    AllowIndependentCredit

ASSUME /\ Payers # {}
       /\ Deployments # {}
       /\ DeployOrder \in Seq(Deployments)
       /\ {DeployOrder[index] : index \in 1..Len(DeployOrder)} = Deployments
       /\ InitialBalance \in [Payers -> Nat]
       /\ EpochMint \in [Payers -> Nat]
       /\ CertifiedBound \in [Deployments -> [Payers -> Nat]]
       /\ RealizedCost \in [Deployments -> [Payers -> Nat]]
       /\ RealizedFee \in [Deployments -> [Payers -> Nat]]
       /\ FeeRecipient \in [Deployments -> Payers]
       /\ AuthorizedPayers \in [Deployments -> SUBSET Payers]
       /\ UnauthorizedPayer \in Payers
       /\ NoDeployment \notin Deployments
       /\ \A deployment \in Deployments :
            \A payer \in Payers :
              RealizedCost[deployment][payer]
                + RealizedFee[deployment][payer]
                  <= CertifiedBound[deployment][payer]
       /\ \A deployment \in Deployments :
            \A payer \in Payers \ AuthorizedPayers[deployment] :
              CertifiedBound[deployment][payer] = 0
       /\ \A flag \in {
            AllowExecuteBeforeReserve,
            AllowNonAtomicReservation,
            AllowUnauthorizedDraw,
            AllowRefundLoss,
            AllowReplayOmission,
            AllowDoubleMint,
            AllowIndependentCredit
          } : flag \in BOOLEAN

VARIABLES
    phase,
    cursor,
    status,
    balance,
    escrow,
    reservation,
    burned,
    feeCredits,
    mintRounds,
    protocolMinted,
    independentCredit,
    replayCursor,
    replayBalance,
    replayEscrow,
    replayBurned,
    replayFeeCredits,
    replayed

vars == <<phase, cursor, status, balance, escrow, reservation, burned, feeCredits,
          mintRounds, protocolMinted, independentCredit, replayCursor,
          replayBalance, replayEscrow, replayBurned, replayFeeCredits, replayed>>

ZeroPayers == [payer \in Payers |-> 0]
ZeroReservations == [deployment \in Deployments |-> ZeroPayers]

RECURSIVE SumSet(_, _)

SumSet(function, domain) ==
    IF domain = {}
    THEN 0
    ELSE LET element == CHOOSE value \in domain : TRUE
         IN function[element] + SumSet(function, domain \ {element})

VectorAdd(left, right) ==
    [payer \in Payers |-> left[payer] + right[payer]]

VectorSub(left, right) ==
    [payer \in Payers |-> left[payer] - right[payer]]

VectorLeq(left, right) ==
    \A payer \in Payers : left[payer] <= right[payer]

Settlement(deployment) ==
    [payer \in Payers |->
      RealizedCost[deployment][payer] + RealizedFee[deployment][payer]]

FeeCredit(deployment) ==
    [payer \in Payers |->
      IF payer = FeeRecipient[deployment]
      THEN SumSet(RealizedFee[deployment], Payers)
      ELSE 0]

Refund(deployment) ==
    VectorSub(CertifiedBound[deployment], Settlement(deployment))

ReservationVector(deployment) ==
    IF AllowUnauthorizedDraw
    THEN [payer \in Payers |->
           IF payer = UnauthorizedPayer
           THEN SumSet(CertifiedBound[deployment], Payers)
           ELSE 0]
    ELSE CertifiedBound[deployment]

CurrentDeployment == DeployOrder[cursor]

Init ==
    /\ phase = "Admission"
    /\ cursor = 1
    /\ status = [deployment \in Deployments |-> "Pending"]
    /\ balance = InitialBalance
    /\ escrow = ZeroPayers
    /\ reservation = ZeroReservations
    /\ burned = ZeroPayers
    /\ feeCredits = ZeroPayers
    /\ mintRounds = 0
    /\ protocolMinted = ZeroPayers
    /\ independentCredit = ZeroPayers
    /\ replayCursor = 1
    /\ replayBalance = InitialBalance
    /\ replayEscrow = ZeroPayers
    /\ replayBurned = ZeroPayers
    /\ replayFeeCredits = ZeroPayers
    /\ replayed = {}

Mint ==
    /\ phase = "Admission"
    /\ \/ mintRounds = 0
       \/ /\ AllowDoubleMint
          /\ mintRounds = 1
    /\ balance' = VectorAdd(balance, EpochMint)
    /\ protocolMinted' = VectorAdd(protocolMinted, EpochMint)
    /\ mintRounds' = mintRounds + 1
    /\ UNCHANGED <<phase, cursor, status, escrow, reservation, burned, feeCredits,
                    independentCredit, replayCursor, replayBalance,
                    replayEscrow, replayBurned, replayFeeCredits, replayed>>

CreateIndependentCredit ==
    /\ AllowIndependentCredit
    /\ phase = "Admission"
    /\ independentCredit = ZeroPayers
    /\ independentCredit' = CertifiedBound[DeployOrder[1]]
    /\ UNCHANGED <<phase, cursor, status, balance, escrow, reservation, burned,
                    feeCredits, mintRounds, protocolMinted, replayCursor,
                    replayBalance, replayEscrow, replayBurned, replayFeeCredits,
                    replayed>>

Reserve ==
    /\ phase = "Admission"
    /\ mintRounds = 1
    /\ cursor <= Len(DeployOrder)
    /\ LET deployment == CurrentDeployment
           requested == ReservationVector(deployment)
       IN /\ status[deployment] = "Pending"
          /\ VectorLeq(requested, balance)
          /\ balance' = VectorSub(balance, requested)
          /\ escrow' = VectorAdd(escrow, requested)
          /\ reservation' = [reservation EXCEPT ![deployment] = requested]
          /\ status' = [status EXCEPT ![deployment] = "Reserved"]
    /\ cursor' = cursor + 1
    /\ UNCHANGED <<phase, burned, feeCredits, mintRounds, protocolMinted,
                    independentCredit, replayCursor, replayBalance,
                    replayEscrow, replayBurned, replayFeeCredits, replayed>>

ReservePartial ==
    /\ AllowNonAtomicReservation
    /\ phase = "Admission"
    /\ mintRounds = 1
    /\ cursor <= Len(DeployOrder)
    /\ LET deployment == CurrentDeployment
           payer == CHOOSE candidate \in Payers :
                      CertifiedBound[deployment][candidate] > 0
           amount == CertifiedBound[deployment][payer]
       IN /\ status[deployment] = "Pending"
          /\ amount <= balance[payer]
          /\ balance' = [balance EXCEPT ![payer] = @ - amount]
          /\ escrow' = [escrow EXCEPT ![payer] = @ + amount]
          /\ reservation' =
                [reservation EXCEPT ![deployment][payer] = amount]
          /\ status' = [status EXCEPT ![deployment] = "PartiallyReserved"]
    /\ cursor' = cursor + 1
    /\ UNCHANGED <<phase, burned, feeCredits, mintRounds, protocolMinted,
                    independentCredit, replayCursor, replayBalance,
                    replayEscrow, replayBurned, replayFeeCredits, replayed>>

Reject ==
    /\ phase = "Admission"
    /\ mintRounds = 1
    /\ cursor <= Len(DeployOrder)
    /\ LET deployment == CurrentDeployment
           requested == ReservationVector(deployment)
       IN /\ status[deployment] = "Pending"
          /\ ~VectorLeq(requested, balance)
          /\ status' = [status EXCEPT ![deployment] = "Rejected"]
    /\ cursor' = cursor + 1
    /\ UNCHANGED <<phase, balance, escrow, reservation, burned, feeCredits,
                    mintRounds, protocolMinted, independentCredit,
                    replayCursor, replayBalance, replayEscrow, replayBurned,
                    replayFeeCredits, replayed>>

StartExecution ==
    /\ phase = "Admission"
    /\ cursor > Len(DeployOrder)
    /\ phase' = "Execution"
    /\ UNCHANGED <<cursor, status, balance, escrow, reservation, burned, feeCredits,
                    mintRounds, protocolMinted, independentCredit,
                    replayCursor, replayBalance, replayEscrow, replayBurned,
                    replayFeeCredits, replayed>>

Execute ==
    /\ phase = "Execution"
    /\ \E deployment \in Deployments :
         /\ \/ status[deployment] = "Reserved"
            \/ /\ AllowExecuteBeforeReserve
               /\ status[deployment] \in {"Pending", "PartiallyReserved"}
         /\ status' = [status EXCEPT ![deployment] = "Executed"]
    /\ UNCHANGED <<phase, cursor, balance, escrow, reservation, burned, feeCredits,
                    mintRounds, protocolMinted, independentCredit,
                    replayCursor, replayBalance, replayEscrow, replayBurned,
                    replayFeeCredits, replayed>>

Settle ==
    /\ phase = "Execution"
    /\ \E deployment \in Deployments :
         /\ status[deployment] = "Executed"
         /\ VectorLeq(Settlement(deployment), reservation[deployment])
         /\ LET fullRefund == Refund(deployment)
                refund ==
                  IF AllowRefundLoss
                  THEN [payer \in Payers |->
                         IF fullRefund[payer] > 0
                         THEN fullRefund[payer] - 1
                         ELSE 0]
                  ELSE fullRefund
            IN /\ balance' =
                       VectorAdd(VectorAdd(balance, refund), FeeCredit(deployment))
               /\ escrow' = VectorSub(escrow, reservation[deployment])
               /\ burned' = VectorAdd(burned, RealizedCost[deployment])
               /\ feeCredits' = VectorAdd(feeCredits, FeeCredit(deployment))
         /\ status' = [status EXCEPT ![deployment] = "Settled"]
    /\ UNCHANGED <<phase, cursor, reservation, mintRounds, protocolMinted,
                    independentCredit, replayCursor, replayBalance,
                    replayEscrow, replayBurned, replayFeeCredits, replayed>>

StartReplay ==
    /\ phase = "Execution"
    /\ \A deployment \in Deployments :
         status[deployment] \in {"Settled", "Rejected"}
    /\ phase' = "Replay"
    /\ replayBalance' = VectorAdd(InitialBalance, protocolMinted)
    /\ UNCHANGED <<cursor, status, balance, escrow, reservation, burned, feeCredits,
                    mintRounds, protocolMinted, independentCredit,
                    replayCursor, replayEscrow, replayBurned, replayFeeCredits,
                    replayed>>

ReplayOne ==
    /\ phase = "Replay"
    /\ replayCursor <= Len(DeployOrder)
    /\ LET deployment == DeployOrder[replayCursor]
       IN /\ status[deployment] = "Settled"
          /\ replayBalance' = VectorAdd(
                VectorSub(replayBalance, Settlement(deployment)),
                FeeCredit(deployment))
          /\ replayBurned' =
                VectorAdd(replayBurned, RealizedCost[deployment])
          /\ replayFeeCredits' =
                VectorAdd(replayFeeCredits, FeeCredit(deployment))
          /\ replayed' = replayed \cup {deployment}
    /\ replayCursor' = replayCursor + 1
    /\ UNCHANGED <<phase, cursor, status, balance, escrow, reservation, burned,
                    feeCredits, mintRounds, protocolMinted, independentCredit,
                    replayEscrow>>

ReplayRejected ==
    /\ phase = "Replay"
    /\ replayCursor <= Len(DeployOrder)
    /\ status[DeployOrder[replayCursor]] = "Rejected"
    /\ replayCursor' = replayCursor + 1
    /\ UNCHANGED <<phase, cursor, status, balance, escrow, reservation, burned,
                    feeCredits, mintRounds, protocolMinted, independentCredit,
                    replayBalance, replayEscrow, replayBurned, replayFeeCredits,
                    replayed>>

ReplayOmit ==
    /\ AllowReplayOmission
    /\ phase = "Replay"
    /\ replayCursor <= Len(DeployOrder)
    /\ status[DeployOrder[replayCursor]] = "Settled"
    /\ replayCursor' = replayCursor + 1
    /\ UNCHANGED <<phase, cursor, status, balance, escrow, reservation, burned,
                    feeCredits, mintRounds, protocolMinted, independentCredit,
                    replayBalance, replayEscrow, replayBurned, replayFeeCredits,
                    replayed>>

FinishReplay ==
    /\ phase = "Replay"
    /\ replayCursor > Len(DeployOrder)
    /\ phase' = "Done"
    /\ UNCHANGED <<cursor, status, balance, escrow, reservation, burned, feeCredits,
                    mintRounds, protocolMinted, independentCredit,
                    replayCursor, replayBalance, replayEscrow, replayBurned,
                    replayFeeCredits, replayed>>

Next ==
    \/ Mint
    \/ CreateIndependentCredit
    \/ Reserve
    \/ ReservePartial
    \/ Reject
    \/ StartExecution
    \/ Execute
    \/ Settle
    \/ StartReplay
    \/ ReplayOne
    \/ ReplayRejected
    \/ ReplayOmit
    \/ FinishReplay

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"Admission", "Execution", "Replay", "Done"}
    /\ cursor \in Nat
    /\ status \in [Deployments ->
         {"Pending", "Reserved", "PartiallyReserved", "Executed",
          "Settled", "Rejected"}]
    /\ balance \in [Payers -> Nat]
    /\ escrow \in [Payers -> Nat]
    /\ reservation \in [Deployments -> [Payers -> Nat]]
    /\ burned \in [Payers -> Nat]
    /\ feeCredits \in [Payers -> Nat]
    /\ mintRounds \in Nat
    /\ protocolMinted \in [Payers -> Nat]
    /\ independentCredit \in [Payers -> Nat]
    /\ replayCursor \in Nat
    /\ replayBalance \in [Payers -> Nat]
    /\ replayEscrow \in [Payers -> Nat]
    /\ replayBurned \in [Payers -> Nat]
    /\ replayFeeCredits \in [Payers -> Nat]
    /\ replayed \subseteq Deployments

SingleCanonicalLedger == independentCredit = ZeroPayers

MintOccursAtMostOnce == mintRounds <= 1

ReservationMatchesCertificate ==
    \A deployment \in Deployments :
      status[deployment] \in
        {"Reserved", "PartiallyReserved", "Executed", "Settled"}
        => reservation[deployment] = CertifiedBound[deployment]

ReservationUsesAuthorizedPayers ==
    \A deployment \in Deployments :
      \A payer \in Payers \ AuthorizedPayers[deployment] :
        reservation[deployment][payer] = 0

ExecutionRequiresCompleteReservation ==
    \A deployment \in Deployments :
      status[deployment] \in {"Executed", "Settled"}
        => reservation[deployment] = CertifiedBound[deployment]

RealizedNeverExceedsReservation ==
    \A deployment \in Deployments :
      status[deployment] = "Settled"
        => VectorLeq(Settlement(deployment), reservation[deployment])

CanonicalValueConserved ==
    SumSet(InitialBalance, Payers) + SumSet(protocolMinted, Payers)
      = SumSet(balance, Payers) + SumSet(escrow, Payers) + SumSet(burned, Payers)

FeeCreditsAreCanonicalTransfers ==
    SumSet(feeCredits, Payers)
      = SumSet(
          [deployment \in Deployments |->
            IF status[deployment] = "Settled"
            THEN SumSet(RealizedFee[deployment], Payers)
            ELSE 0],
          Deployments)

SettledReservationsLeaveEscrow ==
    phase \in {"Replay", "Done"} => escrow = ZeroPayers

ReplayMatchesCommit ==
    phase = "Done" =>
      /\ replayBalance = balance
      /\ replayEscrow = escrow
      /\ replayBurned = burned
      /\ replayFeeCredits = feeCredits
      /\ replayed = {deployment \in Deployments : status[deployment] = "Settled"}

EventuallyDone == <> (phase = "Done")

=============================================================================
