---------------------- MODULE MCWalletFundedLollipop ----------------------
EXTENDS WalletFundedLollipop

CONSTANTS
    \* @type: Str;
    sponsorPurse,
    \* @type: Str;
    outerPurse,
    \* @type: Str;
    slotPurse,
    \* @type: Str;
    gatewayPurse,
    \* @type: Str;
    attackerCaller,
    \* @type: Str;
    proposerPurse,
    \* @type: Str;
    validatorA,
    \* @type: Str;
    validatorB,
    \* @type: Str;
    outerAddressName,
    \* @type: Str;
    slotAddressName,
    \* @type: Str;
    slotCapabilityName,
    \* @type: Str;
    noPayer,
    \* @type: Str;
    noCaller

PayersDef == {sponsorPurse, outerPurse, slotPurse, gatewayPurse, proposerPurse}
CallersDef == {gatewayPurse, attackerCaller}
ValidatorsDef == {validatorA, validatorB}
NamesDef == {outerAddressName, slotAddressName, slotCapabilityName}

InitialBalanceDef ==
    [payer \in PayersDef |->
      CASE payer = sponsorPurse -> 10
        [] payer = outerPurse -> 0
        [] payer = slotPurse -> 0
        [] payer = gatewayPurse -> 5
        [] OTHER -> 0]

=============================================================================
