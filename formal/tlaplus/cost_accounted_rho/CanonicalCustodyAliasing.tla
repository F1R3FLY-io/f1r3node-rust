------------------------ MODULE CanonicalCustodyAliasing ------------------------
EXTENDS Naturals, FiniteSets, Sequences, TLC, Apalache

CONSTANTS
    \* @type: Set(Str);
    Lanes,
    \* @type: Set(Str);
    Purses,
    \* @type: Set(Str);
    Events,
    \* @type: Str -> Str;
    PurseOf,
    \* @type: Str -> Str;
    EventLane,
    \* @type: Str -> Bool;
    EventUsesStack,
    \* @type: Str -> Int;
    InitialBalance,
    \* @type: Str -> Int;
    InitialStack,
    \* @type: Int -> Str;
    EventOrder,
    \* @type: Bool;
    DuplicateLaneCapacity

ASSUME /\ Lanes # {}
       /\ Purses # {}
       /\ Events # {}
       /\ PurseOf \in [Lanes -> Purses]
       /\ EventLane \in [Events -> Lanes]
       /\ EventUsesStack \in [Events -> BOOLEAN]
       /\ InitialBalance \in [Purses -> Nat]
       /\ InitialStack \in [Lanes -> Nat]
       /\ Cardinality(Events) = 4
       /\ EventOrder \in [1..4 -> Events]
       /\ {EventOrder[index] : index \in 1..4} = Events
       /\ DuplicateLaneCapacity \in BOOLEAN
       /\ \E left, right \in Lanes : left # right /\ PurseOf[left] = PurseOf[right]

VARIABLES
    \* @type: Int;
    playIndex,
    \* @type: Set(Str);
    playAccepted,
    \* @type: Set(Str);
    playRejected,
    \* @type: Str -> Int;
    playVaultByLane,
    \* @type: Str -> Int;
    playVaultByPurse,
    \* @type: Str -> Int;
    playStackByLane,
    \* @type: Int;
    replayIndex,
    \* @type: Set(Str);
    replayAccepted,
    \* @type: Set(Str);
    replayRejected,
    \* @type: Str -> Int;
    replayVaultByLane,
    \* @type: Str -> Int;
    replayVaultByPurse,
    \* @type: Str -> Int;
    replayStackByLane

vars ==
    <<playIndex, playAccepted, playRejected, playVaultByLane,
      playVaultByPurse, playStackByLane, replayIndex, replayAccepted,
      replayRejected, replayVaultByLane, replayVaultByPurse,
      replayStackByLane>>

ZeroLanes == [lane \in Lanes |-> 0]
ZeroPurses == [purse \in Purses |-> 0]

\* @type: (Str -> Int, Set(Str)) => Int;
SumSet(function, domain) ==
    LET AddValue(total, element) == total + function[element]
    IN ApaFoldSet(AddValue, 0, domain)

Prefix(index) ==
    CASE index = 1 -> {}
      [] index = 2 -> {EventOrder[1]}
      [] index = 3 -> {EventOrder[1], EventOrder[2]}
      [] index = 4 -> {EventOrder[1], EventOrder[2], EventOrder[3]}
      [] OTHER -> Events

LaneVaultTotal(byLane, purse) ==
    SumSet(
      [lane \in Lanes |-> IF PurseOf[lane] = purse THEN byLane[lane] ELSE 0],
      Lanes)

AcceptedVaultCount(accepted, purse) ==
    Cardinality(
      {event \in accepted :
        /\ ~EventUsesStack[event]
        /\ PurseOf[EventLane[event]] = purse})

AcceptedLaneVaultCount(accepted, lane) ==
    Cardinality(
      {event \in accepted :
        /\ ~EventUsesStack[event]
        /\ EventLane[event] = lane})

AcceptedStackCount(accepted, lane) ==
    Cardinality(
      {event \in accepted :
        /\ EventUsesStack[event]
        /\ EventLane[event] = lane})

PlayCanUseVault(event) ==
    LET lane == EventLane[event]
        purse == PurseOf[lane]
    IN IF DuplicateLaneCapacity
       THEN playVaultByLane[lane] < InitialBalance[purse]
       ELSE playVaultByPurse[purse] < InitialBalance[purse]

ReplayCanUseVault(event) ==
    LET lane == EventLane[event]
        purse == PurseOf[lane]
    IN IF DuplicateLaneCapacity
       THEN replayVaultByLane[lane] < InitialBalance[purse]
       ELSE replayVaultByPurse[purse] < InitialBalance[purse]

PlayCanAccept(event) ==
    IF EventUsesStack[event]
    THEN playStackByLane[EventLane[event]] < InitialStack[EventLane[event]]
    ELSE PlayCanUseVault(event)

ReplayCanAccept(event) ==
    IF EventUsesStack[event]
    THEN replayStackByLane[EventLane[event]] < InitialStack[EventLane[event]]
    ELSE ReplayCanUseVault(event)

Init ==
    /\ playIndex = 1
    /\ playAccepted = {}
    /\ playRejected = {}
    /\ playVaultByLane = ZeroLanes
    /\ playVaultByPurse = ZeroPurses
    /\ playStackByLane = ZeroLanes
    /\ replayIndex = 1
    /\ replayAccepted = {}
    /\ replayRejected = {}
    /\ replayVaultByLane = ZeroLanes
    /\ replayVaultByPurse = ZeroPurses
    /\ replayStackByLane = ZeroLanes

PlayStep ==
    /\ playIndex <= 4
    /\ LET event == EventOrder[playIndex]
           lane == EventLane[event]
           purse == PurseOf[lane]
           accepted == PlayCanAccept(event)
       IN /\ playAccepted' = IF accepted THEN playAccepted \cup {event} ELSE playAccepted
          /\ playRejected' = IF accepted THEN playRejected ELSE playRejected \cup {event}
          /\ playVaultByLane' =
               IF accepted /\ ~EventUsesStack[event]
               THEN [playVaultByLane EXCEPT ![lane] = @ + 1]
               ELSE playVaultByLane
          /\ playVaultByPurse' =
               IF accepted /\ ~EventUsesStack[event]
               THEN [playVaultByPurse EXCEPT ![purse] = @ + 1]
               ELSE playVaultByPurse
          /\ playStackByLane' =
               IF accepted /\ EventUsesStack[event]
               THEN [playStackByLane EXCEPT ![lane] = @ + 1]
               ELSE playStackByLane
    /\ playIndex' = playIndex + 1
    /\ UNCHANGED <<replayIndex, replayAccepted, replayRejected,
                    replayVaultByLane, replayVaultByPurse, replayStackByLane>>

ReplayStep ==
    /\ replayIndex <= 4
    /\ LET event == EventOrder[replayIndex]
           lane == EventLane[event]
           purse == PurseOf[lane]
           accepted == ReplayCanAccept(event)
       IN /\ replayAccepted' = IF accepted THEN replayAccepted \cup {event} ELSE replayAccepted
          /\ replayRejected' = IF accepted THEN replayRejected ELSE replayRejected \cup {event}
          /\ replayVaultByLane' =
               IF accepted /\ ~EventUsesStack[event]
               THEN [replayVaultByLane EXCEPT ![lane] = @ + 1]
               ELSE replayVaultByLane
          /\ replayVaultByPurse' =
               IF accepted /\ ~EventUsesStack[event]
               THEN [replayVaultByPurse EXCEPT ![purse] = @ + 1]
               ELSE replayVaultByPurse
          /\ replayStackByLane' =
               IF accepted /\ EventUsesStack[event]
               THEN [replayStackByLane EXCEPT ![lane] = @ + 1]
               ELSE replayStackByLane
    /\ replayIndex' = replayIndex + 1
    /\ UNCHANGED <<playIndex, playAccepted, playRejected, playVaultByLane,
                    playVaultByPurse, playStackByLane>>

Next == PlayStep \/ ReplayStep

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ playIndex \in 1..5
    /\ replayIndex \in 1..5
    /\ playAccepted \subseteq Events
    /\ playRejected \subseteq Events
    /\ replayAccepted \subseteq Events
    /\ replayRejected \subseteq Events
    /\ playVaultByLane \in [Lanes -> Nat]
    /\ replayVaultByLane \in [Lanes -> Nat]
    /\ playVaultByPurse \in [Purses -> Nat]
    /\ replayVaultByPurse \in [Purses -> Nat]
    /\ playStackByLane \in [Lanes -> Nat]
    /\ replayStackByLane \in [Lanes -> Nat]

CanonicalOrderProgress ==
    /\ playAccepted \cap playRejected = {}
    /\ playAccepted \cup playRejected = Prefix(playIndex)
    /\ replayAccepted \cap replayRejected = {}
    /\ replayAccepted \cup replayRejected = Prefix(replayIndex)

ExactLogicalAttribution ==
    /\ \A lane \in Lanes :
         /\ playVaultByLane[lane] = AcceptedLaneVaultCount(playAccepted, lane)
         /\ replayVaultByLane[lane] = AcceptedLaneVaultCount(replayAccepted, lane)
         /\ playStackByLane[lane] = AcceptedStackCount(playAccepted, lane)
         /\ replayStackByLane[lane] = AcceptedStackCount(replayAccepted, lane)
    /\ \A purse \in Purses :
         /\ playVaultByPurse[purse] = AcceptedVaultCount(playAccepted, purse)
         /\ replayVaultByPurse[purse] = AcceptedVaultCount(replayAccepted, purse)
         /\ LaneVaultTotal(playVaultByLane, purse) = playVaultByPurse[purse]
         /\ LaneVaultTotal(replayVaultByLane, purse) = replayVaultByPurse[purse]

NoDoubleCapacity ==
    /\ \A purse \in Purses : playVaultByPurse[purse] <= InitialBalance[purse]
    /\ \A purse \in Purses : replayVaultByPurse[purse] <= InitialBalance[purse]

CanonicalCustodyConserved ==
    /\ \A purse \in Purses :
         InitialBalance[purse] - playVaultByPurse[purse]
           + playVaultByPurse[purse] = InitialBalance[purse]
    /\ \A purse \in Purses :
         InitialBalance[purse] - replayVaultByPurse[purse]
           + replayVaultByPurse[purse] = InitialBalance[purse]

StackAuthorityPreserved ==
    /\ \A lane \in Lanes :
         /\ playStackByLane[lane] <= InitialStack[lane]
         /\ replayStackByLane[lane] <= InitialStack[lane]
         /\ InitialStack[lane] - playStackByLane[lane]
              + playStackByLane[lane] = InitialStack[lane]
         /\ InitialStack[lane] - replayStackByLane[lane]
              + replayStackByLane[lane] = InitialStack[lane]

ConcurrentReplayDeterministic ==
    playIndex = replayIndex =>
      /\ playAccepted = replayAccepted
      /\ playRejected = replayRejected
      /\ playVaultByLane = replayVaultByLane
      /\ playVaultByPurse = replayVaultByPurse
      /\ playStackByLane = replayStackByLane

CompletedReplayMatchesPlay ==
    (/\ playIndex = 5
     /\ replayIndex = 5)
    => /\ playAccepted = replayAccepted
       /\ playRejected = replayRejected
       /\ playVaultByLane = replayVaultByLane
       /\ playVaultByPurse = replayVaultByPurse
       /\ playStackByLane = replayStackByLane

=============================================================================
