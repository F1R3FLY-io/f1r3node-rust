------------------------ MODULE ReplayChargeScheduleInvariance ------------------------
(***************************************************************************)
(* D-C5a (epic 8946, Phase D; DR-100): the replay charge of one program    *)
(* is the same in every schedule of its participants.                      *)
(*                                                                         *)
(* Three participants run concurrently. p1 and p2 do work that is not an   *)
(* operation; p3 runs one operation. An operation of a lone participant    *)
(* runs directly; otherwise it is submitted and runs when the driver takes *)
(* the frontier, which happens when every live participant waits. Both    *)
(* paths charge the operation alike (DR-100, R0).                          *)
(* TotalIndependentOfSchedule: every completed run has the same total.     *)
(*                                                                         *)
(* Rust correspondence:                                                    *)
(*   rholang/src/rust/interpreter/deterministic_reduction.rs               *)
(*     (submit_produce, submit_consume, prepare, drive).                   *)
(* Rocq correspondence: theories/ReplayChargeScheduleInvariance.v          *)
(*   (direct_and_driver_paths_charge_alike).                               *)
(*                                                                         *)
(* Mutation selects the negative control, which must violate               *)
(* TotalIndependentOfSchedule in its *Unsafe.cfg:                          *)
(*   "driverRead"  the driver path also charges a joins read               *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS OperationCharge, PrepareCharge, WorkCharge, Mutation

ASSUME /\ OperationCharge \in Nat
       /\ PrepareCharge \in Nat \ {0}
       /\ WorkCharge \in Nat
       /\ Mutation \in {"none", "driverRead"}

Participants == {"p1", "p2", "p3"}

Program(p) == CASE p = "p1" -> <<"work">>
                [] p = "p2" -> <<"work", "work">>
                [] p = "p3" -> <<"operation">>

VARIABLES pc, waiting, total

vars == <<pc, waiting, total>>

Finished(p) == pc[p] > Len(Program(p))
Alive == {p \in Participants : ~Finished(p)}
Step(p) == Program(p)[pc[p]]
Ready(p) == p \in Alive /\ p \notin waiting

Work(p) ==
  /\ Ready(p)
  /\ Step(p) = "work"
  /\ total' = total + WorkCharge
  /\ pc' = [pc EXCEPT ![p] = @ + 1]
  /\ UNCHANGED waiting

Direct(p) ==
  /\ Ready(p)
  /\ Step(p) = "operation"
  /\ Alive = {p}
  /\ waiting = {}
  /\ total' = total + OperationCharge
  /\ pc' = [pc EXCEPT ![p] = @ + 1]
  /\ UNCHANGED waiting

Submit(p) ==
  /\ Ready(p)
  /\ Step(p) = "operation"
  /\ Alive # {p}
  /\ waiting' = waiting \cup {p}
  /\ UNCHANGED <<pc, total>>

DriveCharge == OperationCharge + IF Mutation = "driverRead" THEN PrepareCharge ELSE 0

Drive ==
  /\ waiting # {}
  /\ waiting = Alive
  /\ total' = total + Cardinality(waiting) * DriveCharge
  /\ pc' = [p \in Participants |-> IF p \in waiting THEN pc[p] + 1 ELSE pc[p]]
  /\ waiting' = {}

Init ==
  /\ pc = [p \in Participants |-> 1]
  /\ waiting = {}
  /\ total = 0

Next == (\E p \in Participants : Work(p) \/ Direct(p) \/ Submit(p)) \/ Drive

Spec == Init /\ [][Next]_vars

Done == \A p \in Participants : Finished(p)

ExpectedTotal == 3 * WorkCharge + OperationCharge

TypeOK ==
  /\ pc \in [Participants -> 1..3]
  /\ waiting \subseteq Participants
  /\ total \in Nat

TotalIndependentOfSchedule == Done => total = ExpectedTotal
=============================================================================
