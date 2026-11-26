------------------------- MODULE JustificationHistory -------------------------
EXTENDS Naturals, TLC
CONSTANT UseLocalLatest
VARIABLES previous, candidate, available, latest, verdict
vars == <<previous, candidate, available, latest, verdict>>
Validators == {0, 1}
Seqs == 0..2
Sender == 0
Others == Validators \ {Sender}
Init ==
    /\ previous \in [Validators -> Seqs]
    /\ candidate \in [Validators -> Seqs]
    /\ available \in [Validators -> BOOLEAN]
    /\ latest = 0
    /\ verdict = "pending"
Pair(v, reference) ==
    IF ~available[v] THEN "deferred"
    ELSE IF candidate[v] < reference[v] THEN "rejected"
    ELSE "valid"
Judge(reference) == Pair(CHOOSE v \in Others: TRUE, reference)
Advance ==
    /\ latest = 0
    /\ latest' = 1
    /\ UNCHANGED <<previous, candidate, available, verdict>>
Validate ==
    /\ verdict = "pending"
    /\ verdict' = Judge(IF UseLocalLatest /\ latest = 1
                        THEN [v \in Validators |-> 2] ELSE previous)
    /\ UNCHANGED <<previous, candidate, available, latest>>
Next == Advance \/ Validate
Spec == Init /\ [][Next]_vars
HistoryRelative ==
    verdict = "pending" \/
    ((\A v \in Others: available[v]) => verdict = Judge(previous))
MissingEvidenceNeverAccepted ==
    verdict = "valid" => \A v \in Others: available[v]
GenuineRegressionRejected ==
    (\A v \in Others: available[v]) /\
    (\E v \in Others: candidate[v] < previous[v]) /\
    verdict # "pending" => verdict = "rejected"
=============================================================================
