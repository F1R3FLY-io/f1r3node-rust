---------------------- MODULE TransportConcurrency ----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
    \* @type: Set(Str);
    Requests,
    \* @type: Int;
    ClientPreSettingsLimit,
    \* @type: Int;
    Http2Limit,
    \* @type: Int;
    HandlerLimit,
    \* @type: Int;
    ItemLimit,
    \* @type: Int;
    MaxDecodedBytes,
    \* @type: Bool;
    BoundedHandlerExecution

ASSUME Requests # {}
ASSUME ClientPreSettingsLimit >= 1 /\ Http2Limit >= 1
ASSUME HandlerLimit >= 1 /\ ItemLimit >= 1 /\ MaxDecodedBytes >= 1

Phases == {"new", "initiated", "admitted", "handling", "retained",
           "done", "refused", "resourceRejected"}
TransportActivePhases == {"initiated", "admitted", "handling"}
TerminalPhases == {"done", "refused", "resourceRejected"}

VARIABLES
    \* @type: Str -> Str;
    phase,
    \* @type: Bool;
    settingsSeen,
    \* @type: Set(Str);
    reportedSuccess

vars == <<phase, settingsSeen, reportedSuccess>>

\* @type: Set(Str) => Set(Str);
InPhases(phases) == {request \in Requests : phase[request] \in phases}

TransportActive == InPhases(TransportActivePhases)
Admitted == InPhases({"admitted", "handling"})
Handling == InPhases({"handling"})
Retained == InPhases({"retained"})
Refused == InPhases({"refused"})
ResourceRejected == InPhases({"resourceRejected"})
Done == InPhases({"done"})

ClientLimit == IF settingsSeen THEN Http2Limit ELSE ClientPreSettingsLimit

Init ==
    /\ phase = [request \in Requests |-> "new"]
    /\ settingsSeen = FALSE
    /\ reportedSuccess = {}

Initiate(request) ==
    /\ phase[request] = "new"
    /\ Cardinality(TransportActive) < ClientLimit
    /\ phase' = [phase EXCEPT ![request] = "initiated"]
    /\ UNCHANGED <<settingsSeen, reportedSuccess>>

ObserveSettings ==
    /\ ~settingsSeen
    /\ settingsSeen' = TRUE
    /\ UNCHANGED <<phase, reportedSuccess>>

Admit(request) ==
    /\ phase[request] = "initiated"
    /\ Cardinality(Admitted) < Http2Limit
    /\ phase' = [phase EXCEPT ![request] = "admitted"]
    /\ UNCHANGED <<settingsSeen, reportedSuccess>>

Refuse(request) ==
    /\ phase[request] = "initiated"
    /\ Cardinality(Admitted) >= Http2Limit
    /\ phase' = [phase EXCEPT ![request] = "refused"]
    /\ UNCHANGED <<settingsSeen, reportedSuccess>>

StartHandler(request) ==
    /\ phase[request] = "admitted"
    /\ (~BoundedHandlerExecution \/ Cardinality(Handling) < HandlerLimit)
    /\ phase' = [phase EXCEPT ![request] = "handling"]
    /\ UNCHANGED <<settingsSeen, reportedSuccess>>

EnqueueAndAcknowledge(request) ==
    /\ phase[request] = "handling"
    /\ Cardinality(Retained) < ItemLimit
    /\ phase' = [phase EXCEPT ![request] = "retained"]
    /\ reportedSuccess' = reportedSuccess \cup {request}
    /\ UNCHANGED settingsSeen

RejectAtPayloadBudget(request) ==
    /\ phase[request] = "handling"
    /\ Cardinality(Retained) >= ItemLimit
    /\ phase' = [phase EXCEPT ![request] = "resourceRejected"]
    /\ UNCHANGED <<settingsSeen, reportedSuccess>>

CompleteHandling(request) ==
    /\ phase[request] = "retained"
    /\ phase' = [phase EXCEPT ![request] = "done"]
    /\ UNCHANGED <<settingsSeen, reportedSuccess>>

Quiescent ==
    /\ \A request \in Requests : phase[request] \in TerminalPhases
    /\ UNCHANGED vars

Next ==
    \/ \E request \in Requests : Initiate(request)
    \/ ObserveSettings
    \/ \E request \in Requests : Admit(request)
    \/ \E request \in Requests : Refuse(request)
    \/ \E request \in Requests : StartHandler(request)
    \/ \E request \in Requests : EnqueueAndAcknowledge(request)
    \/ \E request \in Requests : RejectAtPayloadBudget(request)
    \/ \E request \in Requests : CompleteHandling(request)
    \/ Quiescent

Fairness ==
    /\ WF_vars(ObserveSettings)
    /\ \A request \in Requests : WF_vars(Initiate(request))
    /\ \A request \in Requests : WF_vars(Admit(request))
    /\ \A request \in Requests : WF_vars(Refuse(request))
    /\ \A request \in Requests : WF_vars(StartHandler(request))
    /\ \A request \in Requests : WF_vars(EnqueueAndAcknowledge(request))
    /\ \A request \in Requests : WF_vars(RejectAtPayloadBudget(request))
    /\ \A request \in Requests : WF_vars(CompleteHandling(request))

Spec == Init /\ [][Next]_vars /\ Fairness

TypeOK ==
    /\ phase \in [Requests -> Phases]
    /\ settingsSeen \in BOOLEAN
    /\ reportedSuccess \subseteq Requests

Inv_TransportActiveBounded == Cardinality(TransportActive) <= Http2Limit
Inv_HandlerExecutionBounded == Cardinality(Handling) <= HandlerLimit
Inv_PreReservationDecodedBounded ==
    Cardinality(Handling) * MaxDecodedBytes <= HandlerLimit * MaxDecodedBytes
Inv_PayloadItemsBounded == Cardinality(Retained) <= ItemLimit
Inv_NoRequestsRefused == Refused = {}
Inv_NoPayloadBudgetRejection == ResourceRejected = {}
Inv_SuccessRequiresRemoteAcknowledgement ==
    \A request \in reportedSuccess : phase[request] \in {"retained", "done"}

Safety ==
    /\ TypeOK
    /\ Inv_TransportActiveBounded
    /\ Inv_HandlerExecutionBounded
    /\ Inv_PreReservationDecodedBounded
    /\ Inv_PayloadItemsBounded
    /\ Inv_NoRequestsRefused
    /\ Inv_NoPayloadBudgetRejection
    /\ Inv_SuccessRequiresRemoteAcknowledgement

Live_AllRequestsComplete ==
    \A request \in Requests : phase[request] = "new" ~> request \in Done

=============================================================================
