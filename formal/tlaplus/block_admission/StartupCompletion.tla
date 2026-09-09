-------------------------- MODULE StartupCompletion --------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Contexts, Requests, Owner, None, CallbackRequired,
          ExactCompletion, ExactAuthorization, ExactSuccess, ExactDrop,
          AtomicStop, AtomicPublication, BoundedPending, CompletePhases,
          AwaitCallback, ExactRetirement
VARIABLES context, engine, usedContexts, current, active, usedRequests, stage,
          scanned, authorized, returned, succeeded, stopped, registered,
          badAuthorization, badSuccess, badDrop, prepared
vars == <<context, engine, usedContexts, current, active, usedRequests, stage,
          scanned, authorized, returned, succeeded, stopped, registered,
          badAuthorization, badSuccess, badDrop, prepared>>

Terminal == {"unused", "succeeded", "failed", "cancelled"}
Live(r) == ~stopped /\ current = r /\ context = Owner[r]
RequestOwner == [r \in Requests |-> IF r = 3 THEN 2 ELSE 1]

Init ==
    /\ context = None /\ engine = None /\ usedContexts = {}
    /\ current = None /\ active = {} /\ usedRequests = {}
    /\ stage = [r \in Requests |-> "unused"]
    /\ scanned = {} /\ authorized = {} /\ returned = {} /\ succeeded = {}
    /\ stopped = FALSE /\ registered = None /\ prepared = {}
    /\ badAuthorization = FALSE /\ badSuccess = FALSE /\ badDrop = FALSE

Revoke(s) == [r \in Requests |-> IF s[r] \in Terminal THEN s[r] ELSE "cancelled"]

Publish(c) ==
    /\ ~stopped /\ c \in Contexts \ usedContexts
    /\ context' = c
    /\ engine' = IF AtomicPublication THEN c ELSE engine
    /\ registered' = IF AtomicPublication THEN None ELSE c
    /\ usedContexts' = usedContexts \cup {c}
    /\ stage' = Revoke(stage)
    /\ UNCHANGED <<current, active, usedRequests, scanned, authorized, returned, succeeded,
                   stopped, badAuthorization, badSuccess, badDrop, prepared>>

FinishPublication ==
    /\ ~AtomicPublication /\ registered # None
    /\ engine' = registered /\ registered' = None
    /\ UNCHANGED <<context, usedContexts, current, active, usedRequests, stage,
                   scanned, authorized, returned, succeeded, stopped,
                   badAuthorization, badSuccess, badDrop, prepared>>

ClearContext ==
    /\ context # None
    /\ context' = None /\ engine' = None /\ registered' = None
    /\ stage' = Revoke(stage)
    /\ UNCHANGED <<current, usedContexts, active, usedRequests, scanned, authorized,
                   returned, succeeded, stopped, badAuthorization, badSuccess,
                   badDrop, prepared>>

Install(r) ==
    /\ ~stopped /\ r \in Requests \ usedRequests /\ context = Owner[r]
    /\ current' = r /\ usedRequests' = usedRequests \cup {r}
    /\ stage' = [q \in Requests |-> IF q = r THEN "pending"
                  ELSE IF BoundedPending /\ stage[q] \notin Terminal
                       THEN "cancelled" ELSE stage[q]]
    /\ UNCHANGED <<context, engine, usedContexts, active, scanned, authorized,
                   returned, succeeded, stopped, registered,
                   badAuthorization, badSuccess, badDrop, prepared>>

Activate(r) ==
    /\ Live(r) /\ stage[r] = "pending" /\ active = {}
    /\ active' = {r} /\ stage' = [stage EXCEPT ![r] = "presence"]
    /\ UNCHANGED <<context, engine, usedContexts, current, usedRequests,
                   scanned, authorized, returned, succeeded, stopped, registered,
                   badAuthorization, badSuccess, badDrop, prepared>>

PresenceComplete(r) ==
    /\ Live(r) /\ r \in active /\ stage[r] = "presence"
    /\ stage' = [stage EXCEPT ![r] = "admission"]
    /\ UNCHANGED <<context, engine, usedContexts, current, active, usedRequests,
                   scanned, authorized, returned, succeeded, stopped, registered,
                   badAuthorization, badSuccess, badDrop, prepared>>

ScanComplete(r) ==
    /\ r \in active
    /\ IF ExactCompletion THEN Live(r) ELSE current # None
    /\ LET target == IF ExactCompletion THEN r ELSE current
       IN /\ (stage[target] = "admission" \/ ~CompletePhases \/ ~ExactCompletion)
          /\ stage' = [stage EXCEPT ![target] = "ready"]
          /\ scanned' = IF stage[r] = "admission" /\ Live(r)
                         THEN scanned \cup {r} ELSE scanned
    /\ UNCHANGED <<context, engine, usedContexts, current, active, usedRequests,
                   authorized, returned, succeeded, stopped, registered,
                   badAuthorization, badSuccess, badDrop, prepared>>

Retire(r) ==
    /\ IF ExactRetirement THEN r \in active ELSE active # {} /\ r \in usedRequests
    /\ active' = IF ExactRetirement THEN active \ {r} ELSE {}
    /\ badDrop' = (badDrop \/ r \notin active)
    /\ UNCHANGED <<context, engine, usedContexts, current, usedRequests, stage,
                   scanned, authorized, returned, succeeded, stopped, registered,
                   badAuthorization, badSuccess, prepared>>

PrepareAuthorization(r) ==
    /\ ~AtomicStop /\ Live(r) /\ stage[r] = "ready"
    /\ prepared' = prepared \cup {r}
    /\ UNCHANGED <<context, engine, usedContexts, current, active, usedRequests,
                   stage, scanned, authorized, returned, succeeded, stopped,
                   registered, badAuthorization, badSuccess, badDrop>>

Authorize(r) ==
    /\ r \in CallbackRequired
    /\ IF AtomicStop THEN
          IF ExactAuthorization THEN Live(r) /\ stage[r] = "ready"
          ELSE r \in scanned /\ r \notin authorized
       ELSE r \in prepared /\ r \notin authorized
    /\ badAuthorization' = (badAuthorization \/ ~(Live(r) /\ stage[r] = "ready"))
    /\ authorized' = authorized \cup {r}
    /\ stage' = [stage EXCEPT ![r] = "authorized"]
    /\ UNCHANGED <<context, engine, usedContexts, current, active, usedRequests,
                   scanned, returned, succeeded, stopped, registered,
                   badSuccess, badDrop, prepared>>

CallbackReturns(r) ==
    /\ r \in authorized /\ r \notin returned
    /\ returned' = returned \cup {r}
    /\ UNCHANGED <<context, engine, usedContexts, current, active, usedRequests,
                   stage, scanned, authorized, succeeded, stopped, registered,
                   badAuthorization, badSuccess, badDrop, prepared>>

Success(r) ==
    /\ IF ExactSuccess THEN Live(r) /\
          ((stage[r] = "ready" /\ r \notin CallbackRequired)
           \/ (stage[r] = "authorized" /\ (r \in returned \/ ~AwaitCallback)))
       ELSE r \in returned /\ r \notin succeeded
    /\ badSuccess' = (badSuccess \/ ~(Live(r) /\
          ((stage[r] = "ready" /\ r \notin CallbackRequired)
           \/ (stage[r] = "authorized" /\ r \in returned))))
    /\ stage' = [stage EXCEPT ![r] = "succeeded"]
    /\ succeeded' = succeeded \cup {r}
    /\ UNCHANGED <<context, engine, usedContexts, current, active, usedRequests,
                   scanned, authorized, returned, stopped, registered,
                   badAuthorization, badDrop, prepared>>

Cancel(r) ==
    /\ r \in usedRequests
    /\ current' = IF ExactDrop THEN current ELSE None
    /\ stage' = [stage EXCEPT ![r] = IF @ \in Terminal THEN @ ELSE "cancelled"]
    /\ badDrop' = (badDrop \/ (~ExactDrop /\ current # None /\ current # r))
    /\ UNCHANGED <<context, engine, usedContexts, active, usedRequests,
                   scanned, authorized, returned, succeeded, stopped, registered,
                   badAuthorization, badSuccess, prepared>>

Fail(r) ==
    /\ Live(r) /\ stage[r] \notin Terminal
    /\ stage' = [stage EXCEPT ![r] = "failed"]
    /\ UNCHANGED <<context, engine, usedContexts, current, active, usedRequests,
                   scanned, authorized, returned, succeeded, stopped, registered,
                   badAuthorization, badSuccess, badDrop, prepared>>

Stop ==
    /\ ~stopped /\ stopped' = TRUE
    /\ stage' = Revoke(stage)
    /\ context' = None /\ registered' = None
    /\ UNCHANGED <<current, engine, usedContexts, active, usedRequests, scanned, authorized,
                   returned, succeeded, badAuthorization, badSuccess, badDrop, prepared>>

Next == (\E c \in Contexts : Publish(c)) \/ FinishPublication \/ ClearContext \/ Stop
        \/ (\E r \in Requests : Install(r) \/ Activate(r) \/ PresenceComplete(r)
           \/ ScanComplete(r) \/ Retire(r) \/ PrepareAuthorization(r)
           \/ Authorize(r) \/ CallbackReturns(r) \/ Success(r) \/ Cancel(r) \/ Fail(r))

TypeOK ==
    /\ context \in Contexts \cup {None} /\ engine \in Contexts \cup {None}
    /\ usedContexts \subseteq Contexts /\ current \in Requests \cup {None}
    /\ active \subseteq Requests /\ usedRequests \subseteq Requests
    /\ stage \in [Requests -> {"unused", "pending", "presence", "admission",
                               "ready", "authorized", "succeeded", "failed", "cancelled"}]
    /\ scanned \subseteq Requests /\ authorized \subseteq Requests
    /\ returned \subseteq Requests /\ succeeded \subseteq Requests
    /\ stopped \in BOOLEAN /\ registered \in Contexts \cup {None}
    /\ badAuthorization \in BOOLEAN /\ badSuccess \in BOOLEAN /\ badDrop \in BOOLEAN
    /\ prepared \subseteq Requests
Inv_StartupCompletionIdentity == {r \in Requests : stage[r] \in {"ready", "authorized", "succeeded"}} \subseteq scanned
Inv_StartupAuthorizationOrigin == ~badAuthorization /\ authorized \subseteq scanned
Inv_StartupSuccessOrigin == ~badSuccess /\ succeeded \subseteq scanned
Inv_StartupRetirementOwnership == ~badDrop
Inv_RecoveryStopTerminal == stopped =>
    (current = None \/ stage[current] \in Terminal) /\ ~badAuthorization /\ ~badSuccess
Inv_ContextPublication == ~stopped => context = engine
Inv_StartupOwnershipBound == Cardinality(active) <= 1 /\ Cardinality({r \in Requests : stage[r] = "pending"}) <= 1
Inv_CommittedSuccessPreserved == \A r \in succeeded : stage[r] = "succeeded"
Spec == Init /\ [][Next]_vars
=============================================================================
