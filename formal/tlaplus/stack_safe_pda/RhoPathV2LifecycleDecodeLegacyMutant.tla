---- MODULE RhoPathV2LifecycleDecodeLegacyMutant ----
EXTENDS Naturals, FiniteSets, TLC

VARIABLE phase, decodedIdentity

Init == /\ phase = "decoded"
        /\ decodedIdentity = [image \in {"img-B-V1", "img-B-V2"} |-> "legacy-B"]
Next == UNCHANGED <<phase, decodedIdentity>>
Spec == Init /\ [][Next]_<<phase, decodedIdentity>>

FullImageIdentityPreserved ==
    decodedIdentity["img-B-V1"] # decodedIdentity["img-B-V2"]

=============================================================================
