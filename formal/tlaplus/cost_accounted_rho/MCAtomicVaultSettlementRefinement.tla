------------- MODULE MCAtomicVaultSettlementRefinement -------------
EXTENDS AtomicVaultSettlementRefinement

CONSTANTS
    \* @type: Str;
    alice,
    \* @type: Str;
    bob,
    \* @type: Str;
    validator,
    \* @type: Str;
    left,
    \* @type: Str;
    right,
    \* @type: Str;
    none

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

ApplicationDebitDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = alice -> 2
          [] deployment = right /\ payer = alice -> 1
          [] OTHER -> 0]]

ApplicationCreditDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = bob -> 2
          [] deployment = right /\ payer = bob -> 1
          [] OTHER -> 0]]

ApplicationDebitOverdrawDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = alice -> 2
          [] deployment = right /\ payer = alice -> 2
          [] OTHER -> 0]]

ApplicationCreditOverdrawDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = bob -> 2
          [] deployment = right /\ payer = bob -> 2
          [] OTHER -> 0]]

RealizedBurnDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = alice -> 1
          [] deployment = left /\ payer = validator -> 1
          [] deployment = right /\ payer = alice -> 1
          [] deployment = right /\ payer = bob -> 1
          [] deployment = right /\ payer = validator -> 1
          [] OTHER -> 0]]

RealizedBurnOverdrawDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = alice -> 2
          [] deployment = left /\ payer = validator -> 1
          [] deployment = right /\ payer = alice -> 1
          [] deployment = right /\ payer = bob -> 1
          [] deployment = right /\ payer = validator -> 1
          [] OTHER -> 0]]

RealizedByteBurnDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = alice -> 1
          [] OTHER -> 0]]

RealizedByteBurnOverdrawDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = alice -> 2
          [] OTHER -> 0]]

RealizedFeeDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = alice -> 1
          [] deployment = right /\ payer = alice -> 1
          [] OTHER -> 0]]

RealizedFeeOverdrawDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = left /\ payer = alice -> 2
          [] deployment = right /\ payer = alice -> 1
          [] OTHER -> 0]]

FeeRecipientDef ==
    [deployment \in DeploymentsDef |-> validator]

=============================================================================
