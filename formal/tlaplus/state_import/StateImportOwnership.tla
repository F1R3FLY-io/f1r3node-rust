-------------------------- MODULE StateImportOwnership --------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Actors, Roots, HistoryRefs, ColdRefs, RootRef, Children,
          ActorRoot, InitialRoot, Generations, OwnershipDefect
VARIABLES history, cold, tags, currentRoot, owner, phase, checked, frontier,
          pending, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked,
          writePermit, tagPermit, retirement, capturedRoot, unauthorizedTag, unmatchedWrite

Base == INSTANCE StateImportPublication WITH Defect <- "Safe"
BaseVars == <<history, cold, tags, currentRoot, owner, phase, checked, frontier,
    pending, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>
GhostVars == <<writePermit, tagPermit, retirement, capturedRoot, unauthorizedTag, unmatchedWrite>>
vars == <<BaseVars, GhostVars>>
None == "none"
NoRetirement == <<"none", 0>>
Attempts == Actors \X (1..Generations)
Refs == HistoryRefs \cup ColdRefs
ASSUME None \notin Refs
Owned(attempt) == Base!Owned(attempt)
Active(attempt) == Owned(attempt) /\ pending[attempt[1]]
AttemptRoot(attempt) == Base!AttemptRoot(attempt)

ASSUME OwnershipDefect \in {"Safe", "LateAuthority", "RetireClosedBeforeTag",
    "RetireFailedAfterTag", "FollowCurrentRoot", "UnmatchedWrite"}

Init ==
    /\ Base!Init
    /\ writePermit = [attempt \in Attempts |-> None]
    /\ tagPermit = [attempt \in Attempts |-> None]
    /\ retirement = [actor \in Actors |-> NoRetirement]
    /\ capturedRoot = None
    /\ unauthorizedTag = FALSE
    /\ unmatchedWrite = FALSE

ReserveWrite(attempt, ref) ==
    /\ phase[attempt] = "Load" /\ Active(attempt) /\ writePermit[attempt] = None
    /\ ref \in Base!Reach(AttemptRoot(attempt)) \ Base!Available
    /\ writePermit' = [writePermit EXCEPT ![attempt] = ref]
    /\ UNCHANGED <<BaseVars, tagPermit, retirement, capturedRoot, unauthorizedTag, unmatchedWrite>>

CommitWrite(attempt, ref) ==
    /\ phase[attempt] = "Load" /\ ref \in Refs
    /\ writePermit[attempt] = ref \/ OwnershipDefect = "UnmatchedWrite"
    /\ Base!WriteHistory(attempt, ref) \/ Base!WriteCold(attempt, ref)
        \/ (ref \in Base!Available /\ writePermit[attempt] = ref /\ UNCHANGED BaseVars)
    /\ writePermit' = [writePermit EXCEPT ![attempt] = None]
    /\ unmatchedWrite' = (unmatchedWrite \/ writePermit[attempt] # ref)
    /\ UNCHANGED <<tagPermit, retirement, capturedRoot, unauthorizedTag>>

BeginScan(attempt) ==
    /\ Base!BeginScan(attempt) /\ Active(attempt) /\ writePermit[attempt] = None
    /\ UNCHANGED GhostVars

CheckReference(attempt, ref) ==
    /\ Base!CheckReference(attempt, ref)
    /\ UNCHANGED GhostVars

AuthorizeTag(attempt) ==
    /\ Base!AuthorizeTag(attempt)
    /\ Active(attempt) \/ OwnershipDefect = "LateAuthority"
    /\ tagPermit' = [tagPermit EXCEPT ![attempt] = "Tag"]
    /\ unauthorizedTag' = (unauthorizedTag \/ ~Active(attempt))
    /\ UNCHANGED <<writePermit, retirement, capturedRoot, unmatchedWrite>>

WriteTag(attempt) ==
    /\ Base!WriteTag(attempt) /\ tagPermit[attempt] = "Tag"
    /\ tagPermit' = [tagPermit EXCEPT ![attempt] = "Pointer"]
    /\ UNCHANGED <<writePermit, retirement, capturedRoot, unauthorizedTag, unmatchedWrite>>

WriteCurrentRoot(attempt) ==
    /\ Base!WriteCurrentRoot(attempt) /\ tagPermit[attempt] = "Pointer"
    /\ tagPermit' = [tagPermit EXCEPT ![attempt] = None]
    /\ UNCHANGED <<writePermit, retirement, capturedRoot, unauthorizedTag, unmatchedWrite>>

Complete(attempt) ==
    /\ Base!Complete(attempt)
    /\ retirement' = [retirement EXCEPT ![attempt[1]] = <<"Attempt", attempt[2]>>]
    /\ tagPermit' = [tagPermit EXCEPT ![attempt] = None]
    /\ UNCHANGED <<writePermit, capturedRoot, unauthorizedTag, unmatchedWrite>>

FailAttempt(attempt) ==
    /\ Base!FailAttempt(attempt)
    /\ writePermit' = [writePermit EXCEPT ![attempt] = None]
    /\ tagPermit' = [tagPermit EXCEPT ![attempt] = None]
    /\ UNCHANGED <<retirement, capturedRoot, unauthorizedTag, unmatchedWrite>>

RetryAttempt(attempt) ==
    /\ Base!RetryAttempt(attempt)
    /\ UNCHANGED GhostVars

ReplaceRequest(actor) ==
    /\ Base!ReplaceRequest(actor)
    /\ retirement' = [retirement EXCEPT ![actor] = NoRetirement]
    /\ UNCHANGED <<writePermit, tagPermit, capturedRoot, unauthorizedTag, unmatchedWrite>>

ObservePublishedRoot(actor) ==
    /\ Base!ObservePublishedRoot(actor)
    /\ retirement' = [retirement EXCEPT ![actor] = <<"Observation", owner[actor]>>]
    /\ UNCHANGED <<writePermit, tagPermit, capturedRoot, unauthorizedTag, unmatchedWrite>>

CheckpointSelect(root) ==
    /\ Base!CheckpointSelect(root)
    /\ UNCHANGED GhostVars

ReaderCapture ==
    /\ Base!ReaderCapture
    /\ capturedRoot' = currentRoot
    /\ UNCHANGED <<writePermit, tagPermit, retirement, unauthorizedTag, unmatchedWrite>>

ReaderCheck(ref) == Base!ReaderCheck(ref) /\ UNCHANGED GhostVars
ReaderComplete == Base!ReaderComplete /\ UNCHANGED GhostVars

RetireFailedAttempt(attempt) ==
    /\ Owned(attempt) /\ phase[attempt] = "Failed" /\ pending[attempt[1]]
    /\ Base!Closed(AttemptRoot(attempt))
    /\ (OwnershipDefect = "RetireClosedBeforeTag" /\ AttemptRoot(attempt) \notin tags)
        \/ (OwnershipDefect = "RetireFailedAfterTag" /\ AttemptRoot(attempt) \in tags)
    /\ pending' = [pending EXCEPT ![attempt[1]] = FALSE]
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, phase, checked, frontier,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked, GhostVars>>

FollowCurrentRoot ==
    /\ OwnershipDefect = "FollowCurrentRoot" /\ readerPhase = "Read"
    /\ currentRoot # readerRoot
    /\ readerRoot' = currentRoot
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, phase, checked, frontier,
        pending, staleCompletion, readerPhase, readerTodo, readerChecked, GhostVars>>

Next ==
    \/ \E attempt \in Attempts :
        \/ \E ref \in Refs : ReserveWrite(attempt, ref) \/ CommitWrite(attempt, ref)
            \/ CheckReference(attempt, ref)
        \/ BeginScan(attempt) \/ AuthorizeTag(attempt) \/ WriteTag(attempt)
            \/ WriteCurrentRoot(attempt) \/ Complete(attempt)
            \/ FailAttempt(attempt) \/ RetryAttempt(attempt) \/ RetireFailedAttempt(attempt)
    \/ \E actor \in Actors : ReplaceRequest(actor) \/ ObservePublishedRoot(actor)
    \/ \E root \in Roots : CheckpointSelect(root)
    \/ ReaderCapture \/ ReaderComplete \/ FollowCurrentRoot
    \/ \E ref \in Refs : ReaderCheck(ref)

TypeOK ==
    /\ Base!TypeOK
    /\ writePermit \in [Attempts -> Refs \cup {None}]
    /\ tagPermit \in [Attempts -> {None, "Tag", "Pointer"}]
    /\ retirement \in [Actors -> ({"Attempt", "Observation"} \X (1..Generations)) \cup {NoRetirement}]
    /\ capturedRoot \in Roots \cup {None} /\ unauthorizedTag \in BOOLEAN
    /\ unmatchedWrite \in BOOLEAN

WritePermitsBoundToRoot == \A attempt \in Attempts : writePermit[attempt] # None =>
    /\ phase[attempt] = "Load"
    /\ writePermit[attempt] \in Base!Reach(AttemptRoot(attempt))
TagAuthorizationCurrent == ~unauthorizedTag
WriteCompletionMatched == ~unmatchedWrite
TagWritesAuthorized == \A attempt \in Attempts :
    /\ (phase[attempt] = "Ready" => tagPermit[attempt] = "Tag")
    /\ (phase[attempt] = "Tagged" => tagPermit[attempt] = "Pointer")
    /\ (phase[attempt] = "Pointed" => tagPermit[attempt] = None)
RetiredRecoveryHasPublishedEvidence == \A actor \in Actors : ~pending[actor] =>
    /\ ActorRoot[actor] \in tags /\ Base!Closed(ActorRoot[actor])
RetirementHasWitness == \A actor \in Actors : ~pending[actor] =>
    \/ retirement[actor] = <<"Observation", owner[actor]>>
    \/ (retirement[actor] = <<"Attempt", owner[actor]>>
        /\ phase[<<actor, owner[actor]>>] = "Complete")
ReaderUsesCapturedRoot == readerPhase # "Capture" => readerRoot = capturedRoot
ReaderCoversCapturedRoot == readerPhase # "Capture" =>
    /\ readerTodo \cup readerChecked = Base!Reach(capturedRoot)
    /\ readerTodo \cap readerChecked = {}
BaseInvariants == Base!FrontierCut /\ Base!PublishedRootsClosed /\ Base!CurrentRootClosed
    /\ Base!UnresolvedRecoveryOwned /\ Base!NoStaleCompletion /\ Base!CapturedReaderClosed
    /\ Base!ReaderValuesPreserved /\ Base!CompletedAttemptsClosed
RefinesBase == Base!Spec
Spec == Init /\ [][Next]_vars
=============================================================================
