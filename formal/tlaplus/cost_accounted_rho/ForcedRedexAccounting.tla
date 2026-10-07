------------------------- MODULE ForcedRedexAccounting -------------------------
EXTENDS Integers, TLC

CONSTANTS DeduplicateRegionAcrossEvents, ReplayIgnoresCertificate

ASSUME /\ DeduplicateRegionAcrossEvents \in BOOLEAN
       /\ ReplayIgnoresCertificate \in BOOLEAN

InitialDepth == 2
TargetForces == 2

VARIABLES phase, stackDepth, forced, certificate, replayDepth, replayForced

vars == <<phase, stackDepth, forced, certificate, replayDepth, replayForced>>

Init ==
  /\ phase = "Play"
  /\ stackDepth = InitialDepth
  /\ forced = 0
  /\ certificate = 0
  /\ replayDepth = InitialDepth
  /\ replayForced = 0

Force ==
  /\ phase = "Play"
  /\ forced < TargetForces
  /\ stackDepth > 0
  /\ stackDepth' =
       IF DeduplicateRegionAcrossEvents /\ forced > 0
       THEN stackDepth
       ELSE stackDepth - 1
  /\ forced' = forced + 1
  /\ phase' = IF forced' = TargetForces THEN "Certify" ELSE phase
  /\ UNCHANGED <<certificate, replayDepth, replayForced>>

Certify ==
  /\ phase = "Certify"
  /\ certificate' = InitialDepth - stackDepth
  /\ phase' = "Replay"
  /\ UNCHANGED <<stackDepth, forced, replayDepth, replayForced>>

ReplayForce ==
  /\ phase = "Replay"
  /\ replayForced < IF ReplayIgnoresCertificate THEN TargetForces + 1 ELSE certificate
  /\ replayDepth' = replayDepth - 1
  /\ replayForced' = replayForced + 1
  /\ phase' =
       IF replayForced' = IF ReplayIgnoresCertificate THEN TargetForces + 1 ELSE certificate
       THEN "Done"
       ELSE phase
  /\ UNCHANGED <<stackDepth, forced, certificate>>

Next == Force \/ Certify \/ ReplayForce

Spec == /\ Init
        /\ [][Next]_vars
        /\ WF_vars(Force)
        /\ WF_vars(Certify)
        /\ WF_vars(ReplayForce)

TypeOK ==
  /\ phase \in {"Play", "Certify", "Replay", "Done"}
  /\ stackDepth \in Nat
  /\ forced \in Nat
  /\ certificate \in Nat
  /\ replayDepth \in Int
  /\ replayForced \in Nat

EveryForcedRedexConsumesOne ==
  phase \in {"Play", "Certify"} => InitialDepth - stackDepth = forced

CertificateBindsEveryForcedRedex ==
  phase \in {"Replay", "Done"} => certificate = forced

ReplayStaysWithinCertificate == replayForced <= certificate

ReplayMatchesCertifiedConsumption ==
  phase = "Done" =>
    /\ replayForced = certificate
    /\ replayDepth = stackDepth

EventuallyDone == <>(phase = "Done")

=============================================================================
