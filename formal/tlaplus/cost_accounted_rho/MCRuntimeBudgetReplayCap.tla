----------------------- MODULE MCRuntimeBudgetReplayCap -----------------------
(****************************************************************************)
(* Model-checking instance for RuntimeBudgetReplay — BOUNDED-K CAP arm.      *)
(*                                                                          *)
(* Companion to MCRuntimeBudgetReplay (OOP arm) and                         *)
(* MCRuntimeBudgetReplayNonOop (complete-commit arm). Here the trace cap     *)
(* MaxTraceEvents (2) is SMALLER than the number of intrinsically-valid      *)
(* events (3), and InitialBudget (12) is large, so the BOUNDED-K window      *)
(* (K = min(MaxTraceEvents, InitialBudget+1) = 2) is the binding constraint  *)
(* on the reconciliation rather than the budget. This is the                *)
(* MAX_COST_TRACE_EVENTS backstop path: `reconcile()` truncates the          *)
(* canonical attempt log to the lowest K events before walking.             *)
(*                                                                          *)
(* It exercises: CapTruncates = TRUE; MergeReadsBoundedKWindow under         *)
(* truncation; and the GUARDED threshold clauses of                         *)
(* ConsumedAndVerdictScheduleIndependent / TotalCostMatchesClampedSum        *)
(* (which correctly do NOT assert the Σ-vs-InitialBudget law when the cap    *)
(* bites, while the unconditional clamp law still holds). The reconciled     *)
(* answer remains a pure function of the constants — schedule-independent.   *)
(*                                                                          *)
(* Same event attributes as the other two instances; ValidEventSet =        *)
(* {e1, e2, e3} by rank, so KWindow = <<e1, e2>> and the canonical bounded   *)
(* commit set is {e1, e2} with consumed = 4, no OOP within the window.       *)
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
MC_MaxTraceEvents == 2
MC_MaxSourcePathComponents == 2
MC_MaxPrimitiveDescriptor == 9
MC_NoOop == no_oop

=============================================================================
