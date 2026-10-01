---------------------- MODULE AtomicCommAccounting ----------------------
EXTENDS Integers, FiniteSets

CONSTANTS
    Commands,
    Events,
    Requirements,
    Budget,
    ChargeOnIntroduction,
    NoCommand,
    sA, rA, sB, sC, jBC, u, binary, join

ASSUME Commands # {}
ASSUME Events # {}
ASSUME Requirements \in [Events -> SUBSET Commands]
ASSUME \A event \in Events : Requirements[event] # {}
ASSUME \A left, right \in Events :
    left # right => Requirements[left] \cap Requirements[right] = {}
ASSUME NoCommand \notin Commands
ASSUME Budget \in Nat
ASSUME ChargeOnIntroduction \in BOOLEAN

VARIABLES
    attempted,
    pending,
    committed,
    rejected,
    rejectionBefore,
    rejectionTrigger,
    cost,
    replayed,
    replayCost,
    phase

vars == <<attempted, pending, committed, rejected, rejectionBefore,
          rejectionTrigger, cost, replayed, replayCost, phase>>

AllRequired == UNION {Requirements[event] : event \in Events}
UnmatchedCommands == Commands \ AllRequired

Init ==
    /\ attempted = {}
    /\ pending = {}
    /\ committed = {}
    /\ rejected = {}
    /\ rejectionBefore = [event \in Events |-> {}]
    /\ rejectionTrigger = [event \in Events |-> NoCommand]
    /\ cost = 0
    /\ replayed = {}
    /\ replayCost = 0
    /\ phase = "arrival"

Ready(command) ==
    {event \in Events \ committed :
        Requirements[event] \subseteq pending \cup {command}}

StoreIntroduction(command) ==
    /\ attempted' = attempted \cup {command}
    /\ pending' = pending \cup {command}
    /\ UNCHANGED <<committed, rejected, rejectionBefore,
                    rejectionTrigger, replayed, replayCost, phase>>

Commit(event, command, debit) ==
    /\ attempted' = attempted \cup {command}
    /\ pending' = (pending \cup {command}) \ Requirements[event]
    /\ committed' = committed \cup {event}
    /\ cost' = cost + debit
    /\ UNCHANGED <<rejected, rejectionBefore, rejectionTrigger,
                    replayed, replayCost, phase>>

Reject(event, command) ==
    /\ attempted' = attempted \cup {command}
    /\ rejected' = rejected \cup {event}
    /\ rejectionBefore' = [rejectionBefore EXCEPT ![event] = pending]
    /\ rejectionTrigger' = [rejectionTrigger EXCEPT ![event] = command]
    /\ UNCHANGED <<pending, committed, cost, replayed, replayCost, phase>>

Arrival(command) ==
    /\ phase = "arrival"
    /\ command \in Commands \ attempted
    /\ IF ChargeOnIntroduction
          THEN IF Ready(command) = {}
                  THEN StoreIntroduction(command) /\ cost' = cost + 1
                  ELSE \E event \in Ready(command) : Commit(event, command, 1)
          ELSE IF Ready(command) = {}
                  THEN StoreIntroduction(command) /\ UNCHANGED cost
                  ELSE \E event \in Ready(command) :
                      IF cost < Budget
                         THEN Commit(event, command, 1)
                         ELSE Reject(event, command)

BeginReplay ==
    /\ phase = "arrival"
    /\ attempted = Commands
    /\ phase' = "replay"
    /\ UNCHANGED <<attempted, pending, committed, rejected,
                    rejectionBefore, rejectionTrigger, cost,
                    replayed, replayCost>>

Replay(event) ==
    /\ phase = "replay"
    /\ event \in committed \ replayed
    /\ replayed' = replayed \cup {event}
    /\ replayCost' = replayCost + 1
    /\ UNCHANGED <<attempted, pending, committed, rejected,
                    rejectionBefore, rejectionTrigger, cost, phase>>

Finish ==
    /\ phase = "replay"
    /\ replayed = committed
    /\ phase' = "done"
    /\ UNCHANGED <<attempted, pending, committed, rejected,
                    rejectionBefore, rejectionTrigger, cost,
                    replayed, replayCost>>

Next ==
    \/ \E command \in Commands : Arrival(command)
    \/ BeginReplay
    \/ \E event \in Events : Replay(event)
    \/ Finish

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ \A command \in Commands : WF_vars(Arrival(command))
    /\ WF_vars(BeginReplay)
    /\ \A event \in Events : WF_vars(Replay(event))
    /\ WF_vars(Finish)

TypeOK ==
    /\ attempted \subseteq Commands
    /\ pending \subseteq Commands
    /\ committed \subseteq Events
    /\ rejected \subseteq Events
    /\ rejectionBefore \in [Events -> SUBSET Commands]
    /\ rejectionTrigger \in [Events -> Commands \cup {NoCommand}]
    /\ cost \in Nat
    /\ replayed \subseteq Events
    /\ replayCost \in Nat
    /\ phase \in {"arrival", "replay", "done"}

CostEqualsCommittedComms ==
    ~ChargeOnIntroduction => cost = Cardinality(committed)

ExactCommCost ==
    cost = Cardinality(committed)

UnmatchedIntroductionsAreFree ==
    ~ChargeOnIntroduction =>
        cost = Cardinality(committed)

JoinArityDoesNotMultiplyCost ==
    ~ChargeOnIntroduction =>
        cost = Cardinality(committed \ {join})
            + IF join \in committed THEN 1 ELSE 0

RejectedCommIsAtomic ==
    \A event \in rejected :
        /\ rejectionBefore[event] \subseteq pending
        /\ rejectionTrigger[event] \notin pending
        /\ event \notin committed

ReplayPrefixMatchesCommittedComms ==
    /\ replayed \subseteq committed
    /\ replayCost = Cardinality(replayed)

ReplayMatchesPlayAtCompletion ==
    phase = "done" => replayCost = cost

BudgetNeverOverspent ==
    ~ChargeOnIntroduction => cost <= Budget

AllFundedCommsCommit ==
    Budget >= Cardinality(Events) /\ phase # "arrival" => committed = Events

TerminalRSpaceIsScheduleIndependent ==
    Budget >= Cardinality(Events) /\ phase = "done" => pending = UnmatchedCommands

EventuallyComplete == <>(phase = "done")

CommandsDef == {sA, rA, sB, sC, jBC, u}
EventsDef == {binary, join}
RequirementsDef ==
    [event \in EventsDef |->
        IF event = binary THEN {sA, rA} ELSE {sB, sC, jBC}]

=============================================================================
