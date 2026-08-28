-------------------- MODULE ParentFrontierCapacity --------------------
EXTENDS Naturals, FiniteSets

CONSTANT
  \* @type: Set(Str);
  ModelNodes,
  \* @type: Int;
  ConfiguredActiveMaximum,
  \* @type: Int;
  ParentCap,
  \* @type: Set(Str);
  ExactParents,
  \* @type: Bool;
  UseStaticMaximumGate

ASSUME /\ ModelNodes # {}
       /\ ConfiguredActiveMaximum \in Nat
       /\ ParentCap \in Nat \ {0}
       /\ ExactParents # {}
       /\ Cardinality(ExactParents) <= ConfiguredActiveMaximum + 1
       /\ UseStaticMaximumGate \in BOOLEAN

Nodes == ModelNodes

ExactFrontierFits == Cardinality(ExactParents) <= ParentCap
StaticWorstCaseFits == ConfiguredActiveMaximum + 1 <= ParentCap

VARIABLES
  \* @type: Set(Str);
  evaluated,
  \* @type: Set(Str);
  admitted,
  \* @type: Set(Str);
  deferred,
  \* @type: Str -> Set(Str);
  recordedParents

vars == <<evaluated, admitted, deferred, recordedParents>>

Init ==
  /\ evaluated = {}
  /\ admitted = {}
  /\ deferred = {}
  /\ recordedParents = [node \in Nodes |-> {}]

AdmissionDecision ==
  /\ ExactFrontierFits
  /\ (~UseStaticMaximumGate \/ StaticWorstCaseFits)

Evaluate(node) ==
  /\ node \notin evaluated
  /\ evaluated' = evaluated \union {node}
  /\ IF AdmissionDecision
       THEN /\ admitted' = admitted \union {node}
            /\ deferred' = deferred
            /\ recordedParents' = [recordedParents EXCEPT ![node] = ExactParents]
       ELSE /\ admitted' = admitted
            /\ deferred' = deferred \union {node}
            /\ recordedParents' = recordedParents

Idle ==
  /\ evaluated = Nodes
  /\ UNCHANGED vars

Next == (\E node \in Nodes : Evaluate(node)) \/ Idle

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A node \in Nodes : WF_vars(Evaluate(node))

TypeOK ==
  /\ evaluated \subseteq Nodes
  /\ admitted \subseteq Nodes
  /\ deferred \subseteq Nodes
  /\ recordedParents \in [Nodes -> SUBSET ExactParents]

Inv_DecisionPartition ==
  /\ admitted \subseteq evaluated
  /\ deferred \subseteq evaluated
  /\ admitted \cap deferred = {}
  /\ evaluated = admitted \union deferred

Inv_OverCapNeverSigns == ~ExactFrontierFits => admitted = {}

Inv_AdmissionPreservesExactFrontier ==
  \A node \in admitted : recordedParents[node] = ExactParents

Inv_DeferralPublishesNoParents ==
  \A node \in deferred : recordedParents[node] = {}

Inv_ExactFitIsAdmitted ==
  ExactFrontierFits => evaluated = admitted

Inv_OverCapIsDeferred ==
  ~ExactFrontierFits => evaluated = deferred

Live_AllNodesEvaluated == <>(evaluated = Nodes)

Live_AllNodesAdmitExactFit ==
  ExactFrontierFits => <>(admitted = Nodes)
=======================================================================
