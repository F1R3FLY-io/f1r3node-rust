------------------------- MODULE JustificationHistory -------------------------
EXTENDS Naturals, TLC
CONSTANT UseLocalLatest
VARIABLES previous, candidate, invalid, available, latest, verdict
vars == <<previous, candidate, invalid, available, latest, verdict>>
Validators == {0, 1}
Seqs == 0..2
Init ==
    /\ previous \in [Validators -> Seqs]
    /\ candidate \in [Validators -> Seqs]
    /\ invalid \in [Validators -> BOOLEAN]
    /\ available \in [Validators -> BOOLEAN]
    /\ latest = 0
    /\ verdict = "pending"
Pair(v, reference) ==
    IF ~available[v] THEN "deferred"
    ELSE IF ~invalid[v] /\ candidate[v] < reference[v] THEN "rejected"
    ELSE "valid"
Judge(reference) ==
    IF Pair(0, reference) # "valid" THEN Pair(0, reference)
    ELSE Pair(1, reference)
Advance ==
    /\ latest = 0
    /\ latest' = 1
    /\ UNCHANGED <<previous, candidate, invalid, available, verdict>>
Validate ==
    /\ verdict = "pending"
    /\ verdict' = Judge(IF UseLocalLatest /\ latest = 1
                        THEN [v \in Validators |-> 2] ELSE previous)
    /\ UNCHANGED <<previous, candidate, invalid, available, latest>>
Next == Advance \/ Validate
Spec == Init /\ [][Next]_vars
HistoryRelative ==
    verdict = "pending" \/
    ((\A v \in Validators: available[v]) => verdict = Judge(previous))
MissingEvidenceNeverAccepted ==
    verdict = "valid" => \A v \in Validators: available[v]
GenuineRegressionRejected ==
    (\A v \in Validators: available[v]) /\
    (\E v \in Validators: ~invalid[v] /\ candidate[v] < previous[v]) /\
    verdict # "pending" => verdict = "rejected"
=============================================================================
