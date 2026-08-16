------------- MODULE MCAtomicVaultSettlementRefinement -------------
EXTENDS AtomicVaultSettlementRefinement

CONSTANTS alice, bob, validator, left, right, none

PayersDef == {alice, bob, validator}
DeploymentsDef == {left, right}

InitialBalanceDef ==
    [payer \in PayersDef |->
      CASE payer = alice -> 8
        [] payer = bob -> 6
        [] OTHER -> 2]

CertifiedBoundDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = alice -> 5
          [] deployment = left /\ payer = validator -> 1
          [] deployment = right /\ payer = alice -> 4
          [] deployment = right /\ payer = bob -> 3
          [] deployment = right /\ payer = validator -> 1
          [] OTHER -> 0]]

RealizedBurnDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = alice -> 2
          [] deployment = left /\ payer = validator -> 1
          [] deployment = right /\ payer = alice -> 1
          [] deployment = right /\ payer = bob -> 1
          [] deployment = right /\ payer = validator -> 1
          [] OTHER -> 0]]

RealizedFeeDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = alice -> 1
          [] deployment = right /\ payer = alice -> 1
          [] OTHER -> 0]]

FeeRecipientDef ==
    [deployment \in DeploymentsDef |-> validator]

=============================================================================
