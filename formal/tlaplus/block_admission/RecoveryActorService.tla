-------------------------- MODULE RecoveryActorService --------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
    \* @type: Set(Str);
    Actors,
    \* @type: Int;
    QueueCap,
    \* @type: Bool;
    FixedPriority,
    \* @type: Bool;
    StopWhenClosed

Lanes == 0..2
Inputs == {0, 1}

VARIABLES
    \* @type: Str -> (Int -> Int);
    queued,
    \* @type: Str -> Set(Int);
    inputOpen,
    \* @type: Str -> Set(Int);
    observedOpen,
    \* @type: Str -> Bool;
    due,
    \* @type: Str -> Int;
    cursor,
    \* @type: Str -> (Int -> Int);
    debt,
    \* @type: Str -> Bool;
    busy,
    \* @type: Str -> Bool;
    alive

vars == <<queued, inputOpen, observedOpen, due, cursor, debt, busy, alive>>

NextLane(lane) == (lane + 1) % 3
Distance(start, lane) == (lane + 3 - start) % 3

Ready(actor, lane) ==
    IF lane = 2
    THEN due[actor]
    ELSE lane \in observedOpen[actor]
         /\ (queued[actor][lane] > 0 \/ lane \notin inputOpen[actor])

ReadyLanes(actor) == {lane \in Lanes : Ready(actor, lane)}

FirstLane(actor) == IF FixedPriority THEN 0 ELSE cursor[actor]

Chosen(actor) ==
    CHOOSE lane \in ReadyLanes(actor) :
        \A other \in ReadyLanes(actor) :
            Distance(FirstLane(actor), lane) <= Distance(FirstLane(actor), other)

Init ==
    /\ queued = [actor \in Actors |-> [lane \in Inputs |-> 0]]
    /\ inputOpen = [actor \in Actors |-> Inputs]
    /\ observedOpen = [actor \in Actors |-> Inputs]
    /\ due = [actor \in Actors |-> FALSE]
    /\ cursor = [actor \in Actors |-> 0]
    /\ debt = [actor \in Actors |-> [lane \in Lanes |-> 0]]
    /\ busy = [actor \in Actors |-> FALSE]
    /\ alive = [actor \in Actors |-> TRUE]

Enqueue(actor, lane) ==
    /\ alive[actor]
    /\ lane \in inputOpen[actor]
    /\ queued[actor][lane] < QueueCap
    /\ queued' = [queued EXCEPT ![actor][lane] = @ + 1]
    /\ UNCHANGED <<inputOpen, observedOpen, due, cursor, debt, busy, alive>>

CloseInput(actor, lane) ==
    /\ lane \in inputOpen[actor]
    /\ inputOpen' = [inputOpen EXCEPT ![actor] = @ \ {lane}]
    /\ UNCHANGED <<queued, observedOpen, due, cursor, debt, busy, alive>>

Tick(actor) ==
    /\ alive[actor]
    /\ ~due[actor]
    /\ due' = [due EXCEPT ![actor] = TRUE]
    /\ UNCHANGED <<queued, inputOpen, observedOpen, cursor, debt, busy, alive>>

Serve(actor) ==
    /\ alive[actor]
    /\ ~busy[actor]
    /\ ~(StopWhenClosed /\ observedOpen[actor] = {})
    /\ ReadyLanes(actor) # {}
    /\ LET selected == Chosen(actor)
           closes == selected \in Inputs /\ queued[actor][selected] = 0
       IN /\ queued' =
                 IF selected \in Inputs /\ ~closes
                 THEN [queued EXCEPT ![actor][selected] = @ - 1]
                 ELSE queued
          /\ observedOpen' =
                 IF closes
                 THEN [observedOpen EXCEPT ![actor] = @ \ {selected}]
                 ELSE observedOpen
          /\ due' = IF selected = 2 THEN [due EXCEPT ![actor] = FALSE] ELSE due
          /\ cursor' = [cursor EXCEPT ![actor] = NextLane(selected)]
          /\ debt' = [debt EXCEPT ![actor] =
                 [lane \in Lanes |->
                     IF lane = selected \/ ~Ready(actor, lane)
                     THEN 0
                     ELSE IF debt[actor][lane] < 3
                          THEN debt[actor][lane] + 1
                          ELSE 3]]
          /\ busy' = [busy EXCEPT ![actor] = TRUE]
    /\ UNCHANGED <<inputOpen, alive>>

Finish(actor) ==
    /\ alive[actor]
    /\ busy[actor]
    /\ busy' = [busy EXCEPT ![actor] = FALSE]
    /\ UNCHANGED <<queued, inputOpen, observedOpen, due, cursor, debt, alive>>

Stop(actor) ==
    /\ StopWhenClosed
    /\ alive[actor]
    /\ ~busy[actor]
    /\ observedOpen[actor] = {}
    /\ alive' = [alive EXCEPT ![actor] = FALSE]
    /\ UNCHANGED <<queued, inputOpen, observedOpen, due, cursor, debt, busy>>

Next ==
    \/ \E actor \in Actors, lane \in Inputs : Enqueue(actor, lane)
    \/ \E actor \in Actors, lane \in Inputs : CloseInput(actor, lane)
    \/ \E actor \in Actors : Tick(actor)
    \/ \E actor \in Actors : Serve(actor)
    \/ \E actor \in Actors : Finish(actor)
    \/ \E actor \in Actors : Stop(actor)
    \/ UNCHANGED vars

Fairness ==
    /\ \A actor \in Actors : WF_vars(Serve(actor))
    /\ \A actor \in Actors : WF_vars(Finish(actor))
    /\ \A actor \in Actors : WF_vars(Stop(actor))

Spec == Init /\ [][Next]_vars /\ Fairness

TypeOK ==
    /\ queued \in [Actors -> [Inputs -> 0..QueueCap]]
    /\ inputOpen \in [Actors -> SUBSET Inputs]
    /\ observedOpen \in [Actors -> SUBSET Inputs]
    /\ due \in [Actors -> BOOLEAN]
    /\ cursor \in [Actors -> Lanes]
    /\ debt \in [Actors -> [Lanes -> 0..3]]
    /\ busy \in [Actors -> BOOLEAN]
    /\ alive \in [Actors -> BOOLEAN]
    /\ \A actor \in Actors : inputOpen[actor] \subseteq observedOpen[actor]
    /\ \A actor \in Actors, lane \in Inputs :
           lane \notin observedOpen[actor] => queued[actor][lane] = 0
    /\ \A actor \in Actors : ~alive[actor] => ~busy[actor]

Inv_BoundedServiceGap ==
    \A actor \in Actors, lane \in Lanes : debt[actor][lane] <= 2

Inv_ServiceRank ==
    \A actor \in Actors, lane \in Lanes :
        Ready(actor, lane) => debt[actor][lane] + Distance(cursor[actor], lane) <= 2

Live_ClosedInputsTerminate ==
    \A actor \in Actors : (inputOpen[actor] = {}) ~> ~alive[actor]

Inv_InactiveLaneDebt ==
    \A actor \in Actors, lane \in Lanes : ~Ready(actor, lane) => debt[actor][lane] = 0

Safety == TypeOK /\ Inv_BoundedServiceGap /\ Inv_ServiceRank /\ Inv_InactiveLaneDebt

=============================================================================
