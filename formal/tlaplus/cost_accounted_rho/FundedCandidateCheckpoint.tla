-------------------- MODULE FundedCandidateCheckpoint --------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Workers, PublishEarly, PartialRollback, ReuseCanceled, KeepReplayTrace

Parts == {"Application", "Wallets", "Stacks", "Receipts"}
Phases == {"Ready", "Working", "Failed", "Discarded", "Done"}
PartAt(step) == CASE step = 1 -> "Application"
                 [] step = 2 -> "Wallets"
                 [] step = 3 -> "Stacks"
                 [] OTHER -> "Receipts"
Prefix(step) == {PartAt(index) : index \in 1..step}

VARIABLES phase, step, candidate, published, traceUsed, dirtyReplayStart
vars == <<phase, step, candidate, published, traceUsed, dirtyReplayStart>>

Init ==
  /\ phase = [w \in Workers |-> "Ready"]
  /\ step = [w \in Workers |-> 0]
  /\ candidate = [w \in Workers |-> {}]
  /\ published = [w \in Workers |-> {}]
  /\ traceUsed = [w \in Workers |-> FALSE]
  /\ dirtyReplayStart = [w \in Workers |-> FALSE]

Advance(w) ==
  /\ phase[w] \in {"Ready", "Working"}
  /\ step[w] < 4
  /\ dirtyReplayStart' = [dirtyReplayStart EXCEPT
       ![w] = @ \/ (phase[w] = "Ready" /\ traceUsed[w])]
  /\ phase' = [phase EXCEPT ![w] = "Working"]
  /\ step' = [step EXCEPT ![w] = @ + 1]
  /\ candidate' = [candidate EXCEPT ![w] = @ \cup {PartAt(step[w] + 1)}]
  /\ traceUsed' = [traceUsed EXCEPT ![w] = TRUE]
  /\ UNCHANGED published

Commit(w) ==
  /\ phase[w] = "Working"
  /\ step[w] = 4 \/ PublishEarly
  /\ phase' = [phase EXCEPT ![w] = "Done"]
  /\ published' = [published EXCEPT ![w] = candidate[w]]
  /\ UNCHANGED <<step, candidate, traceUsed, dirtyReplayStart>>

Fail(w) ==
  /\ phase[w] = "Working"
  /\ phase' = [phase EXCEPT ![w] = "Failed"]
  /\ candidate' = IF PartialRollback THEN candidate
                   ELSE [candidate EXCEPT ![w] = {}]
  /\ step' = [step EXCEPT ![w] = 0]
  /\ UNCHANGED <<published, traceUsed, dirtyReplayStart>>

Cancel(w) ==
  /\ phase[w] = "Working"
  /\ phase' = [phase EXCEPT ![w] = IF ReuseCanceled THEN "Ready" ELSE "Discarded"]
  /\ UNCHANGED <<step, candidate, published, traceUsed, dirtyReplayStart>>

Restart(w) ==
  /\ phase[w] \in {"Failed", "Discarded"}
  /\ phase' = [phase EXCEPT ![w] = "Ready"]
  /\ step' = [step EXCEPT ![w] = 0]
  /\ candidate' = [candidate EXCEPT ![w] = {}]
  /\ traceUsed' = IF KeepReplayTrace THEN traceUsed
                   ELSE [traceUsed EXCEPT ![w] = FALSE]
  /\ UNCHANGED <<published, dirtyReplayStart>>

Next == \E w \in Workers : Advance(w) \/ Commit(w) \/ Fail(w) \/ Cancel(w) \/ Restart(w)
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in [Workers -> Phases]
  /\ step \in [Workers -> 0..4]
  /\ candidate \in [Workers -> SUBSET Parts]
  /\ published \in [Workers -> SUBSET Parts]
  /\ traceUsed \in [Workers -> BOOLEAN]
  /\ dirtyReplayStart \in [Workers -> BOOLEAN]

AtomicPublication == \A w \in Workers : published[w] \in {{}, Parts}
CompleteSettlement == \A w \in Workers : phase[w] = "Done" => published[w] = Parts
UnpublishedCandidatePrivate == \A w \in Workers : phase[w] # "Done" => published[w] = {}
FailedCandidateRestored == \A w \in Workers : phase[w] = "Failed" => candidate[w] = {}
ReadyCandidateClean == \A w \in Workers : phase[w] = "Ready" =>
  /\ step[w] = 0
  /\ candidate[w] = {}
FreshReplayAttempt == \A w \in Workers : ~dirtyReplayStart[w]
WorkingPrefix == \A w \in Workers : phase[w] = "Working" => candidate[w] = Prefix(step[w])
IndependentCandidatesEnabled == \A w \in Workers :
  phase[w] \in {"Ready", "Working"} /\ step[w] < 4 => ENABLED Advance(w)
CompletedCandidatesAgree == \A left, right \in Workers :
  phase[left] = "Done" /\ phase[right] = "Done" => published[left] = published[right]
=============================================================================
