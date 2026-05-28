--------------------- MODULE MCRuntimeBudgetReplayNonOop ----------------------
(****************************************************************************)
(* Model-checking instance for RuntimeBudgetReplay — NON-OOP arm.            *)
(*                                                                          *)
(* Companion to MCRuntimeBudgetReplay (which sizes the budget so the deploy  *)
(* goes OOP). Here InitialBudget is large enough that the sum of all         *)
(* intrinsically-valid weights stays within budget, so the deploy NEVER     *)
(* goes OOP. This exercises the non-OOP arm of                              *)
(* ConsumedAndVerdictScheduleIndependent / TotalCostMatchesClampedSum and   *)
(* NonOopCommittedMultisetComplete: under every schedule the reconciled     *)
(* committed multiset is the complete valid-event set and total_cost = Σ.   *)
(*                                                                          *)
(* Same event attributes as MCRuntimeBudgetReplay (so e0 = zero-weight,     *)
(* e4 = over-long source path, e5 = over-large primitive descriptor are all *)
(* intrinsically invalid; ValidEventSet = {e1, e2, e3}, Σ weights = 9).     *)
(* InitialBudget = 12 > 9, MaxTraceEvents = 4 >= 3 valid events, so neither  *)
(* the budget nor the trace cap bites — pure complete-commit path.          *)
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
MC_SourcePath == [e \in MC_Events |-> CASE e = e0 -> <<0>>
                                         [] e = e1 -> <<1>>
                                         [] e = e2 -> <<1>>
                                         [] e = e3 -> <<2>>
                                         [] e = e4 -> <<0, 1, 2>>
                                         [] e = e5 -> <<0>>]
MC_RedexId == [e \in MC_Events |-> CASE e = e0 -> 0
                                      [] e = e1 -> 1
                                      [] e = e2 -> 1
                                      [] e = e3 -> 2
                                      [] e = e4 -> 3
                                      [] e = e5 -> 4]
MC_LocalIndex == [e \in MC_Events |-> 0]
MC_KindId == [e \in MC_Events |-> CASE e = e3 -> 1
                                     [] e = e5 -> 1
                                     [] OTHER -> 0]
MC_PrimitiveDescriptor == [e \in MC_Events |-> CASE e = e3 -> 9
                                                  [] e = e5 -> 10
                                                  [] OTHER -> 0]
MC_Weight == [e \in MC_Events |-> CASE e = e0 -> 0
                                     [] e = e1 -> 2
                                     [] e = e2 -> 2
                                     [] e = e3 -> 5
                                     [] e = e4 -> 1
                                     [] e = e5 -> 1]
MC_Rank == [e \in MC_Events |-> CASE e = e0 -> 0
                                   [] e = e1 -> 1
                                   [] e = e2 -> 2
                                   [] e = e3 -> 3
                                   [] e = e4 -> 4
                                   [] e = e5 -> 5]
MC_InitialBudget == 12
MC_MaxTraceEvents == 4
MC_MaxSourcePathComponents == 2
MC_MaxPrimitiveDescriptor == 9
MC_NoOop == no_oop

=============================================================================
