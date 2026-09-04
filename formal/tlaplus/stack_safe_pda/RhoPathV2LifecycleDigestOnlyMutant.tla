---- MODULE RhoPathV2LifecycleDigestOnlyMutant ----
EXTENDS Naturals, FiniteSets, TLC

(***************************************************************************)
(* Negative control: a digest-only authority records one representative    *)
(* for d-B and silently loses the distinct V2 image.  The invariant below  *)
(* is intentionally violated in the initial state.                          *)
(***************************************************************************)

VARIABLE digestBucket

Init == digestBucket = [d \in {"d-B", "d-T"} |->
          IF d = "d-B" THEN {"img-B-V1"} ELSE {}]
Next == UNCHANGED digestBucket
Spec == Init /\ [][Next]_digestBucket

DistinctCollisionRepresentativesPreserved ==
    {"img-B-V1", "img-B-V2"} \subseteq digestBucket["d-B"]

=============================================================================
