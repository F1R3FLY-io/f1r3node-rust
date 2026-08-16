---------------- MODULE AtomicVaultSettlementRefinement ----------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    Payers,
    Deployments,
    InitialBalance,
    CertifiedBound,
    RealizedBurn,
    RealizedFee,
    FeeRecipient,
    NoDeployment,
    ExposeReservationCell

ASSUME /\ Payers # {}
       /\ Deployments # {}
       /\ InitialBalance \in [Payers -> Nat]
       /\ CertifiedBound \in [Deployments -> [Payers -> Nat]]
       /\ RealizedBurn \in [Deployments -> [Payers -> Nat]]
       /\ RealizedFee \in [Deployments -> [Payers -> Nat]]
       /\ FeeRecipient \in [Deployments -> Payers]
       /\ NoDeployment \notin Deployments
       /\ ExposeReservationCell \in BOOLEAN
       /\ \A deployment \in Deployments :
            \A payer \in Payers :
              RealizedBurn[deployment][payer]
                + RealizedFee[deployment][payer]
                  <= CertifiedBound[deployment][payer]

VARIABLES
    phase,
    selected,
    finalized,
    rejected,
    balance,
    burned,
    feeCredits,
    replayBalance,
    replayBurned,
    replayFeeCredits,
    reservationCell

vars == <<phase, selected, finalized, rejected, balance, burned, feeCredits,
          replayBalance, replayBurned, replayFeeCredits, reservationCell>>

RECURSIVE SumSet(_, _)

SumSet(function, domain) ==
    IF domain = {}
    THEN 0
    ELSE LET element == CHOOSE value \in domain : TRUE
         IN function[element] + SumSet(function, domain \ {element})

Settlement(deployment, payer) ==
    RealizedBurn[deployment][payer] + RealizedFee[deployment][payer]

AggregateSettlement(deployments, payer) ==
    SumSet([deployment \in deployments |-> Settlement(deployment, payer)], deployments)

AggregateBurn(deployments, payer) ==
    SumSet([deployment \in deployments |-> RealizedBurn[deployment][payer]], deployments)

AggregateFee(deployments) ==
    SumSet(
      [deployment \in deployments |->
        SumSet(RealizedFee[deployment], Payers)],
      deployments)

FeeCredit(deployments, payer) ==
    SumSet(
      [deployment \in deployments |->
        IF FeeRecipient[deployment] = payer
        THEN SumSet(RealizedFee[deployment], Payers)
        ELSE 0],
      deployments)

IndividuallyAdmissible(deployment) ==
    \A payer \in Payers :
      CertifiedBound[deployment][payer] <= InitialBalance[payer]

AggregateAdmissible(deployments) ==
    \A payer \in Payers :
      AggregateSettlement(deployments, payer) <= InitialBalance[payer]

VisibleBalance(deployments) ==
    [payer \in Payers |->
      InitialBalance[payer]
        - AggregateSettlement(deployments, payer)
        + FeeCredit(deployments, payer)]

VisibleBurn(deployments) ==
    [payer \in Payers |-> AggregateBurn(deployments, payer)]

VisibleFeeCredit(deployments) ==
    [payer \in Payers |-> FeeCredit(deployments, payer)]

Init ==
    /\ phase = "Selection"
    /\ selected = {}
    /\ finalized = {}
    /\ rejected = {}
    /\ balance = InitialBalance
    /\ burned = [payer \in Payers |-> 0]
    /\ feeCredits = [payer \in Payers |-> 0]
    /\ replayBalance = InitialBalance
    /\ replayBurned = [payer \in Payers |-> 0]
    /\ replayFeeCredits = [payer \in Payers |-> 0]
    /\ reservationCell = NoDeployment

Select(deployment) ==
    /\ phase = "Selection"
    /\ deployment \in Deployments \ selected
    /\ IndividuallyAdmissible(deployment)
    /\ selected' = selected \cup {deployment}
    /\ reservationCell' =
         IF ExposeReservationCell THEN deployment ELSE reservationCell
    /\ UNCHANGED <<phase, finalized, rejected, balance, burned, feeCredits,
                    replayBalance, replayBurned, replayFeeCredits>>

Finalize ==
    /\ phase = "Selection"
    /\ selected = Deployments
    /\ IF AggregateAdmissible(selected)
       THEN /\ finalized' = selected
            /\ rejected' = {}
            /\ balance' = VisibleBalance(selected)
            /\ burned' = VisibleBurn(selected)
            /\ feeCredits' = VisibleFeeCredit(selected)
       ELSE /\ finalized' = {}
            /\ rejected' = selected
            /\ balance' = InitialBalance
            /\ burned' = [payer \in Payers |-> 0]
            /\ feeCredits' = [payer \in Payers |-> 0]
    /\ phase' = "Replay"
    /\ UNCHANGED <<selected, replayBalance, replayBurned, replayFeeCredits,
                    reservationCell>>

Replay ==
    /\ phase = "Replay"
    /\ replayBalance' =
         IF finalized = {} THEN InitialBalance ELSE VisibleBalance(finalized)
    /\ replayBurned' =
         IF finalized = {}
         THEN [payer \in Payers |-> 0]
         ELSE VisibleBurn(finalized)
    /\ replayFeeCredits' =
         IF finalized = {}
         THEN [payer \in Payers |-> 0]
         ELSE VisibleFeeCredit(finalized)
    /\ phase' = "Done"
    /\ UNCHANGED <<selected, finalized, rejected, balance, burned, feeCredits,
                    reservationCell>>

Next ==
    \/ \E deployment \in Deployments : Select(deployment)
    \/ Finalize
    \/ Replay

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"Selection", "Replay", "Done"}
    /\ selected \subseteq Deployments
    /\ finalized \subseteq Deployments
    /\ rejected \subseteq Deployments
    /\ balance \in [Payers -> Nat]
    /\ burned \in [Payers -> Nat]
    /\ feeCredits \in [Payers -> Nat]
    /\ replayBalance \in [Payers -> Nat]
    /\ replayBurned \in [Payers -> Nat]
    /\ replayFeeCredits \in [Payers -> Nat]
    /\ reservationCell \in Deployments \cup {NoDeployment}

NoPersistentReservationState == reservationCell = NoDeployment

EverySelectedBranchWasStateBoundFunded ==
    \A deployment \in selected : IndividuallyAdmissible(deployment)

FinalizedAggregateIsFunded ==
    finalized # {} => AggregateAdmissible(finalized)

AtomicVisibleRefinement ==
    /\ balance =
         IF finalized = {} THEN InitialBalance ELSE VisibleBalance(finalized)
    /\ burned =
         IF finalized = {}
         THEN [payer \in Payers |-> 0]
         ELSE VisibleBurn(finalized)
    /\ feeCredits =
         IF finalized = {}
         THEN [payer \in Payers |-> 0]
         ELSE VisibleFeeCredit(finalized)

CanonicalValueConserved ==
    SumSet(balance, Payers) + SumSet(burned, Payers)
      = SumSet(InitialBalance, Payers)

FeeCreditIsAConservingTransfer ==
    SumSet(feeCredits, Payers) = AggregateFee(finalized)

RejectedAggregateHasNoEffect ==
    rejected # {} =>
      /\ balance = InitialBalance
      /\ burned = [payer \in Payers |-> 0]
      /\ feeCredits = [payer \in Payers |-> 0]

ReplayMatchesFinalizedState ==
    phase = "Done" =>
      /\ replayBalance = balance
      /\ replayBurned = burned
      /\ replayFeeCredits = feeCredits

=============================================================================
