-------------------- MODULE NativeReplayAuthority --------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Workers, Events, Purses, Demand, Capacity, Granted, Transfers,
          MaxStarts, OmitDebit, OmitAuthorityRestore, IgnorePending
VARIABLES phase, owner, published, amounts, held, starts, boundary, snapshot
vars == <<phase, owner, published, amounts, held, starts, boundary, snapshot>>

Zero == [p \in Purses |-> 0]
Add(a, b) == [p \in Purses |-> a[p] + b[p]]
Sub(a, b) == [p \in Purses |-> a[p] - b[p]]
Fits(a) == \A p \in Purses : a[p] <= Capacity[p]
RECURSIVE Total(_)
Total(events) == IF events = {} THEN Zero
                ELSE LET e == CHOOSE e \in events : TRUE
                     IN Add(Demand[e], Total(events \ {e}))
Active == {e \in Events : phase[e] = "Prepared"}
Done == {e \in Events : phase[e] = "Published"}
EmptySnapshot == [valid |-> FALSE, done |-> {}, value |-> Zero]

Init ==
  /\ phase = [e \in Events |-> "Available"]
  /\ owner = [w \in Workers |-> {}]
  /\ published = {}
  /\ amounts = Zero
  /\ held = Zero
  /\ starts = 0
  /\ boundary = FALSE
  /\ snapshot = EmptySnapshot

Prepare(w, e) ==
  /\ ~boundary /\ owner[w] = {} /\ phase[e] = "Available"
  /\ e \in Granted /\ starts < MaxStarts
  /\ Fits(Add(IF IgnorePending THEN amounts ELSE held, Demand[e]))
  /\ phase' = [phase EXCEPT ![e] = "Prepared"]
  /\ owner' = [owner EXCEPT ![w] = {e}]
  /\ held' = Add(held, Demand[e])
  /\ starts' = starts + 1
  /\ UNCHANGED <<published, amounts, boundary, snapshot>>

Reject(e) ==
  /\ ~boundary /\ phase[e] = "Available" /\ e \notin Granted
  /\ phase' = [phase EXCEPT ![e] = "Denied"]
  /\ UNCHANGED <<owner, published, amounts, held, starts, boundary, snapshot>>

Publish(w, e) ==
  /\ ~boundary /\ owner[w] = {e} /\ phase[e] = "Prepared"
  /\ phase' = [phase EXCEPT ![e] = "Published"]
  /\ owner' = [owner EXCEPT ![w] = {}]
  /\ published' = published \cup {e}
  /\ amounts' = IF OmitDebit /\ e \notin Transfers THEN amounts
                ELSE Add(amounts, Demand[e])
  /\ UNCHANGED <<held, starts, boundary, snapshot>>

Cancel(w, e) ==
  /\ ~boundary /\ owner[w] = {e} /\ phase[e] = "Prepared"
  /\ phase' = [phase EXCEPT ![e] = "Available"]
  /\ owner' = [owner EXCEPT ![w] = {}]
  /\ held' = Sub(held, Demand[e])
  /\ UNCHANGED <<published, amounts, starts, boundary, snapshot>>

BeginBoundary ==
  /\ ~boundary /\ Active = {}
  /\ boundary' = TRUE
  /\ UNCHANGED <<phase, owner, published, amounts, held, starts, snapshot>>

Capture ==
  /\ boundary
  /\ snapshot' = [valid |-> TRUE, done |-> Done, value |-> amounts]
  /\ UNCHANGED <<phase, owner, published, amounts, held, starts, boundary>>

Restore ==
  /\ boundary /\ snapshot.valid /\ snapshot.done \subseteq Done
  /\ phase' = [e \in Events |-> IF e \in snapshot.done THEN "Published" ELSE "Available"]
  /\ published' = snapshot.done
  /\ amounts' = IF OmitAuthorityRestore THEN amounts ELSE snapshot.value
  /\ held' = IF OmitAuthorityRestore THEN held ELSE snapshot.value
  /\ UNCHANGED <<owner, starts, boundary, snapshot>>

EndBoundary ==
  /\ boundary
  /\ boundary' = FALSE
  /\ UNCHANGED <<phase, owner, published, amounts, held, starts, snapshot>>

Next == (\E w \in Workers, e \in Events : Prepare(w, e) \/ Publish(w, e) \/ Cancel(w, e))
     \/ (\E e \in Events : Reject(e))
     \/ BeginBoundary \/ Capture \/ Restore \/ EndBoundary
Spec == Init /\ [][Next]_vars

TypeOK == /\ phase \in [Events -> {"Available", "Prepared", "Published", "Denied"}]
          /\ owner \in [Workers -> SUBSET Events]
          /\ amounts \in [Purses -> Nat] /\ held \in [Purses -> Nat]
          /\ starts \in 0..MaxStarts /\ boundary \in BOOLEAN
          /\ published \subseteq Events
          /\ snapshot \in [valid : BOOLEAN, done : SUBSET Events, value : [Purses -> Nat]]
ExclusiveOwnership == /\ \A w \in Workers : Cardinality(owner[w]) <= 1
                      /\ \A w, v \in Workers : w = v \/ owner[w] \cap owner[v] = {}
                      /\ UNION {owner[w] : w \in Workers} = Active
ExactPublication == published = Done
ExactAuthority == amounts = Total(published)
ExactReservation == held = Add(amounts, Total(Active))
NoOverdraw == Fits(held)
DeniedUncharged == (Done \cup Active) \subseteq Granted
QuiescentBoundary == boundary => Active = {}
SnapshotAuthority == snapshot.valid => snapshot.value = Total(snapshot.done)
IndependentPreparation == \A w \in Workers, e \in Events :
  (~boundary /\ owner[w] = {} /\ phase[e] = "Available" /\ e \in Granted
   /\ starts < MaxStarts /\ Fits(Add(held, Demand[e]))) => ENABLED Prepare(w, e)
=============================================================================
