---------------- MODULE AtomicVaultSettlementRefinement ----------------
EXTENDS Naturals, FiniteSets, TLC, Apalache

CONSTANTS
    \* @type: Set(Str);
    Payers,
    \* @type: Set(Str);
    Deployments,
    \* @type: Str -> Int;
    InitialBalance,
    \* @type: Str -> (Str -> Int);
    CertifiedBound,
    \* @type: Str -> (Str -> Int);
    ApplicationDebit,
    \* @type: Str -> (Str -> Int);
    ApplicationCredit,
    \* @type: Str -> (Str -> Int);
    RealizedBurn,
    \* @type: Str -> (Str -> Int);
    RealizedByteBurn,
    \* @type: Str -> (Str -> Int);
    RealizedFee,
    \* @type: Str -> Str;
    FeeRecipient,
    \* @type: Str;
    NoDeployment,
    \* @type: Bool;
    ExposeReservationCell,
    \* @type: Bool;
    OmitApplicationDebit,
    \* @type: Bool;
    OmitPhysicalBurn,
    \* @type: Bool;
    OmitByteBurn,
    \* @type: Bool;
    OmitFee

ASSUME /\ Payers # {}
       /\ Deployments # {}
       /\ InitialBalance \in [Payers -> Nat]
       /\ CertifiedBound \in [Deployments -> [Payers -> Nat]]
       /\ ApplicationDebit \in [Deployments -> [Payers -> Nat]]
       /\ ApplicationCredit \in [Deployments -> [Payers -> Nat]]
       /\ RealizedBurn \in [Deployments -> [Payers -> Nat]]
       /\ RealizedByteBurn \in [Deployments -> [Payers -> Nat]]
       /\ RealizedFee \in [Deployments -> [Payers -> Nat]]
       /\ FeeRecipient \in [Deployments -> Payers]
       /\ NoDeployment \notin Deployments
       /\ ExposeReservationCell \in BOOLEAN
       /\ OmitApplicationDebit \in BOOLEAN
       /\ OmitPhysicalBurn \in BOOLEAN
       /\ OmitByteBurn \in BOOLEAN
       /\ OmitFee \in BOOLEAN
       /\ \A deployment \in Deployments :
            \A payer \in Payers :
              RealizedBurn[deployment][payer]
                + RealizedByteBurn[deployment][payer]
                + RealizedFee[deployment][payer]
                  <= CertifiedBound[deployment][payer]

VARIABLES
    \* @type: Str;
    phase,
    \* @type: Set(Str);
    selected,
    \* @type: Set(Str);
    finalized,
    \* @type: Set(Str);
    rejected,
    \* @type: Str -> Int;
    balance,
    \* @type: Str -> Int;
    burned,
    \* @type: Str -> Int;
    feeCredits,
    \* @type: Str -> Int;
    replayBalance,
    \* @type: Str -> Int;
    replayBurned,
    \* @type: Str -> Int;
    replayFeeCredits,
    \* @type: Str;
    reservationCell

vars == <<phase, selected, finalized, rejected, balance, burned, feeCredits,
          replayBalance, replayBurned, replayFeeCredits, reservationCell>>

\* @type: (Str -> Int, Set(Str)) => Int;
SumSet(function, domain) ==
    LET AddValue(total, element) == total + function[element]
    IN ApaFoldSet(AddValue, 0, domain)

ASSUME \A deployment \in Deployments :
         SumSet(ApplicationDebit[deployment], Payers)
           = SumSet(ApplicationCredit[deployment], Payers)

CompleteSettlement(deployment, payer) ==
    ApplicationDebit[deployment][payer]
      + RealizedBurn[deployment][payer]
      + RealizedByteBurn[deployment][payer]
      + RealizedFee[deployment][payer]

DecisionSettlement(deployment, payer) ==
    (IF OmitApplicationDebit THEN 0 ELSE ApplicationDebit[deployment][payer])
      + (IF OmitPhysicalBurn THEN 0 ELSE RealizedBurn[deployment][payer])
      + (IF OmitByteBurn THEN 0 ELSE RealizedByteBurn[deployment][payer])
      + (IF OmitFee THEN 0 ELSE RealizedFee[deployment][payer])

AggregateSettlement(deployments, payer) ==
    SumSet(
      [deployment \in deployments |-> DecisionSettlement(deployment, payer)],
      deployments)

AggregateCompleteSettlement(deployments, payer) ==
    SumSet(
      [deployment \in deployments |-> CompleteSettlement(deployment, payer)],
      deployments)

AggregateBurn(deployments, payer) ==
    SumSet(
      [deployment \in deployments |->
        RealizedBurn[deployment][payer]
          + RealizedByteBurn[deployment][payer]],
      deployments)

AggregateFee(deployments) ==
    SumSet(
      [deployment \in deployments |->
        SumSet(RealizedFee[deployment], Payers)],
      deployments)

ApplicationCreditTo(deployments, payer) ==
    SumSet(
      [deployment \in deployments |-> ApplicationCredit[deployment][payer]],
      deployments)

FeeCredit(deployments, payer) ==
    SumSet(
      [deployment \in deployments |->
        IF FeeRecipient[deployment] = payer
        THEN SumSet(RealizedFee[deployment], Payers)
        ELSE 0],
      deployments)

CompletelyIndividuallyAdmissible(deployment) ==
    \A payer \in Payers :
      ApplicationDebit[deployment][payer]
        + CertifiedBound[deployment][payer]
          <= InitialBalance[payer]

IndividuallyAdmissible(deployment) ==
    CompletelyIndividuallyAdmissible(deployment)

AggregateAdmissible(deployments) ==
    \A payer \in Payers :
      AggregateSettlement(deployments, payer) <= InitialBalance[payer]

CompleteAggregateAdmissible(deployments) ==
    \A payer \in Payers :
      AggregateCompleteSettlement(deployments, payer) <= InitialBalance[payer]

VisibleBalance(deployments) ==
    [payer \in Payers |->
      InitialBalance[payer]
        - AggregateCompleteSettlement(deployments, payer)
        + ApplicationCreditTo(deployments, payer)
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
    \A deployment \in selected : CompletelyIndividuallyAdmissible(deployment)

FinalizedAggregateIsFunded ==
    finalized # {} => CompleteAggregateAdmissible(finalized)

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
