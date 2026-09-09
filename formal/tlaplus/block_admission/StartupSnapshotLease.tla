------------------------- MODULE StartupSnapshotLease -------------------------
EXTENDS StartupCompletion
CONSTANTS Leases, LeaseMode
VARIABLES leaseState, leaseRole, leaseOwner, usedLeases, snapshots, destroyed,
          episodeRole, released, badRelease, badActivation, badExactRelease,
          busyAllocations
leaseVars == <<leaseState, leaseRole, leaseOwner, usedLeases, snapshots, destroyed,
               episodeRole, released, badRelease, badActivation, badExactRelease,
               busyAllocations>>
allVars == <<vars, leaseVars>>

PendingLeases == {l \in Leases : leaseRole[l] = "pending"}
ActiveLeases == {l \in Leases : leaseRole[l] = "active"}
ActiveEpisodes == {l \in snapshots : episodeRole[l] = "active"}
Owned(r) == {l \in usedLeases : leaseOwner[l] = r}
Fresh(l) == l \in Leases \ usedLeases /\
            \A other \in Leases \ usedLeases : l <= other
CanCapture(r) == r \in Requests /\ Live(r) /\ stage[r] = "pending"

LeaseInit ==
    /\ Init
    /\ leaseState = [l \in Leases |-> "unused"]
    /\ leaseRole = [l \in Leases |-> "none"]
    /\ leaseOwner = [l \in Leases |-> None]
    /\ episodeRole = [l \in Leases |-> "pending"]
    /\ usedLeases = {} /\ snapshots = {} /\ destroyed = {} /\ released = {}
    /\ badRelease = FALSE /\ badActivation = FALSE /\ badExactRelease = FALSE
    /\ busyAllocations = 0

AfterRequestChange(publication) ==
    /\ LET retiring == {l \in PendingLeases : leaseState[l] = "stored"
                          /\ stage'[leaseOwner[l]] \in Terminal}
           early == {l \in PendingLeases :
                        (LeaseMode = "cancel-capture"
                         /\ leaseState[l] \in {"reserved", "capturing", "built"}
                         /\ stage'[leaseOwner[l]] \in Terminal)
                        \/ (LeaseMode = "publication-release" /\ publication
                            /\ l \in retiring)
                        \/ (LeaseMode = "early-pending-release" /\ l \in retiring)}
       IN /\ leaseState' = [l \in Leases |-> IF l \in retiring THEN "retiring" ELSE leaseState[l]]
          /\ leaseRole' = [l \in Leases |-> IF l \in early THEN "none" ELSE leaseRole[l]]
          /\ badRelease' = (badRelease \/ early \cap snapshots # {})
    /\ UNCHANGED <<leaseOwner, usedLeases, snapshots, destroyed, episodeRole,
                   released, badActivation, badExactRelease, busyAllocations>>

LeasePublish(c) == Publish(c) /\ AfterRequestChange(TRUE)
LeaseClear == ClearContext /\ AfterRequestChange(TRUE)
LeaseInstall(r) == Install(r) /\ AfterRequestChange(FALSE)
LeaseCancel(r) == Cancel(r) /\ AfterRequestChange(FALSE)
LeaseFail(r) == Fail(r) /\ AfterRequestChange(FALSE)
LeaseStop == Stop /\ AfterRequestChange(FALSE)

Reserve(l, r) ==
    /\ CanCapture(r) /\ Fresh(l)
    /\ PendingLeases = {} \/ LeaseMode = "double-pending"
    /\ leaseState' = [leaseState EXCEPT ![l] = "reserved"]
    /\ leaseRole' = [leaseRole EXCEPT ![l] = "pending"]
    /\ leaseOwner' = [leaseOwner EXCEPT ![l] = r]
    /\ usedLeases' = usedLeases \cup {l}
    /\ UNCHANGED <<vars, snapshots, destroyed, episodeRole, released,
                   badRelease, badActivation, badExactRelease, busyAllocations>>

BeginCapture(l) ==
    /\ leaseState[l] = "reserved" /\ CanCapture(leaseOwner[l])
    /\ leaseState' = [leaseState EXCEPT ![l] = "capturing"]
    /\ snapshots' = snapshots \cup {l}
    /\ UNCHANGED <<vars, leaseRole, leaseOwner, usedLeases, destroyed, episodeRole,
                   released, badRelease, badActivation, badExactRelease, busyAllocations>>

FinishCapture(l) ==
    /\ leaseState[l] = "capturing"
    /\ leaseState' = [leaseState EXCEPT ![l] = "built"]
    /\ UNCHANGED <<vars, leaseRole, leaseOwner, usedLeases, snapshots, destroyed,
                   episodeRole, released, badRelease, badActivation,
                   badExactRelease, busyAllocations>>

StoreCapture(l) ==
    /\ leaseState[l] = "built" /\ CanCapture(leaseOwner[l])
    /\ leaseRole[l] = "pending"
    /\ leaseState' = [leaseState EXCEPT ![l] = "stored"]
    /\ UNCHANGED <<vars, leaseRole, leaseOwner, usedLeases, snapshots, destroyed,
                   episodeRole, released, badRelease, badActivation,
                   badExactRelease, busyAllocations>>

DiscardCapture(l) ==
    /\ leaseState[l] \in {"reserved", "capturing", "built"}
    /\ leaseState' = [leaseState EXCEPT ![l] = "retiring"]
    /\ UNCHANGED <<vars, leaseRole, leaseOwner, usedLeases, snapshots, destroyed,
                   episodeRole, released, badRelease, badActivation,
                   badExactRelease, busyAllocations>>

LeaseActivate(l) ==
    /\ leaseRole[l] = "pending"
    /\ leaseState[l] = "stored" \/ LeaseMode = "activate-unstored"
    /\ Activate(leaseOwner[l])
    /\ leaseState' = [leaseState EXCEPT ![l] = "active"]
    /\ leaseRole' = [leaseRole EXCEPT ![l] = "active"]
    /\ episodeRole' = [episodeRole EXCEPT ![l] = "active"]
    /\ badActivation' = (badActivation \/ leaseState[l] # "stored"
                         \/ l \notin snapshots)
    /\ UNCHANGED <<leaseOwner, usedLeases, snapshots, destroyed, released,
                   badRelease, badExactRelease, busyAllocations>>

BeginActiveRetirement(l) ==
    /\ leaseState[l] = "active"
    /\ IF LeaseMode = "early-active-release" THEN Retire(leaseOwner[l])
       ELSE UNCHANGED vars
    /\ leaseState' = [leaseState EXCEPT ![l] = "retiring"]
    /\ leaseRole' = [leaseRole EXCEPT ![l] =
                       IF LeaseMode = "early-active-release" THEN "none" ELSE @]
    /\ badRelease' = (badRelease \/ (LeaseMode = "early-active-release" /\ l \in snapshots))
    /\ UNCHANGED <<leaseOwner, usedLeases, snapshots, destroyed, episodeRole,
                   released, badActivation, badExactRelease, busyAllocations>>

Destroy(l) ==
    /\ leaseState[l] = "retiring"
    /\ leaseState' = [leaseState EXCEPT ![l] = "destroyed"]
    /\ snapshots' = snapshots \ {l}
    /\ destroyed' = destroyed \cup {l}
    /\ UNCHANGED <<vars, leaseRole, leaseOwner, usedLeases, episodeRole, released,
                   badRelease, badActivation, badExactRelease, busyAllocations>>

Release(l) ==
    /\ leaseState[l] = "destroyed"
    /\ IF leaseRole[l] = "active" THEN
          /\ active' = active \ {leaseOwner[l]}
          /\ stage' = IF Live(leaseOwner[l]) /\ stage[leaseOwner[l]] \in {"presence", "admission"}
                       THEN [stage EXCEPT ![leaseOwner[l]] = "failed"] ELSE stage
          /\ UNCHANGED <<context, engine, usedContexts, current, usedRequests, scanned,
                         authorized, returned, succeeded, stopped, registered,
                         badAuthorization, badSuccess, badDrop, prepared>>
       ELSE UNCHANGED vars
    /\ leaseState' = [leaseState EXCEPT ![l] = "released"]
    /\ leaseRole' = [leaseRole EXCEPT ![l] = "none"]
    /\ released' = released \cup {l}
    /\ badRelease' = (badRelease \/ l \in snapshots)
    /\ UNCHANGED <<leaseOwner, usedLeases, snapshots, destroyed, episodeRole,
                   badActivation, badExactRelease, busyAllocations>>

UnsafeEarlyCapture(l, r) ==
    /\ LeaseMode = "early-capture" /\ CanCapture(r) /\ Fresh(l)
    /\ leaseState' = [leaseState EXCEPT ![l] = "capturing"]
    /\ leaseOwner' = [leaseOwner EXCEPT ![l] = r]
    /\ usedLeases' = usedLeases \cup {l} /\ snapshots' = snapshots \cup {l}
    /\ UNCHANGED <<vars, leaseRole, destroyed, episodeRole, released, badRelease,
                   badActivation, badExactRelease, busyAllocations>>

UnsafeStaleRelease(old, replacement) ==
    /\ LeaseMode = "stale-release" /\ old \in released
    /\ replacement \in PendingLeases \cup ActiveLeases /\ old # replacement
    /\ leaseRole' = [leaseRole EXCEPT ![replacement] = "none"]
    /\ badExactRelease' = TRUE
    /\ UNCHANGED <<vars, leaseState, leaseOwner, usedLeases, snapshots, destroyed,
                   episodeRole, released, badRelease, badActivation, busyAllocations>>

WrongRoleRelease(l) ==
    /\ leaseRole[l] = "active"
    /\ IF LeaseMode = "stale-role-release" THEN
          /\ leaseRole' = [leaseRole EXCEPT ![l] = "none"]
          /\ badExactRelease' = TRUE
          /\ UNCHANGED <<vars, leaseState, leaseOwner, usedLeases, snapshots, destroyed,
                         episodeRole, released, badRelease, badActivation, busyAllocations>>
       ELSE UNCHANGED allVars

UnsafeBusyAllocation(r) ==
    /\ LeaseMode = "busy-waiter" /\ CanCapture(r) /\ PendingLeases # {}
    /\ busyAllocations < 2 /\ busyAllocations' = busyAllocations + 1
    /\ UNCHANGED <<vars, leaseState, leaseRole, leaseOwner, usedLeases, snapshots,
                   destroyed, episodeRole, released, badRelease, badActivation, badExactRelease>>

CompletionOnly ==
    /\ \E r \in Requests : PresenceComplete(r) \/ ScanComplete(r) \/ Authorize(r)
                           \/ CallbackReturns(r) \/ Success(r)
    /\ UNCHANGED leaseVars

LeaseNext ==
    (\E c \in Contexts : LeasePublish(c)) \/ LeaseClear \/ LeaseStop \/ CompletionOnly
    \/ (\E r \in Requests : LeaseInstall(r) \/ LeaseCancel(r) \/ LeaseFail(r)
                            \/ UnsafeBusyAllocation(r))
    \/ (\E l \in Leases : BeginCapture(l) \/ FinishCapture(l) \/ StoreCapture(l)
         \/ DiscardCapture(l) \/ LeaseActivate(l) \/ BeginActiveRetirement(l)
         \/ Destroy(l) \/ Release(l) \/ WrongRoleRelease(l)
         \/ (\E r \in Requests : Reserve(l, r) \/ UnsafeEarlyCapture(l, r))
         \/ (\E old \in Leases : UnsafeStaleRelease(old, l)))

LeaseTypeOK ==
    /\ TypeOK
    /\ leaseState \in [Leases -> {"unused", "reserved", "capturing", "built",
                                   "stored", "active", "retiring", "destroyed", "released"}]
    /\ leaseRole \in [Leases -> {"none", "pending", "active"}]
    /\ leaseOwner \in [Leases -> Requests \cup {None}]
    /\ usedLeases \subseteq Leases /\ snapshots \subseteq usedLeases
    /\ destroyed \subseteq usedLeases /\ released \subseteq usedLeases
    /\ episodeRole \in [Leases -> {"pending", "active"}]
    /\ badRelease \in BOOLEAN /\ badActivation \in BOOLEAN /\ badExactRelease \in BOOLEAN
    /\ busyAllocations \in 0..2
Inv_EverySnapshotLeased == snapshots \subseteq PendingLeases \cup ActiveLeases
Inv_OnePendingOwner == Cardinality(PendingLeases) <= 1
Inv_OneActiveOwner == Cardinality(ActiveLeases \cup ActiveEpisodes) <= 1
Inv_AtMostTwoSnapshotOwners == Cardinality(snapshots) <= 2
Inv_ReleaseAfterDestruction == ~badRelease /\ released \subseteq destroyed
Inv_ActivationRequiresStoredWork == ~badActivation
Inv_ExactLeaseRelease == ~badExactRelease
Inv_BusyAllocatesNothing == busyAllocations = 0
Inv_ActiveCompletionCorrespondence == active = {leaseOwner[l] : l \in ActiveLeases}
Inv_StoredWorkCurrent == \A l \in Leases : leaseState[l] = "stored" =>
                         CanCapture(leaseOwner[l]) /\ leaseRole[l] = "pending"
LeaseSpec == LeaseInit /\ [][LeaseNext]_allVars
=============================================================================
