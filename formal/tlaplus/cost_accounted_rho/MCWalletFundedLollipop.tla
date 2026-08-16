---------------------- MODULE MCWalletFundedLollipop ----------------------
EXTENDS WalletFundedLollipop

CONSTANTS
    sponsorPurse,
    slotPurse,
    gatewayPurse,
    attackerCaller,
    proposerPurse,
    validatorA,
    validatorB,
    slotAddressName,
    slotCapabilityName,
    noPayer,
    noCaller

PayersDef == {sponsorPurse, slotPurse, gatewayPurse, proposerPurse}
CallersDef == {gatewayPurse, attackerCaller}
ValidatorsDef == {validatorA, validatorB}
NamesDef == {slotAddressName, slotCapabilityName}

InitialBalanceDef ==
    [payer \in PayersDef |->
      CASE payer = sponsorPurse -> 10
        [] payer = slotPurse -> 0
        [] payer = gatewayPurse -> 5
        [] OTHER -> 0]

=============================================================================
