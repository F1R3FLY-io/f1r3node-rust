------------------------ MODULE ColdStoreCompression ------------------------
EXTENDS Integers, FiniteSets, TLC

CONSTANTS CheckIntegrity, DecodeExport, AtomicDelete, CheckBudget, ReleaseAccounting
VARIABLES row, snapshot, pending, held, used, readSafe, exportSafe, deleteSafe

Workers == {1, 2, 3}
Values == {0, 1}
Formats == {"raw", "lz4"}
Rows == [value : Values, format : Formats, valid : BOOLEAN, present : {TRUE}]
Absent == [value |-> 0, format |-> "raw", valid |-> FALSE, present |-> FALSE]
MaxBytes == 4
MaxActive == 2
vars == <<row, snapshot, pending, held, used, readSafe, exportSafe, deleteSafe>>

RECURSIVE SumHeld(_)
SumHeld(workers) == IF workers = {} THEN 0
                   ELSE LET worker == CHOOSE w \in workers : TRUE
                        IN held[worker] + SumHeld(workers \ {worker})
Active == Cardinality({w \in Workers : held[w] > 0})
Matches(value) == IF value = Absent THEN FALSE ELSE value.valid /\ value.value = 0

Init == /\ row = Absent
        /\ snapshot = Absent
        /\ pending = FALSE
        /\ held = [w \in Workers |-> 0]
        /\ used = 0
        /\ readSafe = TRUE
        /\ exportSafe = TRUE
        /\ deleteSafe = TRUE

Write == /\ \E value \in Values, format \in Formats, valid \in BOOLEAN :
                row' = [value |-> value, format |-> format, valid |-> valid, present |-> TRUE]
         /\ UNCHANGED <<snapshot, pending, held, used, readSafe, exportSafe, deleteSafe>>

Read == /\ row # Absent
        /\ readSafe' = (row.valid \/ CheckIntegrity)
        /\ exportSafe' = (~row.valid \/ row.format = "raw" \/ DecodeExport)
        /\ UNCHANGED <<row, snapshot, pending, held, used, deleteSafe>>

BeginDelete == /\ ~pending
               /\ Matches(row)
               /\ snapshot' = row
               /\ pending' = TRUE
               /\ UNCHANGED <<row, held, used, readSafe, exportSafe, deleteSafe>>

FinishDelete ==
    /\ pending
    /\ IF ~AtomicDelete \/ row = snapshot
          THEN /\ row' = Absent
               /\ deleteSafe' = (row = Absent \/ Matches(row))
          ELSE /\ UNCHANGED <<row, deleteSafe>>
    /\ pending' = FALSE
    /\ UNCHANGED <<snapshot, held, used, readSafe, exportSafe>>

Acquire == /\ \E worker \in Workers, bytes \in 1..3 :
                /\ held[worker] = 0
                /\ (~CheckBudget \/ (Active < MaxActive /\ used + bytes <= MaxBytes))
                /\ held' = [held EXCEPT ![worker] = bytes]
                /\ used' = used + bytes
           /\ UNCHANGED <<row, snapshot, pending, readSafe, exportSafe, deleteSafe>>

Release == /\ \E worker \in Workers :
                /\ held[worker] > 0
                /\ used' = IF ReleaseAccounting THEN used - held[worker] ELSE used
                /\ held' = [held EXCEPT ![worker] = 0]
           /\ UNCHANGED <<row, snapshot, pending, readSafe, exportSafe, deleteSafe>>

Next == Write \/ Read \/ BeginDelete \/ FinishDelete \/ Acquire \/ Release
Spec == Init /\ [][Next]_vars

TypeOK == /\ row \in Rows \cup {Absent}
          /\ snapshot \in Rows \cup {Absent}
          /\ pending \in BOOLEAN
          /\ held \in [Workers -> 0..3]
          /\ used \in Nat
          /\ readSafe \in BOOLEAN
          /\ exportSafe \in BOOLEAN
          /\ deleteSafe \in BOOLEAN
CorruptionRefused == readSafe
CanonicalExport == exportSafe
ReplacementPreserved == deleteSafe
BudgetBounded == used <= MaxBytes /\ Active <= MaxActive
BudgetAccounted == used = SumHeld(Workers)
=============================================================================
