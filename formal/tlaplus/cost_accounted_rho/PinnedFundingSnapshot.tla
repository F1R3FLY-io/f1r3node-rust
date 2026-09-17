---------------------- MODULE PinnedFundingSnapshot ----------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Readers, Parallelism, ReadCurrentHead, CheckActualHead
VARIABLES captured, active, observed, values, phase, liveStep
vars == <<captured, active, observed, values, phase, liveStep>>
Fields == {"balanceA", "balanceB", "resource", "fee"}
Roots == {1, 2}
LiveRoot == IF liveStep = 1 THEN 2 ELSE 1
Empty == [present |-> FALSE, amount |-> 0, revision |-> 0, position |-> 0]
Store(root) == [field \in Fields |->
  CASE field = "balanceA" -> [present |-> TRUE, amount |-> root, revision |-> 0, position |-> 0]
    [] field = "balanceB" -> [present |-> TRUE, amount |-> 3 - root, revision |-> 0, position |-> 0]
    [] field = "resource" -> [present |-> TRUE, amount |-> 0, revision |-> root - 1, position |-> root - 1]
    [] OTHER -> [present |-> root = 2, amount |-> 0, revision |-> 0, position |-> 0]]
Init ==
  /\ captured = [r \in Readers |-> 0]
  /\ active = [r \in Readers |-> {}]
  /\ observed = [r \in Readers |-> {}]
  /\ values = [r \in Readers |-> [field \in Fields |-> Empty]]
  /\ phase = [r \in Readers |-> "idle"]
  /\ liveStep = 0
Begin(r) ==
  /\ phase[r] = "idle"
  /\ captured' = [captured EXCEPT ![r] = LiveRoot]
  /\ phase' = [phase EXCEPT ![r] = "reading"]
  /\ UNCHANGED <<active, observed, values, liveStep>>
Start(r, field) ==
  /\ phase[r] = "reading"
  /\ field \notin active[r] \cup observed[r]
  /\ Cardinality(active[r]) < Parallelism
  /\ active' = [active EXCEPT ![r] = @ \cup {field}]
  /\ UNCHANGED <<captured, observed, values, phase, liveStep>>
Finish(r, field) ==
  /\ phase[r] = "reading"
  /\ field \in active[r]
  /\ active' = [active EXCEPT ![r] = @ \ {field}]
  /\ observed' = [observed EXCEPT ![r] = @ \cup {field}]
  /\ values' = [values EXCEPT ![r][field] =
       Store(IF ReadCurrentHead THEN LiveRoot ELSE captured[r])[field]]
  /\ UNCHANGED <<captured, phase, liveStep>>
Fail(r, field) ==
  /\ phase[r] = "reading"
  /\ field \in active[r]
  /\ active' = [active EXCEPT ![r] = {}]
  /\ phase' = [phase EXCEPT ![r] = "failed"]
  /\ UNCHANGED <<captured, observed, values, liveStep>>
Assemble(r) ==
  /\ phase[r] = "reading"
  /\ observed[r] = Fields
  /\ active[r] = {}
  /\ ~CheckActualHead \/ LiveRoot = captured[r]
  /\ phase' = [phase EXCEPT ![r] = "assembled"]
  /\ UNCHANGED <<captured, active, observed, values, liveStep>>
ChangeHead ==
  /\ liveStep < 2
  /\ liveStep' = liveStep + 1
  /\ UNCHANGED <<captured, active, observed, values, phase>>
Next == ChangeHead \/ (\E r \in Readers : Begin(r) \/ Assemble(r) \/
  (\E field \in Fields : Start(r, field) \/ Finish(r, field) \/ Fail(r, field)))
Spec == Init /\ [][Next]_vars
TypeOK ==
  /\ captured \in [Readers -> 0..2]
  /\ active \in [Readers -> SUBSET Fields]
  /\ observed \in [Readers -> SUBSET Fields]
  /\ values \in [Readers -> [Fields ->
       [present : BOOLEAN, amount : 0..2, revision : 0..1, position : 0..1]]]
  /\ phase \in [Readers -> {"idle", "reading", "failed", "assembled"}]
  /\ liveStep \in 0..2
  /\ \A r \in Readers : phase[r] # "idle" => captured[r] \in Roots
  /\ \A r \in Readers : active[r] \cap observed[r] = {}
BoundedReads == \A r \in Readers : Cardinality(active[r]) <= Parallelism
FrozenReads == \A r \in Readers : \A field \in observed[r] :
  values[r][field] = Store(captured[r])[field]
CompleteSnapshot == \A r \in Readers : phase[r] = "assembled" =>
  observed[r] = Fields /\ active[r] = {}
SnapshotAtCapturedRoot == \A r \in Readers : phase[r] = "assembled" =>
  values[r] = Store(captured[r])
SameRootAgreement == \A a, b \in Readers :
  (phase[a] = "assembled" /\ phase[b] = "assembled" /\ captured[a] = captured[b]) =>
  values[a] = values[b]
CursorPresence == \A r \in Readers : phase[r] = "assembled" =>
  values[r]["fee"].present = (captured[r] = 2)
ABAAssembly == \A r \in Readers :
  (phase[r] = "assembled" /\ captured[r] = 1 /\ liveStep = 2) =>
  values[r] = Store(1)
=============================================================================
