---------------------- MODULE MCCanonicalCustodyAliasing ----------------------
EXTENDS CanonicalCustodyAliasing

CONSTANTS
    \* @type: Str;
    laneA,
    \* @type: Str;
    laneB,
    \* @type: Str;
    laneC,
    \* @type: Str;
    purseShared,
    \* @type: Str;
    purseOther,
    \* @type: Str;
    eventA,
    \* @type: Str;
    eventB,
    \* @type: Str;
    eventC,
    \* @type: Str;
    eventD

LanesDef == {laneA, laneB, laneC}
PursesDef == {purseShared, purseOther}
EventsDef == {eventA, eventB, eventC, eventD}

PurseOfDef ==
    [lane \in LanesDef |->
      CASE lane = laneA -> purseShared
        [] lane = laneB -> purseShared
        [] OTHER -> purseOther]

EventLaneDef ==
    [event \in EventsDef |->
      CASE event = eventA -> laneB
        [] event = eventB -> laneA
        [] event = eventC -> laneA
        [] OTHER -> laneC]

EventUsesStackDef ==
    [event \in EventsDef |-> event = eventB]

InitialBalanceDef ==
    [purse \in PursesDef |-> 1]

InitialStackDef ==
    [lane \in LanesDef |-> IF lane = laneA THEN 1 ELSE 0]

\* @type: Int -> Str;
EventOrderDef ==
    [index \in 1..4 |->
      CASE index = 1 -> eventA
        [] index = 2 -> eventB
        [] index = 3 -> eventC
        [] OTHER -> eventD]

=============================================================================
