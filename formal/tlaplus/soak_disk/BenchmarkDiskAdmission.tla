-------------------- MODULE BenchmarkDiskAdmission --------------------
EXTENDS Naturals, TLC

CONSTANTS CheckDiskBand, FloorMiB, BandMiB, FreeValues
VARIABLES phase, freeMiB, sampleMiB, admitted, refused, failures
vars == <<phase, freeMiB, sampleMiB, admitted, refused, failures>>

Init ==
    /\ phase = "sample"
    /\ freeMiB \in FreeValues
    /\ sampleMiB = 0
    /\ admitted = FALSE
    /\ refused = FALSE
    /\ failures = 0

Probe ==
    /\ phase = "sample"
    /\ sampleMiB' = freeMiB
    /\ phase' = "decide"
    /\ UNCHANGED <<freeMiB, admitted, refused, failures>>

Decide ==
    /\ phase = "decide"
    /\ admitted' = (~CheckDiskBand \/ sampleMiB >= FloorMiB + BandMiB)
    /\ refused' = (CheckDiskBand /\ sampleMiB < FloorMiB + BandMiB)
    /\ phase' = "publish"
    /\ UNCHANGED <<freeMiB, sampleMiB, failures>>

Publish ==
    /\ phase = "publish"
    /\ failures' = IF refused THEN 1 ELSE 0
    /\ phase' = "done"
    /\ UNCHANGED <<freeMiB, sampleMiB, admitted, refused>>

Next == Probe \/ Decide \/ Publish
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"sample", "decide", "publish", "done"}
    /\ freeMiB \in FreeValues
    /\ sampleMiB \in FreeValues \cup {0}
    /\ admitted \in BOOLEAN
    /\ refused \in BOOLEAN
    /\ failures \in 0..1

BenchmarkRequiresBand == admitted => sampleMiB >= FloorMiB + BandMiB
RefusalStopsAdmission == refused => ~admitted
RefusalRecorded == (phase = "done" /\ refused) => failures = 1
Completes == <>(phase = "done")
=============================================================================
