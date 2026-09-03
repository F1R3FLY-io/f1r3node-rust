---------------- MODULE SingleParticipantFastPath ----------------
EXTENDS FiniteSets, Naturals

CONSTANT
    \* @type: Bool;
    RequireSingletonDirect

Left == "left"
Right == "right"
None == "none"
Participants == {Left, Right}
Owners == Participants \union {None}

Value(participant) == IF participant = Left THEN 1 ELSE 2

VARIABLES
    \* @type: Set(Str);
    live,
    \* @type: Str;
    directOwner,
    \* @type: Set(Str);
    committed,
    \* @type: Set(Str);
    cancelled,
    \* @type: Int;
    result

vars == <<live, directOwner, committed, cancelled, result>>

Init ==
    /\ live = Participants
    /\ directOwner = None
    /\ committed = {}
    /\ cancelled = {}
    /\ result = 0

CompleteWithoutOperation(participant) ==
    /\ participant \in live
    /\ directOwner = None
    /\ live' = live \ {participant}
    /\ cancelled' = cancelled \union {participant}
    /\ UNCHANGED <<directOwner, committed, result>>

StartDirect(participant) ==
    /\ participant \in live
    /\ directOwner = None
    /\ IF RequireSingletonDirect THEN Cardinality(live) = 1 ELSE TRUE
    /\ directOwner' = participant
    /\ UNCHANGED <<live, committed, cancelled, result>>

FinishDirect(participant) ==
    /\ directOwner = participant
    /\ live' = live \ {participant}
    /\ directOwner' = None
    /\ committed' = committed \union {participant}
    /\ result' = result + Value(participant)
    /\ UNCHANGED cancelled

CancelDirect(participant) ==
    /\ directOwner = participant
    /\ live' = live \ {participant}
    /\ directOwner' = None
    /\ cancelled' = cancelled \union {participant}
    /\ UNCHANGED <<committed, result>>

ScheduledCommit(participant) ==
    /\ participant \in live
    /\ directOwner = None
    /\ Cardinality(live) = 1
    /\ live' = live \ {participant}
    /\ committed' = committed \union {participant}
    /\ result' = result + Value(participant)
    /\ UNCHANGED <<directOwner, cancelled>>

ResolveDirect ==
    \E participant \in Participants :
        FinishDirect(participant) \/ CancelDirect(participant)

Next ==
    (\E participant \in Participants : CompleteWithoutOperation(participant))
    \/ (\E participant \in Participants : StartDirect(participant))
    \/ ResolveDirect
    \/ (\E participant \in Participants : ScheduledCommit(participant))

Spec == Init /\ [][Next]_vars /\ WF_vars(ResolveDirect)

TypeOK ==
    /\ live \subseteq Participants
    /\ directOwner \in Owners
    /\ committed \subseteq Participants
    /\ cancelled \subseteq Participants
    /\ result \in 0..3

Inv_OperationConservation ==
    /\ committed \intersect cancelled = {}
    /\ live \intersect committed = {}
    /\ live \intersect cancelled = {}
    /\ live \union committed \union cancelled = Participants

Inv_DirectOwnerRequiresSingleton ==
    directOwner /= None => Cardinality(live) = 1

Inv_DirectOwnerIsLive ==
    directOwner /= None => directOwner \in live

Inv_ResultMatchesCommittedOperations ==
    result =
        (IF Left \in committed THEN Value(Left) ELSE 0) +
        (IF Right \in committed THEN Value(Right) ELSE 0)

DirectOwnershipEventuallyResolves ==
    directOwner /= None ~> directOwner = None

=============================================================================
