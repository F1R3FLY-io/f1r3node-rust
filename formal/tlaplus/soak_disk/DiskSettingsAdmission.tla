---------------------- MODULE DiskSettingsAdmission ----------------------
EXTENDS Naturals, Sequences, TLC
CONSTANT CheckRange
VARIABLES settings, phase, admitted, rejected
vars == <<settings, phase, admitted, rejected>>
Digits == "0123456789"
Maximum == "9223372036854775807"
Cases == {
    [floorText |-> "4096", bandText |-> "4096"],
    [floorText |-> "9223372036854775808", bandText |-> "4096"],
    [floorText |-> "4096", bandText |-> "9223372036854775808"],
    [floorText |-> "4096", bandText |-> "9223372036854771712"]
}
DigitAt(s, i) == CHOOSE n \in 0..9: SubSeq(Digits, n + 1, n + 1) = SubSeq(s, i, i)
Canonical(s) ==
    IF \A i \in 1..Len(s): SubSeq(s, i, i) = "0"
    THEN "0"
    ELSE SubSeq(s, CHOOSE k \in 1..Len(s):
        /\ SubSeq(s, k, k) # "0"
        /\ \A j \in 1..(k - 1): SubSeq(s, j, j) = "0", Len(s))
DecimalLE(a, b) ==
    LET x == Canonical(a) y == Canonical(b) IN
    \/ Len(x) < Len(y)
    \/ /\ Len(x) = Len(y)
       /\ (x = y \/ \E k \in 1..Len(x):
           /\ SubSeq(x, 1, k - 1) = SubSeq(y, 1, k - 1)
           /\ DigitAt(x, k) < DigitAt(y, k))
Padded(s) == SubSeq("00000000000000000000", 1, 20 - Len(s)) \o s
RECURSIVE AddDigits(_, _, _)
AddDigits(a, b, carry) ==
    IF Len(a) = 0 THEN IF carry = 0 THEN "" ELSE "1"
    ELSE LET n == Len(a)
             sum == DigitAt(a, n) + DigitAt(b, n) + carry
         IN AddDigits(SubSeq(a, 1, n - 1), SubSeq(b, 1, n - 1), sum \div 10)
            \o SubSeq(Digits, (sum % 10) + 1, (sum % 10) + 1)
Valid(s) ==
    /\ DecimalLE(s.floorText, Maximum)
    /\ DecimalLE(s.bandText, Maximum)
    /\ DecimalLE(AddDigits(Padded(s.floorText), Padded(s.bandText), 0), Maximum)
Init ==
    /\ settings \in Cases
    /\ phase = "config"
    /\ admitted = FALSE
    /\ rejected = FALSE
Decide ==
    /\ phase = "config"
    /\ admitted' = (~CheckRange \/ Valid(settings))
    /\ rejected' = ~admitted'
    /\ phase' = "checked"
    /\ UNCHANGED settings
Spec == Init /\ [][Decide]_vars /\ WF_vars(Decide)
TypeOK ==
    /\ settings \in Cases
    /\ phase \in {"config", "checked"}
    /\ admitted \in BOOLEAN
    /\ rejected \in BOOLEAN
AdmissionRequiresValidDiskSettings == admitted => Valid(settings)
RefusalRecorded == (phase = "checked" /\ ~admitted) => rejected
Completes == <>(phase = "checked")
=============================================================================
