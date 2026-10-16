----------------------- MODULE NativeDigestIndexCharge -----------------------
(***************************************************************************)
(* D-S1 (epic 8946, Phase D item D-C2b; DR-96): the charges of the native   *)
(* session store do not depend on the thread schedule.                     *)
(*                                                                         *)
(* Concurrent operations reach the store in an order that the schedule     *)
(* decides. Each key runs its own program in program order: a read, a      *)
(* write and a read. A read of an absent key is a cold fill: it reads the   *)
(* history and inserts the key. A read of a present key is a warm read. A  *)
(* write replaces the value of a present key.                              *)
(*                                                                         *)
(* The digest index charges an operation by the operation and by the       *)
(* presence of its own key only: a search, an insert on a cold fill or a   *)
(* replace on a write, and the work on the key's own value.                *)
(* TotalIndependentOfSchedule: every complete schedule has the total of    *)
(* the canonical schedule, which runs the keys one after another.          *)
(*                                                                         *)
(* Rust correspondence: rspace++/src/rspace/hot_store/native_index.rs      *)
(*   (search_charge, insert_charge, replace_charge).                       *)
(* Rocq correspondence: theories/NativeDigestIndex.v                       *)
(*   (per_key_schedule_total_invariant,                                    *)
(*    population_charge_schedule_dependent).                               *)
(*                                                                         *)
(* Mutation selects the negative control, which must violate the named    *)
(* invariant in its *Unsafe.cfg:                                           *)
(*   "population"  every charge adds the number of present keys, as the    *)
(*                 legacy shard charges did -> TotalIndependentOfSchedule  *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS Keys, SearchCharge, InsertCharge, ReplaceCharge, ValueCharge, Mutation

ASSUME /\ Keys # {} /\ IsFiniteSet(Keys)
       /\ SearchCharge \in Nat /\ InsertCharge \in Nat
       /\ ReplaceCharge \in Nat /\ ValueCharge \in Nat
       /\ Mutation \in {"none", "population"}

Program == <<"read", "write", "read">>

VARIABLES pc, present, total
vars == <<pc, present, total>>

Population == Cardinality({k \in Keys : present[k]})

OpCharge(op, warm, population) ==
  LET base == IF op = "read"
                THEN IF warm THEN SearchCharge + ValueCharge
                     ELSE SearchCharge + InsertCharge + ValueCharge
                ELSE SearchCharge + ReplaceCharge + ValueCharge
  IN IF Mutation = "population" THEN base + population ELSE base

Init == /\ pc = [k \in Keys |-> 1]
        /\ present = [k \in Keys |-> FALSE]
        /\ total = 0

Step(k) == /\ pc[k] <= Len(Program)
           /\ total' = total + OpCharge(Program[pc[k]], present[k], Population)
           /\ present' = [present EXCEPT ![k] = TRUE]
           /\ pc' = [pc EXCEPT ![k] = @ + 1]

Next == \E k \in Keys : Step(k)

Spec == Init /\ [][Next]_vars

Done == \A k \in Keys : pc[k] > Len(Program)

(* The canonical schedule runs the keys one after another. Key i finds    *)
(* i - 1 keys present before its cold fill, and i keys afterwards.         *)
KeyRun(before) == OpCharge("read", FALSE, before)
                  + OpCharge("write", TRUE, before + 1)
                  + OpCharge("read", TRUE, before + 1)

RECURSIVE CanonicalFrom(_)
CanonicalFrom(i) == IF i > Cardinality(Keys) THEN 0
                    ELSE KeyRun(i - 1) + CanonicalFrom(i + 1)

ExpectedTotal == CanonicalFrom(1)

TypeOK == /\ pc \in [Keys -> 1..(Len(Program) + 1)]
          /\ present \in [Keys -> BOOLEAN]
          /\ total \in Nat

TotalIndependentOfSchedule == Done => total = ExpectedTotal
===============================================================================
