-------------------- MODULE NativeIndexPublication --------------------
EXTENDS Integers, FiniteSets

CONSTANTS CheckPreflight, CheckFailure
Workers == {1, 2}
Phases == {"idle", "work", "allocation", "ready", "failed", "done"}

VARIABLES phase, owner, work, storage, invalid, failed, frozen, published, debit
vars == <<phase, owner, work, storage, invalid, failed, frozen, published, debit>>

Init ==
  /\ phase = [w \in Workers |-> "idle"]
  /\ owner = 0
  /\ work = [w \in Workers |-> FALSE]
  /\ storage = [w \in Workers |-> FALSE]
  /\ invalid = FALSE
  /\ failed = FALSE
  /\ frozen = {}
  /\ published = {}
  /\ debit = 0

Begin(w) ==
  /\ owner = 0
  /\ ~invalid
  /\ phase[w] = "idle"
  /\ phase' = [phase EXCEPT ![w] = "work"]
  /\ owner' = w
  /\ UNCHANGED <<work, storage, invalid, failed, frozen, published, debit>>

ReserveWork(w) ==
  /\ owner = w
  /\ phase[w] = "work"
  /\ work' = [work EXCEPT ![w] = TRUE]
  /\ phase' = [phase EXCEPT ![w] = "allocation"]
  /\ UNCHANGED <<owner, storage, invalid, failed, frozen, published, debit>>

ReserveStorage(w) ==
  /\ owner = w
  /\ phase[w] = "allocation"
  /\ storage' = [storage EXCEPT ![w] = TRUE]
  /\ phase' = [phase EXCEPT ![w] = "ready"]
  /\ UNCHANGED <<owner, work, invalid, failed, frozen, published, debit>>

FailPreflight(w) ==
  /\ owner = w
  /\ phase[w] \in {"work", "allocation"}
  /\ phase' = [phase EXCEPT ![w] = "failed"]
  /\ owner' = 0
  /\ invalid' = CheckFailure
  /\ failed' = TRUE
  /\ frozen' = published
  /\ UNCHANGED <<work, storage, published, debit>>

DropFailedTicket(w) ==
  /\ phase[w] = "failed"
  /\ phase' = [phase EXCEPT ![w] = "done"]
  /\ invalid' = TRUE
  /\ UNCHANGED <<owner, work, storage, failed, frozen, published, debit>>

Publish(w) ==
  /\ (phase[w] = "ready" /\ owner = w)
      \/ (~CheckPreflight /\ phase[w] = "allocation" /\ owner = w)
  /\ ~invalid
  /\ phase' = [phase EXCEPT ![w] = "done"]
  /\ owner' = 0
  /\ published' = published \cup {w}
  /\ debit' = debit + 1
  /\ UNCHANGED <<work, storage, invalid, failed, frozen>>

Next == \E w \in Workers : Begin(w) \/ ReserveWork(w) \/ ReserveStorage(w)
  \/ FailPreflight(w) \/ DropFailedTicket(w) \/ Publish(w)
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in [Workers -> Phases]
  /\ owner \in {0, 1, 2}
  /\ work \in [Workers -> BOOLEAN]
  /\ storage \in [Workers -> BOOLEAN]
  /\ invalid \in BOOLEAN
  /\ failed \in BOOLEAN
  /\ frozen \subseteq Workers
  /\ published \subseteq Workers
  /\ debit \in 0..2
PreparedBeforePublication == \A w \in published : work[w] /\ storage[w]
FailureStopsPublication == failed => published = frozen
EffectsMatch == debit = Cardinality(published)
=============================================================================
