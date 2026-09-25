-------------------- MODULE NativeReplayAccounting --------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Workers, Slots, MaxStarts, SkipStage, RepeatStage, DebitRetry, SkipUsageUndo
VARIABLES status, ticket, undo, used, starts, outcome, retry, exactStages
vars == <<status, ticket, undo, used, starts, outcome, retry, exactStages>>
Stages == {"Intro", "Comm"}
Outcomes == {"Stored", "Matched", "DeniedIntro", "DeniedComm"}
Empty == [slot |-> 0, phase |-> "Idle", seen |-> {}, delta |-> 0]
Active == {w \in Workers : ticket[w].phase # "Idle"}
Required(s) == IF outcome[s] \in {"Matched", "DeniedComm"} THEN Stages ELSE {"Intro"}
Granted(s, stage) == IF stage = "Intro" THEN outcome[s] # "DeniedIntro"
                    ELSE outcome[s] = "Matched"
FreshCost(s, stage) == IF ~Granted(s, stage) \/ retry[s][stage] THEN 0
                      ELSE IF stage = "Intro" THEN 2 ELSE 3
Charge(s) == FreshCost(s, "Intro") + FreshCost(s, "Comm")
RECURSIVE Sum(_)
Sum(values) == IF Len(values) = 0 THEN 0 ELSE Head(values) + Sum(Tail(values))
LoggedSlots(log) == {log[i].slot : i \in 1..Len(log)}
LoggedUsage(log) == Sum([i \in 1..Len(log) |-> log[i].delta])
ExpectedUsage(log) == Sum([i \in 1..Len(log) |-> Charge(log[i].slot)])

Init ==
  /\ outcome \in [Slots -> Outcomes]
  /\ retry \in [Slots -> [Stages -> BOOLEAN]]
  /\ \A s \in Slots, stage \in Stages : retry[s][stage] => (stage \in Required(s) /\ Granted(s, stage))
  /\ status = [s \in Slots |-> "Available"]
  /\ ticket = [w \in Workers |-> Empty]
  /\ undo = <<>>
  /\ used = 0
  /\ starts = 0
  /\ exactStages = TRUE

Reserve(w, s) ==
  /\ w \notin Active /\ status[s] = "Available" /\ starts < MaxStarts
  /\ status' = [status EXCEPT ![s] = "Reserved"]
  /\ ticket' = [ticket EXCEPT ![w] = [slot |-> s, phase |-> "Intro", seen |-> {}, delta |-> 0]]
  /\ starts' = starts + 1
  /\ UNCHANGED <<undo, used, outcome, retry, exactStages>>

Observe(w, stage) ==
  /\ w \in Active
  /\ stage = ticket[w].phase \/ (RepeatStage /\ stage \in ticket[w].seen)
  /\ LET s == ticket[w].slot
         amount == IF DebitRetry /\ retry[s][stage] THEN 1 ELSE FreshCost(s, stage)
         phase == IF stage = "Intro" /\ "Comm" \in Required(s) THEN "Comm" ELSE "Complete"
     IN ticket' = [ticket EXCEPT ![w] = [@ EXCEPT !.phase = phase,
          !.seen = @ \cup {stage}, !.delta = @ + amount]]
  /\ exactStages' = (exactStages /\ stage \notin ticket[w].seen /\ stage = ticket[w].phase)
  /\ UNCHANGED <<status, undo, used, starts, outcome, retry>>

Publish(w) ==
  /\ w \in Active
  /\ SkipStage \/ ticket[w].phase = "Complete"
  /\ LET row == [slot |-> ticket[w].slot, delta |-> ticket[w].delta]
     IN /\ undo' = Append(undo, row)
        /\ used' = used + row.delta
        /\ status' = [status EXCEPT ![row.slot] = "Completed"]
        /\ exactStages' = (exactStages /\ ticket[w].seen = Required(row.slot))
  /\ ticket' = [ticket EXCEPT ![w] = Empty]
  /\ UNCHANGED <<starts, outcome, retry>>

Cancel(w) ==
  /\ w \in Active
  /\ status' = [status EXCEPT ![ticket[w].slot] = "Available"]
  /\ ticket' = [ticket EXCEPT ![w] = Empty]
  /\ UNCHANGED <<undo, used, starts, outcome, retry, exactStages>>

Restore(cursor) ==
  /\ Active = {} /\ cursor \in 0..Len(undo)
  /\ undo' = SubSeq(undo, 1, cursor)
  /\ status' = [s \in Slots |-> IF s \in LoggedSlots(undo') THEN "Completed" ELSE "Available"]
  /\ used' = IF SkipUsageUndo THEN used ELSE used - LoggedUsage(SubSeq(undo, cursor + 1, Len(undo)))
  /\ UNCHANGED <<ticket, starts, outcome, retry, exactStages>>

Next == (\E w \in Workers, s \in Slots : Reserve(w, s))
     \/ (\E w \in Workers, stage \in Stages : Observe(w, stage))
     \/ (\E w \in Workers : Publish(w) \/ Cancel(w))
     \/ (\E cursor \in 0..Len(undo) : Restore(cursor))
Spec == Init /\ [][Next]_vars

TypeOK == /\ status \in [Slots -> {"Available", "Reserved", "Completed"}]
          /\ used \in Nat /\ starts \in 0..MaxStarts /\ exactStages \in BOOLEAN
ExclusiveOwnership == \A w, v \in Active : w = v \/ ticket[w].slot # ticket[v].slot
ReservedOwnership == {s \in Slots : status[s] = "Reserved"} = {ticket[w].slot : w \in Active}
CompletedOwnership == {s \in Slots : status[s] = "Completed"} = LoggedSlots(undo)
UniquePublication == Cardinality(LoggedSlots(undo)) = Len(undo)
StageCoverage == exactStages
ExactUsage == used = ExpectedUsage(undo)
UndoUsage == used = LoggedUsage(undo)
IndependentReservation == \A w \in Workers, s \in Slots :
  (w \notin Active /\ status[s] = "Available" /\ starts < MaxStarts) => ENABLED Reserve(w, s)
=============================================================================
