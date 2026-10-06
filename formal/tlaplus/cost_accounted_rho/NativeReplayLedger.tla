-------------------- MODULE NativeReplayLedger --------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Workers, Slots, MaxSerial, CursorOnly, BusyBoundary, WrongOutcome
VARIABLES status, tickets, undo, checkpoint, epoch, nextSerial,
          boundary, expected, exactOutcome, exactRestore
vars == <<status, tickets, undo, checkpoint, epoch, nextSerial,
          boundary, expected, exactOutcome, exactRestore>>
Outcomes == {"Stored", "Matched", "DeniedIntroduction", "DeniedComm"}
EmptyTicket == [slot |-> 0, serial |-> 0, boundary |-> 0]
PrefixSlots(log, cursor) == {log[i].slot : i \in 1..cursor}
Active == {w \in Workers : tickets[w].serial # 0}
LastSerial(log, cursor) == IF cursor = 0 THEN 0 ELSE log[cursor].serial
ValidToken == /\ checkpoint.epoch = epoch
              /\ checkpoint.cursor <= Len(undo)
              /\ (CursorOnly \/ checkpoint.serial = LastSerial(undo, checkpoint.cursor))

Init ==
  /\ status = [s \in Slots |-> "Available"]
  /\ tickets = [w \in Workers |-> EmptyTicket]
  /\ undo = <<>>
  /\ checkpoint = [epoch |-> 0, cursor |-> 0, serial |-> 0, snapshot |-> {}]
  /\ epoch = 1
  /\ nextSerial = 1
  /\ boundary = 0
  /\ expected \in [Slots -> Outcomes]
  /\ exactOutcome = TRUE
  /\ exactRestore = TRUE

Reserve(w, s) ==
  /\ w \notin Active
  /\ status[s] = "Available"
  /\ nextSerial <= MaxSerial
  /\ tickets' = [tickets EXCEPT ![w] = [slot |-> s, serial |-> nextSerial, boundary |-> boundary]]
  /\ status' = [status EXCEPT ![s] = "Reserved"]
  /\ nextSerial' = nextSerial + 1
  /\ UNCHANGED <<undo, checkpoint, epoch, boundary, expected, exactOutcome, exactRestore>>

Drop(w) ==
  /\ w \in Active
  /\ status' = [status EXCEPT ![tickets[w].slot] = "Available"]
  /\ tickets' = [tickets EXCEPT ![w] = EmptyTicket]
  /\ UNCHANGED <<undo, checkpoint, epoch, nextSerial, boundary, expected, exactOutcome, exactRestore>>

Publish(w, observed) ==
  /\ w \in Active
  /\ WrongOutcome \/ observed = expected[tickets[w].slot]
  /\ undo' = Append(undo, [slot |-> tickets[w].slot, serial |-> tickets[w].serial])
  /\ status' = [status EXCEPT ![tickets[w].slot] = "Completed"]
  /\ tickets' = [tickets EXCEPT ![w] = EmptyTicket]
  /\ exactOutcome' = (observed = expected[tickets[w].slot])
  /\ UNCHANGED <<checkpoint, epoch, nextSerial, boundary, expected, exactRestore>>

Capture ==
  /\ Active = {}
  /\ checkpoint' = [epoch |-> epoch, cursor |-> Len(undo),
                    serial |-> LastSerial(undo, Len(undo)), snapshot |-> PrefixSlots(undo, Len(undo))]
  /\ UNCHANGED <<status, tickets, undo, epoch, nextSerial, boundary, expected, exactOutcome, exactRestore>>

Restore(root) ==
  /\ Active = {} \/ BusyBoundary
  /\ boundary < 2
  /\ root \/ ValidToken
  /\ LET cursor == IF root THEN 0 ELSE checkpoint.cursor
         retained == PrefixSlots(undo, cursor)
     IN /\ status' = [s \in Slots |-> IF s \in retained THEN "Completed"
                       ELSE IF s \in PrefixSlots(undo, Len(undo)) THEN "Available" ELSE status[s]]
        /\ undo' = SubSeq(undo, 1, cursor)
        /\ exactRestore' = (root \/ retained = checkpoint.snapshot)
  /\ boundary' = boundary + 1
  /\ UNCHANGED <<tickets, checkpoint, epoch, nextSerial, expected, exactOutcome>>

Reset ==
  /\ Active = {}
  /\ epoch = 1
  /\ epoch' = 2
  /\ status' = [s \in Slots |-> "Available"]
  /\ undo' = <<>>
  /\ nextSerial' = 1
  /\ boundary' = 0
  /\ UNCHANGED <<tickets, checkpoint, expected, exactOutcome, exactRestore>>

Next == (\E w \in Workers, s \in Slots : Reserve(w, s))
     \/ (\E w \in Workers : Drop(w))
     \/ (\E w \in Workers, outcome \in Outcomes \cup {"HostError"} : Publish(w, outcome))
     \/ Capture \/ Restore(TRUE) \/ Restore(FALSE) \/ Reset
Spec == Init /\ [][Next]_vars

TypeOK == /\ status \in [Slots -> {"Available", "Reserved", "Completed"}]
          /\ epoch \in 1..2
          /\ nextSerial \in 1..(MaxSerial + 1)
          /\ boundary \in 0..2
          /\ expected \in [Slots -> Outcomes]
          /\ exactOutcome \in BOOLEAN
          /\ exactRestore \in BOOLEAN
ExclusiveReservation == \A w, v \in Active : w = v \/ tickets[w].slot # tickets[v].slot
ReservedOwnership == {s \in Slots : status[s] = "Reserved"} = {tickets[w].slot : w \in Active}
UndoOwnership == {s \in Slots : status[s] = "Completed"} = PrefixSlots(undo, Len(undo))
UniqueUndo == Cardinality(PrefixSlots(undo, Len(undo))) = Len(undo)
UniqueSerials == Cardinality({undo[i].serial : i \in 1..Len(undo)}) = Len(undo)
BoundedCapacity == Cardinality(Active) + Len(undo) <= Cardinality(Slots)
LiveSnapshot == \A w \in Active : tickets[w].boundary = boundary
ExactOutcome == exactOutcome
ExactRestore == exactRestore
IndependentProgress == \A w \in Workers, s \in Slots :
  (w \notin Active /\ status[s] = "Available" /\ nextSerial <= MaxSerial) => ENABLED Reserve(w, s)
=============================================================================
