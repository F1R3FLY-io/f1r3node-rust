-------------------- MODULE NativeOperationEvidence --------------------
EXTENDS Naturals, FiniteSets

CONSTANT RequireDenial
Operations == {1, 2}
Decisions == {"Missing", "Granted", "Denied"}
Results == {"Pending", "Stored", "Matched", "Rejected"}

VARIABLES intro, comm, finished, result, exported
vars == <<intro, comm, finished, result, exported>>

Init ==
  /\ intro = [op \in Operations |-> "Missing"]
  /\ comm = [op \in Operations |-> "Missing"]
  /\ finished = {}
  /\ result = [op \in Operations |-> "Pending"]
  /\ exported = FALSE

Introduce(op, decision) ==
  /\ ~exported /\ op \notin finished /\ intro[op] = "Missing"
  /\ intro' = [intro EXCEPT ![op] = decision]
  /\ UNCHANGED <<comm, finished, result, exported>>

Communicate(op, decision) ==
  /\ ~exported /\ op \notin finished
  /\ intro[op] = "Granted" /\ comm[op] = "Missing"
  /\ comm' = [comm EXCEPT ![op] = decision]
  /\ UNCHANGED <<intro, finished, result, exported>>

Complete(op, completion) ==
  /\ ~exported /\ op \notin finished
  /\ finished' = finished \cup {op}
  /\ result' = [result EXCEPT ![op] = completion]
  /\ UNCHANGED <<intro, comm, exported>>

Valid(op) ==
  /\ intro[op] # "Missing"
  /\ CASE result[op] = "Stored" -> intro[op] = "Granted" /\ comm[op] = "Missing"
       [] result[op] = "Matched" -> intro[op] = "Granted" /\ comm[op] = "Granted"
       [] result[op] = "Rejected" -> intro[op] = "Denied" \/ comm[op] = "Denied"
            \/ (~RequireDenial /\ comm[op] = "Missing")
       [] OTHER -> FALSE

Export ==
  /\ ~exported /\ finished = Operations
  /\ \A op \in Operations : Valid(op)
  /\ exported' = TRUE
  /\ UNCHANGED <<intro, comm, finished, result>>

Next == Export \/ (\E op \in Operations :
  (\E decision \in {"Granted", "Denied"} :
    Introduce(op, decision) \/ Communicate(op, decision))
  \/ (\E completion \in Results \ {"Pending"} : Complete(op, completion)))

Spec == Init /\ [][Next]_vars
TypeOK ==
  /\ intro \in [Operations -> Decisions]
  /\ comm \in [Operations -> Decisions]
  /\ finished \subseteq Operations
  /\ result \in [Operations -> Results]
  /\ exported \in BOOLEAN
RejectionHasDenial == exported => \A op \in Operations :
  result[op] = "Rejected" => intro[op] = "Denied" \/ comm[op] = "Denied"
SuccessHasGrant == exported => \A op \in Operations :
  /\ (result[op] \in {"Stored", "Matched"} => intro[op] = "Granted")
  /\ (result[op] = "Matched" => comm[op] = "Granted")
CompleteCoverage == exported => finished = Operations
=============================================================================
