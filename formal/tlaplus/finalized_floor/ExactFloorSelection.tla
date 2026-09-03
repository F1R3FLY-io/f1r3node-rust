----------------------- MODULE ExactFloorSelection -----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
  \* @type: Bool;
  UseSignatureContainment,
  \* @type: Bool;
  AllowAnticipatoryFloor,
  \* @type: Bool;
  UseCausalWitnesses,
  \* @type: Bool;
  TreatMissingAsFalse,
  \* @type: Bool;
  ReuseStaleCache,
  \* @type: Bool;
  CheckAllSettledBases

ASSUME /\ UseSignatureContainment \in BOOLEAN
       /\ AllowAnticipatoryFloor \in BOOLEAN
       /\ UseCausalWitnesses \in BOOLEAN
       /\ TreatMissingAsFalse \in BOOLEAN
       /\ ReuseStaleCache \in BOOLEAN
       /\ CheckAllSettledBases \in BOOLEAN

Nodes == {"n1", "n2"}
Validators == {"v1", "v2", "v3"}
Blocks == {"G", "A", "X", "P", "J"}
Effects == {"A:0", "X:0"}
Signatures == {"deploy"}
Candidates == {"P", "J"}
SettledFloors == {"A", "X"}

StateParent == [block \in Blocks |->
  CASE block = "G" -> "G"
    [] block \in {"A", "X"} -> "G"
    [] block = "P" -> "A"
    [] OTHER -> "P"]

Own == [block \in Blocks |->
  CASE block = "A" -> {"A:0"}
    [] block = "X" -> {"X:0"}
    [] OTHER -> {}]

Applied == [block \in Blocks |->
  IF block = "J" THEN {"X:0"} ELSE {}]

State == [block \in Blocks |->
  CASE block = "G" -> {}
    [] block = "A" -> {"A:0"}
    [] block = "X" -> {"X:0"}
    [] block = "P" -> {"A:0"}
    [] OTHER -> {"A:0", "X:0"}]

EffectSignature == [effect \in Effects |-> "deploy"]
SignatureState(block) == {EffectSignature[effect] : effect \in State[block]}
ExactContains(required, candidate) == State[required] \subseteq State[candidate]
SignatureContains(required, candidate) ==
  SignatureState(required) \subseteq SignatureState(candidate)
Contains(required, candidate) ==
  IF UseSignatureContainment
  THEN SignatureContains(required, candidate)
  ELSE ExactContains(required, candidate)

DagPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "A" -> {"G", "A"}
    [] block = "X" -> {"G", "X"}
    [] block = "P" -> {"G", "A", "X", "P"}
    [] OTHER -> Blocks]

MainPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "A" -> {"G", "A"}
    [] block = "X" -> {"G", "X"}
    [] block = "P" -> {"G", "X", "P"}
    [] OTHER -> {"G", "X", "P", "J"}]

DagDescendant(required, candidate) == required \in DagPast[candidate]
MainDescendant(required, candidate) == required \in MainPast[candidate]
ExactSound(candidate) ==
  \A settled \in SettledFloors : ExactContains(settled, candidate)
ObservedSound(candidate) ==
  \A settled \in SettledFloors : Contains(settled, candidate)
AnticipatorySound(candidate) ==
  \A settled \in SettledFloors : DagDescendant(settled, candidate)
CandidateEligible(candidate) ==
  ObservedSound(candidate) \/
    (AllowAnticipatoryFloor /\ AnticipatorySound(candidate))

Tip == [validator \in Validators |->
  IF validator \in {"v1", "v2"} THEN "P" ELSE "J"]
CausalSupporting(candidate, delivered) ==
  {validator \in delivered : MainDescendant(candidate, Tip[validator])}
StateSupporting(candidate, delivered) ==
  {validator \in delivered :
    MainDescendant(candidate, Tip[validator]) /\
    ExactContains(candidate, Tip[validator])}
HasMajority(supporting) == Cardinality(supporting) >= 2
ObservedCertified(candidate, delivered) ==
  IF UseCausalWitnesses
  THEN HasMajority(CausalSupporting(candidate, delivered))
  ELSE HasMajority(StateSupporting(candidate, delivered))

AllFactsKnown(delivered) == Blocks \subseteq delivered
SelectionResult(delivered) ==
  IF ReuseStaleCache
  THEN "P"
  ELSE IF ~AllFactsKnown(delivered)
       THEN IF TreatMissingAsFalse THEN "reject" ELSE "defer"
       ELSE IF CandidateEligible("P") THEN "P" ELSE "J"

HistoricalFloor == "A"
MainBase == "P"
FallbackBase == "J"
MainBaseReady ==
  IF CheckAllSettledBases
  THEN ExactSound(MainBase)
  ELSE ExactContains(HistoricalFloor, MainBase)
FallbackReady == ExactSound(FallbackBase)
HistoricalBase == IF MainBaseReady THEN MainBase ELSE FallbackBase

VARIABLES
  \* @type: Str -> Set(Str);
  knownBlocks,
  \* @type: Str -> Set(Str);
  knownValidators,
  \* @type: Str -> Str;
  selection,
  \* @type: Str -> Str;
  decision,
  \* @type: Str -> Bool;
  certifiedX,
  \* @type: Str -> Bool;
  historicalBaseBuilt,
  \* @type: Str -> Str;
  historicalBaseUsed

vars == <<knownBlocks, knownValidators, selection, decision, certifiedX,
  historicalBaseBuilt, historicalBaseUsed>>

Init ==
  /\ knownBlocks = [node \in Nodes |-> {}]
  /\ knownValidators = [node \in Nodes |-> {}]
  /\ selection = [node \in Nodes |-> "none"]
  /\ decision = [node \in Nodes |-> "idle"]
  /\ certifiedX = [node \in Nodes |-> FALSE]
  /\ historicalBaseBuilt = [node \in Nodes |-> FALSE]
  /\ historicalBaseUsed = [node \in Nodes |-> "G"]

DeliverBlock(node, block) ==
  /\ block \notin knownBlocks[node]
  /\ knownBlocks' = [knownBlocks EXCEPT ![node] = @ \union {block}]
  /\ UNCHANGED <<knownValidators, selection, decision, certifiedX,
       historicalBaseBuilt, historicalBaseUsed>>

DeliverValidator(node, validator) ==
  /\ validator \notin knownValidators[node]
  /\ knownValidators' = [knownValidators EXCEPT ![node] = @ \union {validator}]
  /\ UNCHANGED <<knownBlocks, selection, decision, certifiedX,
       historicalBaseBuilt, historicalBaseUsed>>

AttemptSelection(node) ==
  /\ selection[node] = "none"
  /\ LET result == SelectionResult(knownBlocks[node]) IN
       /\ decision' = [decision EXCEPT ![node] = result]
       /\ selection' = [selection EXCEPT
            ![node] = IF result \in Candidates THEN result ELSE @]
  /\ UNCHANGED <<knownBlocks, knownValidators, certifiedX,
       historicalBaseBuilt, historicalBaseUsed>>

AttemptCertificate(node) ==
  /\ ~certifiedX[node]
  /\ ObservedCertified("X", knownValidators[node])
  /\ certifiedX' = [certifiedX EXCEPT ![node] = TRUE]
  /\ UNCHANGED <<knownBlocks, knownValidators, selection, decision,
       historicalBaseBuilt, historicalBaseUsed>>

BuildHistoricalBase(node) ==
  /\ ~historicalBaseBuilt[node]
  /\ MainBaseReady \/ FallbackReady
  /\ historicalBaseBuilt' = [historicalBaseBuilt EXCEPT ![node] = TRUE]
  /\ historicalBaseUsed' = [historicalBaseUsed EXCEPT ![node] = HistoricalBase]
  /\ UNCHANGED <<knownBlocks, knownValidators, selection, decision, certifiedX>>

Quiesce ==
  /\ \A node \in Nodes : selection[node] # "none"
  /\ UNCHANGED vars

Next ==
  \/ \E node \in Nodes, block \in Blocks : DeliverBlock(node, block)
  \/ \E node \in Nodes, validator \in Validators : DeliverValidator(node, validator)
  \/ \E node \in Nodes : AttemptSelection(node)
  \/ \E node \in Nodes : AttemptCertificate(node)
  \/ \E node \in Nodes : BuildHistoricalBase(node)
  \/ Quiesce

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A node \in Nodes, block \in Blocks : WF_vars(DeliverBlock(node, block))
  /\ \A node \in Nodes, validator \in Validators :
       WF_vars(DeliverValidator(node, validator))
  /\ \A node \in Nodes : WF_vars(AttemptSelection(node))

TypeOK ==
  /\ knownBlocks \in [Nodes -> SUBSET Blocks]
  /\ knownValidators \in [Nodes -> SUBSET Validators]
  /\ selection \in [Nodes -> Candidates \union {"none"}]
  /\ decision \in [Nodes -> Candidates \union {"idle", "defer", "reject"}]
  /\ certifiedX \in [Nodes -> BOOLEAN]
  /\ historicalBaseBuilt \in [Nodes -> BOOLEAN]
  /\ historicalBaseUsed \in [Nodes -> Blocks]

Inv_ExactStateParentRecurrence ==
  \A block \in Blocks \ {"G"} :
    State[block] = State[StateParent[block]] \union Applied[block] \union Own[block]

Inv_SignatureAliasingIsStrictlyWeaker ==
  /\ SignatureContains("X", "P")
  /\ ~ExactContains("X", "P")

Inv_StateSupportRefinesCausalSupport ==
  \A node \in Nodes, candidate \in Blocks :
    StateSupporting(candidate, knownValidators[node])
      \subseteq CausalSupporting(candidate, knownValidators[node])

Inv_SelectedFloorContainsEveryInheritedFloor ==
  \A node \in Nodes :
    selection[node] \in Candidates => ExactSound(selection[node])

Inv_MissingFactsDefer ==
  \A node \in Nodes :
    ~AllFactsKnown(knownBlocks[node]) => decision[node] # "reject"

Inv_CertificateUsesStateSupport ==
  \A node \in Nodes :
    certifiedX[node] => HasMajority(StateSupporting("X", knownValidators[node]))

Inv_UsedBaseContainsEverySettledFloor ==
  \A node \in Nodes :
    historicalBaseBuilt[node] => ExactSound(historicalBaseUsed[node])

Inv_ExactConsumersAgree ==
  \A node \in Nodes :
    selection[node] \in Candidates =>
      /\ ExactSound(selection[node])
      /\ \A settled \in SettledFloors :
           ExactContains(settled, selection[node])

Live_DeliveryOrderConverges ==
  <>(\A node \in Nodes : selection[node] = "J")
=============================================================================
