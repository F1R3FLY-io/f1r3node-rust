---------------------- MODULE FundingSourceSnapshot ----------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Readers, Sources, Parallelism, ReadLatest, PublishPartial
VARIABLES active, observed, values, phase, liveVersion
vars == <<active, observed, values, phase, liveVersion>>
Frozen == [s \in Sources |-> s]
Init ==
  /\ active = [r \in Readers |-> {}]
  /\ observed = [r \in Readers |-> {}]
  /\ values = [r \in Readers |-> [s \in Sources |-> 0]]
  /\ phase = [r \in Readers |-> "reading"]
  /\ liveVersion = 1
Start(r, s) ==
  /\ phase[r] = "reading"
  /\ s \notin active[r] \cup observed[r]
  /\ Cardinality(active[r]) < Parallelism
  /\ active' = [active EXCEPT ![r] = @ \cup {s}]
  /\ UNCHANGED <<observed, values, phase, liveVersion>>
Finish(r, s) ==
  /\ phase[r] = "reading"
  /\ s \in active[r]
  /\ active' = [active EXCEPT ![r] = @ \ {s}]
  /\ observed' = [observed EXCEPT ![r] = @ \cup {s}]
  /\ values' = [values EXCEPT ![r][s] = Frozen[s] + IF ReadLatest THEN liveVersion - 1 ELSE 0]
  /\ UNCHANGED <<phase, liveVersion>>
Fail(r, s) ==
  /\ phase[r] = "reading"
  /\ s \in active[r]
  /\ active' = [active EXCEPT ![r] = {}]
  /\ phase' = [phase EXCEPT ![r] = "failed"]
  /\ UNCHANGED <<observed, values, liveVersion>>
Publish(r) ==
  /\ phase[r] = "reading"
  /\ active[r] = {}
  /\ (PublishPartial \/ observed[r] = Sources)
  /\ phase' = [phase EXCEPT ![r] = "published"]
  /\ UNCHANGED <<active, observed, values, liveVersion>>
AdvanceLive ==
  /\ liveVersion = 1
  /\ liveVersion' = 2
  /\ UNCHANGED <<active, observed, values, phase>>
Next == AdvanceLive \/ (\E r \in Readers :
  Publish(r) \/ (\E s \in Sources : Start(r, s) \/ Finish(r, s) \/ Fail(r, s)))
Spec == Init /\ [][Next]_vars
TypeOK ==
  /\ active \in [Readers -> SUBSET Sources]
  /\ observed \in [Readers -> SUBSET Sources]
  /\ values \in [Readers -> [Sources -> 0..3]]
  /\ phase \in [Readers -> {"reading", "failed", "published"}]
  /\ liveVersion \in 1..2
  /\ \A r \in Readers : active[r] \cap observed[r] = {}
BoundedReads == \A r \in Readers : Cardinality(active[r]) <= Parallelism
FrozenReads == \A r \in Readers : \A s \in observed[r] : values[r][s] = Frozen[s]
CompletePublished == \A r \in Readers : phase[r] = "published" => observed[r] = Sources
PublishedSnapshot == \A r \in Readers : phase[r] = "published" => values[r] = Frozen
IndependentReadersAgree == \A a, b \in Readers :
  (phase[a] = "published" /\ phase[b] = "published") => values[a] = values[b]
=============================================================================
