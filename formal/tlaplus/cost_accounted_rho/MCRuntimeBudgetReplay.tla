------------------------ MODULE MCRuntimeBudgetReplay -------------------------
(****************************************************************************)
(* Model-checking instance for RuntimeBudgetReplay.                          *)
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
MC_LocalIndex == [e \in MC_Events |-> CASE e = e0 -> 0
                                         [] e = e1 -> 0
                                         [] e = e2 -> 0
                                         [] e = e3 -> 0
                                         [] e = e4 -> 0
                                         [] e = e5 -> 0]
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
MC_InitialBudget == 6
MC_MaxTraceEvents == 3
MC_MaxSourcePathComponents == 2
MC_MaxPrimitiveDescriptor == 9
MC_NoOop == no_oop

=============================================================================
