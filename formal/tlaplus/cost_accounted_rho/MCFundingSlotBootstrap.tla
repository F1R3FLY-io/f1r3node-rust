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
    slotPurse,
    \* Added by G3 step S11 (DR-135 item 13): the caller purse.
    \* @type: Str;
    callerPurse

\* Changed by G3 step S11 (DR-135 item 13): the caller purse.
\* PursesDef == {installerPurse, sponsorPurse, outerPurse, slotPurse}
PursesDef == {installerPurse, sponsorPurse, outerPurse, slotPurse, callerPurse}

\* Changed by G3 step S11 (DR-135 item 13): the caller purse holds exactly
\* its hold, CallerLimit * CallerPrice. So the caller can pay its own work
\* but not the whole run.
\* InitialBalanceDef ==
\*     [purse \in PursesDef |->
\*       CASE purse = installerPurse -> 4
\*         [] purse = sponsorPurse -> 8
\*         [] OTHER -> 0]
InitialBalanceDef ==
    [purse \in PursesDef |->
      CASE purse = installerPurse -> 4
        [] purse = sponsorPurse -> 8
        [] purse = callerPurse -> CallerLimit * CallerPrice
        [] OTHER -> 0]

=============================================================================
