-------------------- MODULE NativeOperationLifecycle --------------------
EXTENDS Naturals, FiniteSets

CONSTANTS CaptureObserver, InfallibleCompletion
Operations == {1, 2}
Observers == {1, 2}
Outcomes == {"Stored", "Matched", "IntroductionDenied", "CommDenied"}
Stages == {"Idle", "Started", "Introduced", "Decided", "Applied", "Finished"}

VARIABLES observer, stage, owner, seen, outcome, mutated, returned
vars == <<observer, stage, owner, seen, outcome, mutated, returned>>
CallbackOwner(op) == IF CaptureObserver THEN owner[op] ELSE observer

Init ==
  /\ observer = 1
  /\ stage = [op \in Operations |-> "Idle"]
  /\ owner = [op \in Operations |-> 1]
  /\ seen = [op \in Operations |-> {}]
  /\ outcome = [op \in Operations |-> "Stored"]
  /\ mutated = [op \in Operations |-> FALSE]
  /\ returned = [op \in Operations |-> "Pending"]

Replace ==
  /\ observer' \in Observers \ {observer}
  /\ UNCHANGED <<stage, owner, seen, outcome, mutated, returned>>

Start(op, result) ==
  /\ stage[op] = "Idle"
  /\ stage' = [stage EXCEPT ![op] = "Started"]
  /\ owner' = [owner EXCEPT ![op] = observer]
  /\ seen' = [seen EXCEPT ![op] = {observer}]
  /\ outcome' = [outcome EXCEPT ![op] = result]
  /\ UNCHANGED <<observer, mutated, returned>>

Introduce(op) ==
  /\ stage[op] = "Started"
  /\ stage' = [stage EXCEPT ![op] =
      IF outcome[op] = "IntroductionDenied" THEN "Decided" ELSE "Introduced"]
  /\ seen' = [seen EXCEPT ![op] = @ \cup {CallbackOwner(op)}]
  /\ UNCHANGED <<observer, owner, outcome, mutated, returned>>

Decide(op) ==
  /\ stage[op] = "Introduced"
  /\ stage' = [stage EXCEPT ![op] = "Decided"]
  /\ seen' = [seen EXCEPT ![op] = @ \cup {CallbackOwner(op)}]
  /\ UNCHANGED <<observer, owner, outcome, mutated, returned>>

Apply(op) ==
  /\ stage[op] = "Decided"
  /\ stage' = [stage EXCEPT ![op] = "Applied"]
  /\ mutated' = [mutated EXCEPT ![op] = outcome[op] \in {"Stored", "Matched"}]
  /\ UNCHANGED <<observer, owner, seen, outcome, returned>>

Complete(op, fail) ==
  /\ stage[op] = "Applied"
  /\ ~InfallibleCompletion \/ ~fail
  /\ stage' = [stage EXCEPT ![op] = "Finished"]
  /\ seen' = [seen EXCEPT ![op] = @ \cup {owner[op]}]
  /\ returned' = [returned EXCEPT ![op] = IF fail THEN "CompletionDenied" ELSE outcome[op]]
  /\ UNCHANGED <<observer, owner, outcome, mutated>>

Next == Replace \/ (\E op \in Operations :
    (\E result \in Outcomes : Start(op, result)) \/ Introduce(op) \/ Decide(op)
    \/ Apply(op) \/ (\E fail \in BOOLEAN : Complete(op, fail)))
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ observer \in Observers
  /\ stage \in [Operations -> Stages]
  /\ owner \in [Operations -> Observers]
  /\ seen \in [Operations -> SUBSET Observers]
  /\ outcome \in [Operations -> Outcomes]
  /\ mutated \in [Operations -> BOOLEAN]
  /\ returned \in [Operations -> Outcomes \cup {"Pending", "CompletionDenied"}]
SingleObserver == \A op \in Operations : seen[op] \subseteq {owner[op]}
DenialDoesNotMutate == \A op \in Operations :
  outcome[op] \in {"IntroductionDenied", "CommDenied"} => ~mutated[op]
CompletionPreservesResult == \A op \in Operations :
  stage[op] = "Finished" => returned[op] = outcome[op]
FinishedStateAgreement == \A op \in Operations : stage[op] = "Finished" =>
  (mutated[op] <=> returned[op] \in {"Stored", "Matched"})
=============================================================================
