----------------- MODULE MCStateBoundFrontierExpansion -----------------
EXTENDS StateBoundFrontierExpansion

BackingDef == [index \in 1..3 |-> CASE index = 1 -> 2
                                      [] index = 2 -> 2
                                      [] OTHER -> 1]

=============================================================================
