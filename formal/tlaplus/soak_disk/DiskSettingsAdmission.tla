---------------------- MODULE DiskSettingsAdmission ----------------------
EXTENDS Naturals, Sequences, TLC
CONSTANT CheckRange
VARIABLES settings, phase, admitted, rejected, column, carry, sumText
vars == <<settings, phase, admitted, rejected, column, carry, sumText>>
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
Valid ==
    /\ DecimalLE(settings.floorText, Maximum)
    /\ DecimalLE(settings.bandText, Maximum)
    /\ carry = 0
    /\ DecimalLE(sumText, Maximum)
Init ==
    /\ settings \in Cases
    /\ phase = "adding"
    /\ admitted = FALSE
    /\ rejected = FALSE
    /\ column = 20
    /\ carry = 0
    /\ sumText = ""
AddColumn ==
    /\ phase = "adding"
    /\ LET total == DigitAt(Padded(settings.floorText), column)
                     + DigitAt(Padded(settings.bandText), column) + carry
       IN /\ sumText' = SubSeq(Digits, (total % 10) + 1, (total % 10) + 1) \o sumText
          /\ carry' = total \div 10
    /\ column' = column - 1
    /\ phase' = IF column = 1 THEN "config" ELSE "adding"
    /\ UNCHANGED <<settings, admitted, rejected>>
Decide ==
    /\ phase = "config"
    /\ admitted' = (~CheckRange \/ Valid)
    /\ rejected' = ~admitted'
    /\ phase' = "checked"
    /\ UNCHANGED <<settings, column, carry, sumText>>
Next == AddColumn \/ Decide
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK ==
    /\ settings \in Cases
    /\ phase \in {"adding", "config", "checked"}
    /\ admitted \in BOOLEAN
    /\ rejected \in BOOLEAN
    /\ column \in 0..20
    /\ carry \in 0..1
    /\ Len(sumText) = 20 - column
AdmissionRequiresValidDiskSettings == admitted => Valid
RefusalRecorded == (phase = "checked" /\ ~admitted) => rejected
Completes == <>(phase = "checked")
=============================================================================
