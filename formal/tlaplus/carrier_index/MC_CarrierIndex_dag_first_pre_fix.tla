------------------ MODULE MC_CarrierIndex_dag_first_pre_fix ------------------
EXTENDS CarrierIndex

CONSTANTS B0, B1, B2, S1, S2

MCHeight ==
  [block \in Blocks |->
    CASE block = B0 -> 0
      [] block = B1 -> 1
      [] OTHER -> 2]

MCBlockSigs ==
  [block \in Blocks |->
    CASE block = B0 -> {S1}
      [] block = B1 -> {S2}
      [] OTHER -> {S1, S2}]

=============================================================================
