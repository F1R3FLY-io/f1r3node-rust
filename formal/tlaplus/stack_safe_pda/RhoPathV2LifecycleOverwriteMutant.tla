---- MODULE RhoPathV2LifecycleOverwriteMutant ----
EXTENDS Naturals, FiniteSets, TLC

VARIABLE phase, digestBucket

Init == /\ phase = "ready"
        /\ digestBucket = [d \in {"d-B", "d-T"} |->
              IF d = "d-B" THEN {"img-B-V1", "img-B-V2"} ELSE {}]

MutantCommit ==
    /\ phase = "ready"
    /\ phase' = "committed"
    /\ digestBucket' = [digestBucket EXCEPT !["d-B"] = {"img-B-V2"}]

Next == MutantCommit \/ (phase = "committed" /\ UNCHANGED <<phase, digestBucket>>)
Spec == Init /\ [][Next]_<<phase, digestBucket>>

DistinctCollisionRepresentativesPreserved ==
    {"img-B-V1", "img-B-V2"} \subseteq digestBucket["d-B"]

=============================================================================
