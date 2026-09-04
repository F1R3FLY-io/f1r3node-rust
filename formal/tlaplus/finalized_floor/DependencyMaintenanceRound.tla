-------------------- MODULE DependencyMaintenanceRound --------------------
EXTENDS FiniteSets, TLC

CONSTANT
  \* @type: Bool;
  AbortRoundOnFailure

ASSUME AbortRoundOnFailure \in BOOLEAN

BlockObligations == {"block-a", "block-b"}
CertificateObligations == {"certificate-x", "certificate-y"}
Obligations == BlockObligations \union CertificateObligations
Phases == {"idle", "active", "complete"}
Results == {"none", "ok", "error"}

VARIABLES
  \* @type: Str;
  phase,
  \* @type: Set(Str);
  snapshot,
  \* @type: Set(Str);
  pending,
  \* @type: Set(Str);
  attempted,
  \* @type: Set(Str);
  failed,
  \* @type: Set(Str);
  skipped,
  \* @type: Str;
  firstError,
  \* @type: Str;
  result

vars == <<phase, snapshot, pending, attempted, failed, skipped,
  firstError, result>>

Init ==
  /\ phase = "idle"
  /\ snapshot = {}
  /\ pending = {}
  /\ attempted = {}
  /\ failed = {}
  /\ skipped = {}
  /\ firstError = "none"
  /\ result = "none"

StartRound ==
  /\ phase = "idle"
  /\ phase' = "active"
  /\ snapshot' = Obligations
  /\ pending' = Obligations
  /\ attempted' = {}
  /\ failed' = {}
  /\ skipped' = {}
  /\ firstError' = "none"
  /\ result' = "none"

AttemptSuccess(obligation) ==
  /\ phase = "active"
  /\ obligation \in pending
  /\ pending' = pending \ {obligation}
  /\ attempted' = attempted \union {obligation}
  /\ UNCHANGED <<phase, snapshot, failed, skipped, firstError, result>>

AttemptFailure(obligation) ==
  /\ phase = "active"
  /\ obligation \in pending
  /\ attempted' = attempted \union {obligation}
  /\ failed' = failed \union {obligation}
  /\ firstError' = IF firstError = "none" THEN obligation ELSE firstError
  /\ IF AbortRoundOnFailure
       THEN /\ skipped' = skipped \union (pending \ {obligation})
            /\ pending' = {}
       ELSE /\ skipped' = skipped
            /\ pending' = pending \ {obligation}
  /\ UNCHANGED <<phase, snapshot, result>>

Attempt(obligation) ==
  AttemptSuccess(obligation) \/ AttemptFailure(obligation)

FinishRound ==
  /\ phase = "active"
  /\ pending = {}
  /\ phase' = "complete"
  /\ result' = IF firstError = "none" THEN "ok" ELSE "error"
  /\ UNCHANGED <<snapshot, pending, attempted, failed, skipped, firstError>>

Idle ==
  /\ phase = "complete"
  /\ UNCHANGED vars

Next ==
  \/ StartRound
  \/ \E obligation \in Obligations : Attempt(obligation)
  \/ FinishRound
  \/ Idle

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(StartRound)
  /\ \A obligation \in Obligations : WF_vars(Attempt(obligation))
  /\ WF_vars(FinishRound)

TypeOK ==
  /\ phase \in Phases
  /\ snapshot \subseteq Obligations
  /\ pending \subseteq snapshot
  /\ attempted \subseteq snapshot
  /\ failed \subseteq attempted
  /\ skipped \subseteq snapshot
  /\ firstError \in Obligations \union {"none"}
  /\ result \in Results

AttemptedAndPendingPartitionSnapshot ==
  /\ attempted \cap pending = {}
  /\ attempted \union pending = snapshot

FailureNeverDiscardsUnattemptedObligations == skipped = {}

CompletedRoundAttemptedEntireSnapshot ==
  phase = "complete" => attempted = snapshot

FirstErrorNamesAnAttemptedFailure ==
  (firstError = "none") = (failed = {})
  /\ (firstError # "none" => firstError \in failed)

BlockFailureCannotSuppressCertificateMaintenance ==
  /\ phase = "complete"
  /\ failed \cap BlockObligations # {}
  => CertificateObligations \subseteq attempted

RoundEventuallyCompletes == <> (phase = "complete")

=============================================================================
