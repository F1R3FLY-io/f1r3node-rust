------------------------- MODULE DiskSampleValidation -------------------------
EXTENDS Naturals, TLC

CONSTANTS FloorMiB, BandMiB, ValidSamples, MalformedPrefixMiB, RejectMalformed

ASSUME /\ FloorMiB \in Nat \ {0}
       /\ BandMiB \in Nat
       /\ ValidSamples \subseteq Nat
       /\ MalformedPrefixMiB \in Nat
       /\ RejectMalformed \in BOOLEAN

VARIABLES phase, raw, parsed, admitted, evidence

vars == <<phase, raw, parsed, admitted, evidence>>
RawSamples == {[wellFormed |-> TRUE, numericPrefix |-> value] : value \in ValidSamples}
              \cup {[wellFormed |-> FALSE, numericPrefix |-> MalformedPrefixMiB]}
Unknown == [known |-> FALSE, freeMiB |-> 0]
ParsedSamples == {[known |-> TRUE, freeMiB |-> value] : value \in ValidSamples \cup {MalformedPrefixMiB}}
                 \cup {Unknown}

Init ==
    /\ phase = "probe"
    /\ raw = [wellFormed |-> FALSE, numericPrefix |-> MalformedPrefixMiB]
    /\ parsed = Unknown
    /\ admitted = FALSE
    /\ evidence = FALSE

Probe ==
    /\ phase = "probe"
    /\ raw' \in RawSamples
    /\ phase' = "decode"
    /\ UNCHANGED <<parsed, admitted, evidence>>

Decode ==
    /\ phase = "decode"
    /\ parsed' = IF RejectMalformed /\ ~raw.wellFormed
                    THEN Unknown
                    ELSE [known |-> TRUE, freeMiB |-> raw.numericPrefix]
    /\ phase' = "decide"
    /\ UNCHANGED <<raw, admitted, evidence>>

Decide ==
    /\ phase = "decide"
    /\ phase' = IF parsed.known /\ parsed.freeMiB >= FloorMiB + BandMiB
                   THEN "admit" ELSE "refused"
    /\ UNCHANGED <<raw, parsed, admitted, evidence>>

Admit ==
    /\ phase = "admit"
    /\ admitted' = TRUE
    /\ phase' = "running"
    /\ UNCHANGED <<raw, parsed, evidence>>

PublishRefusal ==
    /\ phase = "refused"
    /\ evidence' = TRUE
    /\ phase' = "done"
    /\ UNCHANGED <<raw, parsed, admitted>>

Next == Probe \/ Decode \/ Decide \/ Admit \/ PublishRefusal
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"probe", "decode", "decide", "admit", "refused", "running", "done"}
    /\ raw \in RawSamples
    /\ parsed \in ParsedSamples
    /\ admitted \in BOOLEAN
    /\ evidence \in BOOLEAN

AdmissionRequiresValidSample == admitted => raw.wellFormed
AdmissionRequiresBand == admitted => parsed.known /\ parsed.freeMiB >= FloorMiB + BandMiB
RefusalRecorded == phase = "done" => evidence /\ ~admitted
Completes == <>(admitted \/ (phase = "done" /\ evidence))
===============================================================================
