-------------------- MODULE FeeObligationBoundary --------------------
EXTENDS AtomicVaultSettlementRefinement

CONSTANTS Wallets, SelectedFunders, ExampleFeePayer, Receiver,
          MonetaryObligation, PerSignerFee

ASSUME /\ IsFiniteSet(Wallets)
       /\ SelectedFunders \subseteq Wallets
       /\ Cardinality(SelectedFunders) > 1
       /\ ExampleFeePayer \in SelectedFunders
       /\ Receiver \notin Wallets
       /\ MonetaryObligation \in Nat \ {0}
       /\ PerSignerFee \in BOOLEAN

PayersDef == Wallets \cup {Receiver}
DeploymentsDef == {"first", "second"}

InitialBalanceDef ==
    [payer \in PayersDef |->
      IF payer \in Wallets THEN 2 * MonetaryObligation ELSE 0]

CertifiedBoundDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        IF payer \in SelectedFunders THEN MonetaryObligation ELSE 0]]

ZeroAmounts ==
    [deployment \in DeploymentsDef |-> [payer \in PayersDef |-> 0]]

RealizedFeeDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        IF (PerSignerFee /\ payer \in SelectedFunders)
             \/ (~PerSignerFee /\ payer = ExampleFeePayer)
        THEN MonetaryObligation ELSE 0]]

FeeRecipientDef == [deployment \in DeploymentsDef |-> Receiver]

FeeMatchesIndependentObligation ==
    SumSet(feeCredits, Payers) = Cardinality(finalized) * MonetaryObligation

PerSignerExcessRemainsConserved ==
    finalized # {} =>
      /\ SumSet(feeCredits, Payers)
           = Cardinality(finalized) * Cardinality(SelectedFunders)
               * MonetaryObligation
      /\ SumSet(feeCredits, Payers)
           > Cardinality(finalized) * MonetaryObligation

=============================================================================
