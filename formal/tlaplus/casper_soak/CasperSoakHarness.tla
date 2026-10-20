-------------------------- MODULE CasperSoakHarness --------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Candidates, MaxSegments, MaxIterations,
          AllowIdentityDrift, AllowHistoryReset, AllowFailureErasure,
          AllowIncompletePass, AllowLaunchAfterStop, AllowEarlyCleanup,
          AllowPolicyLeak, AllowMissingAsZero, AllowUnmergedPostRun,
          AllowWrongControlVerdict

VARIABLE s
vars == <<s>>

Init ==
  \E candidate \in Candidates, phase \in {"pre", "post"}, verified \in BOOLEAN :
    s = [stage |-> "new", pinned |-> candidate, identity |-> candidate,
         phase |-> phase, mergeVerified |-> verified, admitted |-> FALSE,
         segment |-> 1, history |-> {}, recorded |-> {},
         failureSeen |-> FALSE, failed |-> FALSE, stopSeen |-> FALSE,
         lateLaunch |-> FALSE, captured |-> FALSE, evidenceComplete |-> FALSE,
         deleted |-> FALSE, baselinePolicy |-> "baseline", experimentSeen |-> FALSE,
         samplePresent |-> TRUE, measurement |-> "unknown", samplesComplete |-> TRUE,
         controlChecked |-> FALSE, controlExit |-> 0, controlMatches |-> FALSE,
         controlAccepted |-> FALSE, verdict |-> "pending"]

Admit ==
  /\ s.stage = "new"
  /\ (s.phase = "pre" \/ s.mergeVerified \/ AllowUnmergedPostRun)
  /\ s' = [s EXCEPT !.stage = "idle", !.admitted = TRUE]

Launch ==
  /\ s.admitted
  /\ s.stage \in {"idle", "stopped"}
  /\ Cardinality(s.history) < MaxIterations
  /\ (~s.stopSeen \/ AllowLaunchAfterStop)
  /\ s' = [s EXCEPT !.stage = "running", !.lateLaunch = s.lateLaunch \/ s.stopSeen]

Finish(bad, present) ==
  /\ s.stage = "running"
  /\ LET index == Cardinality(s.history) + 1 IN
       s' = [s EXCEPT !.stage = "idle",
                      !.history = s.history \union {index},
                      !.recorded = s.recorded \union {index},
                      !.failureSeen = s.failureSeen \/ bad,
                      !.failed = s.failed \/ bad,
                      !.samplePresent = present,
                      !.measurement = IF present THEN "value"
                                      ELSE IF AllowMissingAsZero THEN "zero" ELSE "unknown",
                      !.samplesComplete = s.samplesComplete /\ present]

Resume(candidate) ==
  /\ s.stage = "idle"
  /\ s.segment < MaxSegments
  /\ (candidate = s.pinned \/ AllowIdentityDrift)
  /\ s' = [s EXCEPT !.segment = s.segment + 1,
                    !.identity = candidate,
                    !.history = IF AllowHistoryReset THEN {} ELSE s.history]

Experiment ==
  /\ s.stage = "idle"
  /\ ~s.experimentSeen
  /\ s' = [s EXCEPT !.experimentSeen = TRUE,
                    !.baselinePolicy = IF AllowPolicyLeak THEN "experimental" ELSE "baseline"]

JudgeControl(code, matches) ==
  /\ s.stage = "idle"
  /\ ~s.controlChecked
  /\ s' = [s EXCEPT !.controlChecked = TRUE, !.controlExit = code,
                    !.controlMatches = matches,
                    !.controlAccepted = AllowWrongControlVerdict \/ (code = 12 /\ matches)]

Stop ==
  /\ s.stage \in {"idle", "running"}
  /\ s' = [s EXCEPT !.stage = "stopped", !.stopSeen = TRUE,
                    !.failed = IF AllowFailureErasure THEN FALSE ELSE s.failed]

Capture(complete) ==
  /\ s.stage = "stopped"
  /\ s' = [s EXCEPT !.stage = "captured", !.captured = TRUE, !.evidenceComplete = complete]

PassInputs ==
  /\ s.history = 1..MaxIterations
  /\ ~s.failed
  /\ s.samplesComplete
  /\ s.controlChecked /\ s.controlAccepted

Report ==
  /\ s.stage = "captured"
  /\ s' = [s EXCEPT !.stage = "reported",
                    !.verdict = IF s.failed THEN "product_failure"
                                ELSE IF PassInputs /\ (s.evidenceComplete \/ AllowIncompletePass)
                                     THEN "passed" ELSE "incomplete"]

Cleanup ==
  /\ s.stage \in {"stopped", "captured", "reported"}
  /\ ((s.captured /\ s.evidenceComplete) \/ AllowEarlyCleanup)
  /\ s' = [s EXCEPT !.stage = "done", !.deleted = TRUE]

Next ==
  \/ Admit
  \/ Launch
  \/ \E bad \in BOOLEAN, present \in BOOLEAN : Finish(bad, present)
  \/ \E candidate \in Candidates : Resume(candidate)
  \/ Experiment
  \/ \E code \in {0, 1, 12, 124}, matches \in BOOLEAN : JudgeControl(code, matches)
  \/ Stop
  \/ \E complete \in BOOLEAN : Capture(complete)
  \/ Report
  \/ Cleanup

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ s.stage \in {"new", "idle", "running", "stopped", "captured", "reported", "done"}
  /\ s.pinned \in Candidates /\ s.identity \in Candidates
  /\ s.phase \in {"pre", "post"}
  /\ s.segment \in 1..MaxSegments
  /\ s.history \subseteq 1..MaxIterations /\ s.recorded \subseteq 1..MaxIterations
  /\ s.baselinePolicy \in {"baseline", "experimental"}
  /\ s.measurement \in {"unknown", "zero", "value"}
  /\ s.controlExit \in {0, 1, 12, 124}
  /\ s.verdict \in {"pending", "passed", "product_failure", "incomplete"}
  /\ \A field \in {"mergeVerified", "admitted", "failureSeen", "failed", "stopSeen",
                    "lateLaunch", "captured", "evidenceComplete", "deleted", "experimentSeen",
                    "samplePresent", "samplesComplete", "controlChecked", "controlMatches", "controlAccepted"} :
       s[field] \in BOOLEAN

IdentityPinned == s.identity = s.pinned
ResumePreservesHistory == s.history = s.recorded
ProductFailureMonotone == s.failureSeen => s.failed
PassRequiresEvidence == s.verdict = "passed" =>
  (s.evidenceComplete /\ s.captured /\ ~s.failureSeen /\ PassInputs)
StopPreventsLaunch == ~s.lateLaunch
EvidenceBeforeCleanup == s.deleted => (s.captured /\ s.evidenceComplete)
PolicyIsolation == s.baselinePolicy = "baseline"
MissingIsUnknown == ~s.samplePresent => s.measurement = "unknown"
PostMergeGate == (s.admitted /\ s.phase = "post") => s.mergeVerified
ControlVerdictExact == s.controlAccepted =>
  (s.controlChecked /\ s.controlExit = 12 /\ s.controlMatches)
=============================================================================
