-------------------- MODULE MCVaultBackedCostLifecycle ---------------------
EXTENDS VaultBackedCostLifecycle

CONSTANTS client, provider, slot, sponsor, call, slotCall, none

PayersDef == {client, provider, slot, sponsor}
DeploymentsDef == {call, slotCall}
DeployOrderDef == <<call, slotCall>>

InitialBalanceDef ==
    [payer \in PayersDef |->
      CASE payer = client -> 5
        [] payer = provider -> 4
        [] payer = slot -> 5
        [] OTHER -> 5]

EpochMintDef ==
    [payer \in PayersDef |-> IF payer = provider THEN 1 ELSE 0]

CertifiedBoundDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = call /\ payer = client -> 2
          [] deployment = call /\ payer = provider -> 1
          [] deployment = slotCall /\ payer = slot -> 3
          [] OTHER -> 0]]

RealizedCostDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = call /\ payer = client -> 1
          [] deployment = call /\ payer = provider -> 1
          [] deployment = slotCall /\ payer = slot -> 1
          [] OTHER -> 0]]

RealizedFeeDef ==
    [deployment \in DeploymentsDef |->
      [payer \in PayersDef |->
        CASE deployment = call /\ payer = client -> 1
          [] deployment = slotCall /\ payer = slot -> 1
          [] OTHER -> 0]]

FeeRecipientDef ==
    [deployment \in DeploymentsDef |-> provider]

AuthorizedPayersDef ==
    [deployment \in DeploymentsDef |->
      IF deployment = call THEN {client, provider} ELSE {slot}]

=============================================================================
