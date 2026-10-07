-------------------- MODULE FundingCursorCells --------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Workers, Scopes, AtomicAcquire, CheckEveryRevision,
          PublishEverySuccessor, RestoreObserved
ASSUME /\ Workers # {} /\ IsFiniteSet(Workers)
       /\ Scopes # {} /\ IsFiniteSet(Scopes)
       /\ AtomicAcquire \in BOOLEAN /\ CheckEveryRevision \in BOOLEAN
       /\ PublishEverySuccessor \in BOOLEAN /\ RestoreObserved \in BOOLEAN

VARIABLES required, available, held, observed, pending, phase, revision, charges
vars == <<required, available, held, observed, pending, phase, revision, charges>>

Init ==
    /\ required \in [Workers -> {ss \in SUBSET Scopes : Cardinality(ss) <= 2}]
    /\ available = Scopes
    /\ held = [w \in Workers |-> {}]
    /\ observed = [w \in Workers |-> [s \in Scopes |-> 0]]
    /\ pending = [w \in Workers |-> {}]
    /\ phase = [w \in Workers |-> "waiting"]
    /\ revision = [s \in Scopes |-> 0]
    /\ charges = [s \in Scopes |-> 0]

Acquire(w, scopes) ==
    /\ phase[w] = "waiting"
    /\ scopes \subseteq available
    /\ IF AtomicAcquire THEN scopes = required[w]
       ELSE /\ scopes \subseteq required[w] \ held[w]
            /\ Cardinality(scopes) = 1
    /\ available' = available \ scopes
    /\ held' = [held EXCEPT ![w] = @ \cup scopes]
    /\ observed' = [observed EXCEPT ![w] =
         [s \in Scopes |-> IF s \in scopes THEN revision[s] ELSE observed[w][s]]]
    /\ phase' = [phase EXCEPT ![w] = IF held'[w] = required[w] THEN "owned" ELSE "waiting"]
    /\ UNCHANGED <<required, pending, revision, charges>>

Decide(w, fail) ==
    /\ phase[w] = "owned"
    /\ LET inspected == IF CheckEveryRevision THEN required[w]
                        ELSE IF required[w] = {} THEN {} ELSE {CHOOSE s \in required[w] : TRUE}
           accepted == ~fail /\ \A s \in inspected : observed[w][s] = 0
           updated == IF PublishEverySuccessor THEN required[w]
                      ELSE IF required[w] = {} THEN {} ELSE {CHOOSE s \in required[w] : TRUE}
       IN /\ revision' = [s \in Scopes |->
                IF s \notin required[w] THEN revision[s]
                ELSE IF accepted THEN IF s \in updated THEN observed[w][s] + 1 ELSE observed[w][s]
                ELSE IF RestoreObserved THEN observed[w][s] ELSE 0]
          /\ charges' = [s \in Scopes |->
                IF accepted /\ s \in required[w] THEN charges[s] + 1 ELSE charges[s]]
    /\ pending' = [pending EXCEPT ![w] = required[w]]
    /\ held' = [held EXCEPT ![w] = {}]
    /\ phase' = [phase EXCEPT ![w] = "publishing"]
    /\ UNCHANGED <<required, available, observed>>

Publish(w, scope) ==
    /\ phase[w] = "publishing" /\ scope \in pending[w]
    /\ available' = available \cup {scope}
    /\ pending' = [pending EXCEPT ![w] = @ \ {scope}]
    /\ UNCHANGED <<required, held, observed, phase, revision, charges>>

Finish(w) ==
    /\ phase[w] = "publishing" /\ pending[w] = {}
    /\ phase' = [phase EXCEPT ![w] = "done"]
    /\ UNCHANGED <<required, available, held, observed, pending, revision, charges>>

Next == (\E w \in Workers, scopes \in SUBSET Scopes : Acquire(w, scopes))
     \/ (\E w \in Workers, fail \in BOOLEAN : Decide(w, fail))
     \/ (\E w \in Workers, scope \in Scopes : Publish(w, scope))
     \/ (\E w \in Workers : Finish(w))
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ available \subseteq Scopes
    /\ held \in [Workers -> SUBSET Scopes]
    /\ pending \in [Workers -> SUBSET Scopes]
    /\ phase \in [Workers -> {"waiting", "owned", "publishing", "done"}]
    /\ revision \in [Scopes -> Nat] /\ charges \in [Scopes -> Nat]

ExactlyOneOwner == \A s \in Scopes :
    (IF s \in available THEN 1 ELSE 0) +
    Cardinality({w \in Workers : s \in held[w]}) +
    Cardinality({w \in Workers : s \in pending[w]}) = 1

NoPartialAcquisition == \A w \in Workers :
    /\ (phase[w] = "waiting" => held[w] = {})
    /\ (phase[w] = "owned" => held[w] = required[w])

OwnedValuesAreCoherent == \A w \in Workers :
    \A s \in held[w] : observed[w][s] = revision[s]

BothContributionsHaveSuccessors == \A s \in Scopes : revision[s] = charges[s]
CapturedRevisionUsedOnce == \A s \in Scopes : charges[s] <= 1

DisjointAcquisitionRemainsEnabled == \A w \in Workers :
    (phase[w] = "waiting" /\ required[w] \subseteq available) => ENABLED Acquire(w, required[w])

=============================================================================
