----------------------- MODULE MCRuntimeBudgetReplayCap -----------------------
(****************************************************************************)
(* Model-checking instance for RuntimeBudgetReplay — CAPACITY WINDOW arm.    *)
(*                                                                          *)
(* Companion to MCRuntimeBudgetReplay (OOP arm) and                         *)
(* MCRuntimeBudgetReplayNonOop (complete-commit arm). Here the trace cap     *)
(* InitialBudget is 2 and all six events are unit-cost valid COMMs. The      *)
(* capacity-derived window K = InitialBudget+1 = 3 is smaller than the       *)
(* attempted workload and preserves exactly two commits plus the OOP witness.*)
(*                                                                          *)
(* It exercises CapTruncates = TRUE while the unguarded exact clamped-cost   *)
(* and OOP laws continue to hold.                                            *)
(*                                                                          *)
(* KWindow is <<e0,e1,e2>>; e0 and e1 commit and e2 is the OOP boundary.     *)
(****************************************************************************)

EXTENDS RuntimeBudgetReplay, TLC

CONSTANTS e0, e1, e2, e3, e4, e5, no_oop

MC_Events == {e0, e1, e2, e3, e4, e5}
MC_DeployId == [e \in MC_Events |-> CASE e = e0 -> 0
                                       [] e = e1 -> 1
                                       [] e = e2 -> 1
                                       [] e = e3 -> 2
                                       [] e = e4 -> 3
                                       [] e = e5 -> 4]
MC_SourcePath == [e \in MC_Events |-> <<0>>]
MC_RedexId == [e \in MC_Events |-> CASE e = e0 -> 0
                                      [] e = e1 -> 1
                                      [] e = e2 -> 1
                                      [] e = e3 -> 2
                                      [] e = e4 -> 3
                                      [] e = e5 -> 4]
MC_LocalIndex == [e \in MC_Events |-> 0]
MC_KindId == [e \in MC_Events |-> 0]
MC_PrimitiveDescriptor == [e \in MC_Events |-> 0]
MC_Weight == [e \in MC_Events |-> 1]
MC_Rank == [e \in MC_Events |-> CASE e = e0 -> 0
                                   [] e = e1 -> 1
                                   [] e = e2 -> 2
                                   [] e = e3 -> 3
                                   [] e = e4 -> 4
                                   [] e = e5 -> 5]
MC_InitialBudget == 2
MC_MaxSourcePathComponents == 2
MC_MaxPrimitiveDescriptor == 9
MC_NoOop == no_oop

=============================================================================
