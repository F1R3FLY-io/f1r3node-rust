-------------------- MODULE MCNativeReplayAuthority --------------------
EXTENDS NativeReplayAuthority
CONSTANTS w1, w2, c1, c2, t1, d1, p1, p2

MCWorkers == {w1, w2}
MCEvents == {c1, c2, t1, d1}
MCPurses == {p1, p2}
MCDemand == [e \in MCEvents |-> [p \in MCPurses |->
  IF e = c1 THEN (IF p = p1 THEN 1 ELSE 0)
  ELSE IF e = c2 THEN 1
  ELSE IF e = t1 THEN (IF p = p1 THEN 2 ELSE 1)
  ELSE 3]]
MCCapacity == [p \in MCPurses |-> 3]
MCGranted == {c1, c2, t1}
MCTransfers == {t1}
=============================================================================
