----------------------- MODULE DispatcherPresenceControl -----------------------
EXTENDS RecoveryDispatcherComposition

PresenceControlNext ==
    \/ StartService("recovery")
    \/ \E r \in Requests : ExternalRequest(r)
    \/ BeginPass \/ BeginAttempt
    \/ UpgradeEndpoint \/ EndpointOperation("none") \/ DropEndpoint
    \/ \E j \in Jobs : SelectCandidate(j) \/ LoadRecoveryBody(j)
    \/ \E k \in Captures : GrantCapture(k) \/ CaptureReturns(k) \/ TakeCapture(k)

PresenceControlSpec == Init /\ [][PresenceControlNext]_vars
=============================================================================
