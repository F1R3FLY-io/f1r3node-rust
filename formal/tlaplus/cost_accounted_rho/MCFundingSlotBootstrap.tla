--------------------- MODULE MCFundingSlotBootstrap ---------------------
EXTENDS FundingSlotBootstrap

CONSTANTS
    \* @type: Str;
    installerPurse,
    \* @type: Str;
    sponsorPurse,
    \* @type: Str;
    outerPurse,
    \* @type: Str;
    slotPurse

PursesDef == {installerPurse, sponsorPurse, outerPurse, slotPurse}

InitialBalanceDef ==
    [purse \in PursesDef |->
      CASE purse = installerPurse -> 4
        [] purse = sponsorPurse -> 8
        [] OTHER -> 0]

=============================================================================
