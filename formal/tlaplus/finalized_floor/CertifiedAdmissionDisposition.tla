---------------------- MODULE CertifiedAdmissionDisposition ----------------------
EXTENDS FiniteSets, Naturals

CONSTANT
  \* @type: Str;
  Defect

ASSUME Defect \in {"None", "SummaryBeforeAuthority", "HashAttributable", "LocalFaultObjective"}

Nodes == {1, 2, 3}
Candidates == {"Honest", "Objective", "Mutated", "Faulting"}
Statuses == {"Pending", "Accepted", "ObjectiveRejected", "UnattributableRejected", "LocalFault"}
Outcomes == {"None", "Accepted", "Rejected"}
TerminalStatuses == {"Accepted", "ObjectiveRejected", "UnattributableRejected"}

VARIABLES
  \* @type: Int -> Set(Str);
  known,
  \* @type: Int -> (Str -> Str);
  status,
  \* @type: Int -> (Str -> Bool);
  authorityCertified,
  \* @type: Int -> (Str -> Str);
  admissionOutcome,
  \* @type: Int -> Set(Str);
  dag,
  \* @type: Int -> Set(Str);
  evidence,
  \* @type: Int -> Bool;
  localFaultActive

vars == <<known, status, authorityCertified, admissionOutcome,
          dag, evidence, localFaultActive>>

Init ==
  /\ known = [node \in Nodes |-> {}]
  /\ status = [node \in Nodes |-> [candidate \in Candidates |-> "Pending"]]
  /\ authorityCertified = [node \in Nodes |-> [candidate \in Candidates |-> FALSE]]
  /\ admissionOutcome = [node \in Nodes |-> [candidate \in Candidates |-> "None"]]
  /\ dag = [node \in Nodes |-> {}]
  /\ evidence = [node \in Nodes |-> {}]
  /\ localFaultActive = [node \in Nodes |-> node \in {1, 3}]

Receive(node, candidate) ==
  /\ candidate \notin known[node]
  /\ known' = [known EXCEPT ![node] = @ \union {candidate}]
  /\ UNCHANGED <<status, authorityCertified, admissionOutcome,
                  dag, evidence, localFaultActive>>

RecordAccepted(node, candidate) ==
  /\ status' = [status EXCEPT ![node][candidate] = "Accepted"]
  /\ authorityCertified' = [authorityCertified EXCEPT ![node][candidate] = TRUE]
  /\ admissionOutcome' = [admissionOutcome EXCEPT ![node][candidate] = "Accepted"]
  /\ dag' = [dag EXCEPT ![node] = @ \union {candidate}]
  /\ UNCHANGED evidence

RecordObjective(node, candidate) ==
  /\ status' = [status EXCEPT ![node][candidate] = "ObjectiveRejected"]
  /\ authorityCertified' = [authorityCertified EXCEPT ![node][candidate] = TRUE]
  /\ admissionOutcome' = [admissionOutcome EXCEPT ![node][candidate] = "Rejected"]
  /\ dag' = [dag EXCEPT ![node] = @ \union {candidate}]
  /\ evidence' = [evidence EXCEPT ![node] = @ \union {candidate}]

RecordUnattributable(node, candidate) ==
  /\ status' = [status EXCEPT ![node][candidate] = "UnattributableRejected"]
  /\ authorityCertified' = [authorityCertified EXCEPT ![node][candidate] = FALSE]
  /\ admissionOutcome' = [admissionOutcome EXCEPT ![node][candidate] = "None"]
  /\ UNCHANGED <<dag, evidence>>

RecordLocalFault(node, candidate) ==
  /\ status' = [status EXCEPT ![node][candidate] = "LocalFault"]
  /\ authorityCertified' = [authorityCertified EXCEPT ![node][candidate] = FALSE]
  /\ admissionOutcome' = [admissionOutcome EXCEPT ![node][candidate] = "None"]
  /\ UNCHANGED <<dag, evidence>>

Validate(node, candidate) ==
  /\ candidate \in known[node]
  /\ status[node][candidate] = "Pending"
  /\ CASE candidate = "Mutated" ->
            IF Defect = "HashAttributable"
            THEN RecordObjective(node, candidate)
            ELSE RecordUnattributable(node, candidate)
       [] candidate = "Objective" ->
            IF Defect = "SummaryBeforeAuthority"
            THEN RecordUnattributable(node, candidate)
            ELSE RecordObjective(node, candidate)
       [] candidate = "Faulting" /\ localFaultActive[node] ->
            IF Defect = "LocalFaultObjective"
            THEN RecordObjective(node, candidate)
            ELSE RecordLocalFault(node, candidate)
       [] OTHER -> RecordAccepted(node, candidate)
  /\ UNCHANGED <<known, localFaultActive>>

ClearLocalFault(node) ==
  /\ localFaultActive[node]
  /\ localFaultActive' = [localFaultActive EXCEPT ![node] = FALSE]
  /\ UNCHANGED <<known, status, authorityCertified,
                  admissionOutcome, dag, evidence>>

RetryLocalFault(node) ==
  /\ status[node]["Faulting"] = "LocalFault"
  /\ ~localFaultActive[node]
  /\ status' = [status EXCEPT ![node]["Faulting"] = "Pending"]
  /\ UNCHANGED <<known, authorityCertified, admissionOutcome,
                  dag, evidence, localFaultActive>>

Next ==
  \/ \E node \in Nodes, candidate \in Candidates : Receive(node, candidate)
  \/ \E node \in Nodes, candidate \in Candidates : Validate(node, candidate)
  \/ \E node \in Nodes : ClearLocalFault(node)
  \/ \E node \in Nodes : RetryLocalFault(node)

TypeOK ==
  /\ known \in [Nodes -> SUBSET Candidates]
  /\ status \in [Nodes -> [Candidates -> Statuses]]
  /\ authorityCertified \in [Nodes -> [Candidates -> BOOLEAN]]
  /\ admissionOutcome \in [Nodes -> [Candidates -> Outcomes]]
  /\ dag \in [Nodes -> SUBSET Candidates]
  /\ evidence \in [Nodes -> SUBSET Candidates]
  /\ localFaultActive \in [Nodes -> BOOLEAN]

Inv_TypedOutcomeShape ==
  \A node \in Nodes, candidate \in Candidates :
    CASE status[node][candidate] = "Accepted" ->
           authorityCertified[node][candidate]
             /\ admissionOutcome[node][candidate] = "Accepted"
       [] status[node][candidate] = "ObjectiveRejected" ->
           authorityCertified[node][candidate]
             /\ admissionOutcome[node][candidate] = "Rejected"
       [] OTHER ->
           ~authorityCertified[node][candidate]
             /\ admissionOutcome[node][candidate] = "None"

Inv_HashMismatchUnattributable ==
  \A node \in Nodes :
    status[node]["Mutated"] \in TerminalStatuses
      => /\ status[node]["Mutated"] = "UnattributableRejected"
         /\ "Mutated" \notin dag[node]
         /\ "Mutated" \notin evidence[node]

Inv_AuthenticatedObjectiveCertified ==
  \A node \in Nodes :
    status[node]["Objective"] \in TerminalStatuses
      => /\ status[node]["Objective"] = "ObjectiveRejected"
         /\ authorityCertified[node]["Objective"]
         /\ admissionOutcome[node]["Objective"] = "Rejected"

Inv_EvidenceRequiresAttributableObjective ==
  \A node \in Nodes : evidence[node] \subseteq {"Objective"}

Inv_LocalFaultHasNoDurableEffects ==
  \A node \in Nodes :
    localFaultActive[node]
      => /\ "Faulting" \notin dag[node]
         /\ "Faulting" \notin evidence[node]
         /\ ~authorityCertified[node]["Faulting"]

Inv_TerminalAgreement ==
  \A first \in Nodes, second \in Nodes, candidate \in Candidates :
    status[first][candidate] \in TerminalStatuses
      /\ status[second][candidate] \in TerminalStatuses
      => status[first][candidate] = status[second][candidate]

Safety ==
  /\ TypeOK
  /\ Inv_TypedOutcomeShape
  /\ Inv_HashMismatchUnattributable
  /\ Inv_AuthenticatedObjectiveCertified
  /\ Inv_EvidenceRequiresAttributableObjective
  /\ Inv_LocalFaultHasNoDurableEffects
  /\ Inv_TerminalAgreement

AllResolved ==
  \A node \in Nodes, candidate \in Candidates :
    status[node][candidate] \in TerminalStatuses

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A node \in Nodes, candidate \in Candidates :
       WF_vars(Receive(node, candidate))
         /\ WF_vars(Validate(node, candidate))
  /\ \A node \in Nodes :
       WF_vars(ClearLocalFault(node))
         /\ WF_vars(RetryLocalFault(node))

Live_AllResolved == <>AllResolved

=============================================================================
