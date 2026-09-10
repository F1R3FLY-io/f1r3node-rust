------------------------ MODULE PendingPolicyRowGuard ------------------------
EXTENDS Naturals
CONSTANT Unsafe
Rows == {"AB", "BA", "AAB", "AC", "missing"}
Decode(r) == CASE r \in {"AB", "BA", "AAB"} -> {"A", "B"}
              [] r = "AC" -> {"A", "C"}
              [] OTHER -> {}
Encodings(r) == IF Decode(r) = {"A", "B"} THEN {"AB", "BA"} ELSE {r}
VARIABLES row, snapshots, stages, rewrites, decisions
vars == <<row, snapshots, stages, rewrites, decisions>>
Init == /\ row \in Rows \ {"missing"}
        /\ snapshots = [p \in 1..2 |-> "missing"]
        /\ stages = [p \in 1..2 |-> "idle"]
        /\ rewrites = 0 /\ decisions = [p \in 1..2 |-> TRUE]
Capture(p) == /\ stages[p] = "idle"
              /\ snapshots' = [snapshots EXCEPT ![p] = row]
              /\ stages' = [stages EXCEPT ![p] = "captured"]
              /\ UNCHANGED <<row, rewrites, decisions>>
Rewrite(r) == /\ rewrites < 2 /\ row' = r /\ rewrites' = rewrites + 1
              /\ UNCHANGED <<snapshots, stages, decisions>>
Check(p, encoded) ==
    LET wanted == snapshots[p] # "missing" /\ row = snapshots[p]
        candidate == IF Unsafe THEN encoded ELSE snapshots[p]
        accepted == snapshots[p] # "missing" /\ row = candidate
    IN /\ stages[p] = "captured" /\ encoded \in Encodings(snapshots[p])
       /\ decisions' = [decisions EXCEPT ![p] = (accepted = wanted)]
       /\ stages' = [stages EXCEPT ![p] = "done"]
       /\ UNCHANGED <<row, snapshots, rewrites>>
Next == (\E p \in 1..2 : Capture(p) \/ (\E encoded \in Rows : Check(p, encoded)))
        \/ (\E r \in Rows : Rewrite(r))
Inv_ExactRowGuard == \A p \in 1..2 : decisions[p]
Spec == Init /\ [][Next]_vars
=============================================================================
