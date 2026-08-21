----------------------- MODULE MCCapacityBoundedTrace ------------------------
EXTENDS CapacityBoundedTrace, TLC

CONSTANTS e0, e1, e2, e3

MC_Events == {e0, e1, e2, e3}
MC_Rank == [event \in MC_Events |-> CASE event = e0 -> 0
                                        [] event = e1 -> 1
                                        [] event = e2 -> 2
                                        [] OTHER -> 3]

=============================================================================
