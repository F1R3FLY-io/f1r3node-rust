----------------------- MODULE StateImportPublication -----------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Actors, Roots, HistoryRefs, ColdRefs, RootRef, Children,
          ActorRoot, InitialRoot, Generations, Defect
VARIABLES history, cold, tags, currentRoot, owner, phase, checked, frontier,
          pending, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked

Refs == HistoryRefs \cup ColdRefs
Attempts == Actors \X (1..Generations)
None == "none"
vars == <<history, cold, tags, currentRoot, owner, phase, checked, frontier,
          pending, staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

RECURSIVE ReachWithin(_, _)
ReachWithin(root, depth) ==
    IF depth = 0 THEN {RootRef[root]}
    ELSE LET previous == ReachWithin(root, depth - 1)
         IN previous \cup UNION {Children[ref] : ref \in previous}

Reach(root) == ReachWithin(root, Cardinality(Refs))
Available == history \cup cold
Closed(root) == Reach(root) \subseteq Available
Owned(attempt) == attempt[2] = owner[attempt[1]]
AttemptRoot(attempt) == ActorRoot[attempt[1]]
ScanningPhases == {"Scan", "Ready", "Tagged", "Pointed", "Complete"}

ASSUME /\ Actors # {} /\ IsFiniteSet(Actors)
       /\ Roots # {} /\ IsFiniteSet(Roots)
       /\ IsFiniteSet(HistoryRefs) /\ IsFiniteSet(ColdRefs)
       /\ HistoryRefs \cap ColdRefs = {}
       /\ RootRef \in [Roots -> HistoryRefs]
       /\ Children \in [Refs -> SUBSET Refs]
       /\ \A ref \in ColdRefs : Children[ref] = {}
       /\ ActorRoot \in [Actors -> Roots]
       /\ InitialRoot \in Roots
       /\ Generations \in Nat \ {0}
       /\ None \notin Roots
       /\ Defect \in {"Safe", "EarlyTag", "RetireFailure", "StaleCompletion", "RemoveShared"}

Init ==
    /\ history = Reach(InitialRoot) \cap HistoryRefs
    /\ cold = Reach(InitialRoot) \cap ColdRefs
    /\ tags = {InitialRoot} /\ currentRoot = InitialRoot
    /\ owner = [actor \in Actors |-> 1]
    /\ phase = [attempt \in Attempts |-> IF attempt[2] = 1 THEN "Load" ELSE "Dormant"]
    /\ checked = [attempt \in Attempts |-> {}]
    /\ frontier = [attempt \in Attempts |-> {}]
    /\ pending = [actor \in Actors |-> TRUE]
    /\ staleCompletion = FALSE
    /\ readerPhase = "Capture" /\ readerRoot = None
    /\ readerTodo = {} /\ readerChecked = {}

CompatibleHistoryBatch(delta) ==
    /\ delta \subseteq HistoryRefs
    /\ history' = history \cup delta
    /\ UNCHANGED <<cold, tags, currentRoot, owner, phase, checked, frontier, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

CompatibleColdBatch(delta) ==
    /\ delta \subseteq ColdRefs
    /\ cold' = cold \cup delta
    /\ UNCHANGED <<history, tags, currentRoot, owner, phase, checked, frontier, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

WriteHistory(attempt, ref) ==
    /\ phase[attempt] = "Load"
    /\ ref \in (Reach(AttemptRoot(attempt)) \cap HistoryRefs) \ history
    /\ CompatibleHistoryBatch({ref})

WriteCold(attempt, ref) ==
    /\ phase[attempt] = "Load"
    /\ ref \in (Reach(AttemptRoot(attempt)) \cap ColdRefs) \ cold
    /\ CompatibleColdBatch({ref})

BeginScan(attempt) ==
    /\ phase[attempt] = "Load"
    /\ phase' = [phase EXCEPT ![attempt] = "Scan"]
    /\ checked' = [checked EXCEPT ![attempt] = {}]
    /\ frontier' = [frontier EXCEPT ![attempt] = {RootRef[AttemptRoot(attempt)]}]
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

CheckReference(attempt, ref) ==
    /\ phase[attempt] = "Scan" /\ ref \in frontier[attempt] \cap Available
    /\ checked' = [checked EXCEPT ![attempt] = @ \cup {ref}]
    /\ frontier' = [frontier EXCEPT ![attempt] =
        (@ \cup Children[ref]) \ (checked[attempt] \cup {ref})]
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, phase, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

AuthorizeTag(attempt) ==
    /\ phase[attempt] = "Scan" /\ frontier[attempt] = {}
    /\ phase' = [phase EXCEPT ![attempt] = "Ready"]
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, checked, frontier, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

WriteTag(attempt) ==
    /\ phase[attempt] = "Ready" \/ (Defect = "EarlyTag" /\ phase[attempt] = "Load")
    /\ tags' = tags \cup {AttemptRoot(attempt)}
    /\ phase' = [phase EXCEPT ![attempt] = "Tagged"]
    /\ UNCHANGED <<history, cold, currentRoot, owner, checked, frontier, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

WriteCurrentRoot(attempt) ==
    /\ phase[attempt] = "Tagged"
    /\ currentRoot' = AttemptRoot(attempt)
    /\ phase' = [phase EXCEPT ![attempt] = "Pointed"]
    /\ UNCHANGED <<history, cold, tags, owner, checked, frontier, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

Complete(attempt) ==
    /\ phase[attempt] = "Pointed"
    /\ Owned(attempt) \/ Defect = "StaleCompletion"
    /\ phase' = [phase EXCEPT ![attempt] = "Complete"]
    /\ pending' = [pending EXCEPT ![attempt[1]] = FALSE]
    /\ staleCompletion' = (staleCompletion \/ ~Owned(attempt))
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, checked, frontier,
        readerPhase, readerRoot, readerTodo, readerChecked>>

FailAttempt(attempt) ==
    /\ phase[attempt] \in {"Load", "Scan", "Ready", "Tagged", "Pointed"}
    /\ phase' = [phase EXCEPT ![attempt] = "Failed"]
    /\ pending' = IF Defect = "RetireFailure"
        THEN [pending EXCEPT ![attempt[1]] = FALSE] ELSE pending
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, checked, frontier,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

RetryAttempt(attempt) ==
    /\ phase[attempt] = "Failed" /\ Owned(attempt) /\ pending[attempt[1]]
    /\ phase' = [phase EXCEPT ![attempt] = "Load"]
    /\ checked' = [checked EXCEPT ![attempt] = {}]
    /\ frontier' = [frontier EXCEPT ![attempt] = {}]
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

ReplaceRequest(actor) ==
    /\ owner[actor] < Generations
    /\ owner' = [owner EXCEPT ![actor] = @ + 1]
    /\ phase' = [phase EXCEPT ![<<actor, owner[actor] + 1>>] = "Load"]
    /\ pending' = [pending EXCEPT ![actor] = TRUE]
    /\ UNCHANGED <<history, cold, tags, currentRoot, checked, frontier,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

ObservePublishedRoot(actor) ==
    /\ pending[actor] /\ ActorRoot[actor] \in tags /\ Closed(ActorRoot[actor])
    /\ pending' = [pending EXCEPT ![actor] = FALSE]
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, phase, checked, frontier,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

CheckpointSelect(root) ==
    /\ root \in tags /\ currentRoot # root
    /\ currentRoot' = root
    /\ UNCHANGED <<history, cold, tags, owner, phase, checked, frontier, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

ReaderCapture ==
    /\ readerPhase = "Capture"
    /\ readerRoot' = currentRoot
    /\ readerTodo' = Reach(currentRoot)
    /\ readerPhase' = "Read"
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, phase, checked, frontier,
        pending, staleCompletion, readerChecked>>

ReaderCheck(ref) ==
    /\ readerPhase = "Read" /\ ref \in readerTodo \cap Available
    /\ readerTodo' = readerTodo \ {ref}
    /\ readerChecked' = readerChecked \cup {ref}
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, phase, checked, frontier,
        pending, staleCompletion, readerPhase, readerRoot>>

ReaderComplete ==
    /\ readerPhase = "Read" /\ readerTodo = {}
    /\ readerPhase' = "Complete"
    /\ UNCHANGED <<history, cold, tags, currentRoot, owner, phase, checked, frontier,
        pending, staleCompletion, readerRoot, readerTodo, readerChecked>>

RemoveShared(ref) ==
    /\ Defect = "RemoveShared" /\ ref \in Available
    /\ \E left, right \in tags : left # right /\ ref \in Reach(left) \cap Reach(right)
    /\ history' = history \ {ref} /\ cold' = cold \ {ref}
    /\ UNCHANGED <<tags, currentRoot, owner, phase, checked, frontier, pending,
        staleCompletion, readerPhase, readerRoot, readerTodo, readerChecked>>

Next ==
    \/ \E delta \in SUBSET HistoryRefs : CompatibleHistoryBatch(delta)
    \/ \E delta \in SUBSET ColdRefs : CompatibleColdBatch(delta)
    \/ \E attempt \in Attempts :
        \/ \E ref \in Refs : WriteHistory(attempt, ref) \/ WriteCold(attempt, ref)
            \/ CheckReference(attempt, ref)
        \/ BeginScan(attempt) \/ AuthorizeTag(attempt) \/ WriteTag(attempt)
            \/ WriteCurrentRoot(attempt) \/ Complete(attempt)
            \/ FailAttempt(attempt) \/ RetryAttempt(attempt)
    \/ \E actor \in Actors : ReplaceRequest(actor) \/ ObservePublishedRoot(actor)
    \/ \E root \in Roots : CheckpointSelect(root)
    \/ ReaderCapture \/ ReaderComplete
    \/ \E ref \in Refs : ReaderCheck(ref) \/ RemoveShared(ref)

TypeOK ==
    /\ history \subseteq HistoryRefs /\ cold \subseteq ColdRefs
    /\ tags \subseteq Roots /\ currentRoot \in Roots
    /\ owner \in [Actors -> 1..Generations]
    /\ phase \in [Attempts -> {"Dormant", "Load", "Scan", "Ready", "Tagged", "Pointed", "Complete", "Failed"}]
    /\ checked \in [Attempts -> SUBSET Refs] /\ frontier \in [Attempts -> SUBSET Refs]
    /\ pending \in [Actors -> BOOLEAN] /\ staleCompletion \in BOOLEAN
    /\ readerPhase \in {"Capture", "Read", "Complete"}
    /\ readerRoot \in Roots \cup {None}
    /\ readerTodo \subseteq Refs /\ readerChecked \subseteq Refs

FrontierCut == \A attempt \in Attempts : phase[attempt] \in ScanningPhases =>
    /\ RootRef[AttemptRoot(attempt)] \in checked[attempt] \cup frontier[attempt]
    /\ checked[attempt] \subseteq Available
    /\ \A ref \in checked[attempt] : Children[ref] \subseteq checked[attempt] \cup frontier[attempt]

PublishedRootsClosed == \A root \in tags : Closed(root)
CurrentRootClosed == Closed(currentRoot)
UnresolvedRecoveryOwned == \A actor \in Actors : ~Closed(ActorRoot[actor]) => pending[actor]
NoStaleCompletion == ~staleCompletion
CapturedReaderClosed == readerRoot # None => Closed(readerRoot)
ReaderValuesPreserved == readerChecked \subseteq Available
CompletedAttemptsClosed == \A attempt \in Attempts :
    phase[attempt] = "Complete" => Closed(AttemptRoot(attempt))

Spec == Init /\ [][Next]_vars
=============================================================================
