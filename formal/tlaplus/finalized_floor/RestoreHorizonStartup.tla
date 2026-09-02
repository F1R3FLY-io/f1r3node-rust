-------------------------- MODULE RestoreHorizonStartup --------------------------
EXTENDS Integers, FiniteSets, TLC

CONSTANT
  \* @type: Str;
  Defect

ASSUME Defect \in {
  "None",
  "SkipReconcile",
  "EnterRunningEarly",
  "SelectStaleSequence",
  "DropGenesisSupport",
  "HeldnessProposal"
}

Nodes == {"Full", "Restored"}
Validators == {"Live", "Silent"}
Blocks == {"G", "F", "L0", "L1", "X"}
CanonicalGenesis == "G"
AuthorityFloor == "F"

\* @type: Str => Int;
ActiveGeneration(validator) == IF validator = "Live" THEN 1 ELSE 0
\* @type: Str => Int;
Generation(block) == IF block = "L0" THEN 0 ELSE IF block = "L1" THEN 1 ELSE 0
\* @type: Str => Int;
SequenceNumber(block) == IF block = "L0" THEN 4 ELSE IF block = "L1" THEN 5 ELSE 0

\* @type: Str => Str;
ExpectedLatest(validator) == IF validator = "Live" THEN "L1" ELSE CanonicalGenesis

\* @type: Str => (Str -> Str);
RawFor(node) ==
  IF node = "Full"
  THEN [validator \in Validators |-> ExpectedLatest(validator)]
  ELSE [validator \in Validators |-> IF validator = "Live" THEN "L0" ELSE "X"]

\* @type: Str => (Str -> Str);
ReconciledFor(node) ==
  [validator \in Validators |->
    IF Defect = "SelectStaleSequence" /\ validator = "Live"
    THEN "L0"
    ELSE ExpectedLatest(validator)]

\* @type: (Str -> Str) => Set(Str);
MapRange(map) == {map[validator] : validator \in DOMAIN map}

\* @type: Str -> Str;
EmptyMap == [validator \in {} |-> CanonicalGenesis]

VARIABLES
  \* @type: Str -> Set(Str);
  held,
  \* @type: Str -> Str;
  phase,
  \* @type: Str -> (Str -> Str);
  rawLatest,
  \* @type: Str -> (Str -> Str);
  latest,
  \* @type: Str -> (Str -> Str);
  exact,
  \* @type: Str -> Set(Str);
  support,
  \* @type: Str -> (Str -> Bool);
  firstProposal

vars == <<held, phase, rawLatest, latest, exact, support, firstProposal>>

\* @type: (Str, Str -> Str) => Bool;
SlotsReady(node, slots) ==
  \A validator \in Validators :
    slots[validator] = CanonicalGenesis \/ slots[validator] \in held[node]

Init ==
  /\ held = [node \in Nodes |->
       IF node = "Full" THEN {"G", "F", "L0", "L1"} ELSE {"F", "L0", "L1"}]
  /\ phase = [node \in Nodes |-> "Raw"]
  /\ rawLatest = [node \in Nodes |-> RawFor(node)]
  /\ latest = [node \in Nodes |-> EmptyMap]
  /\ exact = [node \in Nodes |-> EmptyMap]
  /\ support = [node \in Nodes |-> {}]
  /\ firstProposal = [node \in Nodes |-> [validator \in Validators |-> FALSE]]

\* @type: Str => Bool;
Reconcile(node) ==
  /\ phase[node] = "Raw"
  /\ latest' = [latest EXCEPT
       ![node] = IF Defect = "SkipReconcile" THEN rawLatest[node] ELSE ReconciledFor(node)]
  /\ phase' = [phase EXCEPT ![node] = "Reconciled"]
  /\ UNCHANGED <<held, rawLatest, exact, support, firstProposal>>

\* @type: Str => Bool;
EnterRunning(node) ==
  /\ phase[node] = "Reconciled"
  /\ (SlotsReady(node, latest[node]) \/ Defect = "EnterRunningEarly")
  /\ phase' = [phase EXCEPT ![node] = "Running"]
  /\ UNCHANGED <<held, rawLatest, latest, exact, support, firstProposal>>

\* @type: (Str, Str) => Bool;
ProposalFor(node, validator) ==
  IF latest[node][validator] = CanonicalGenesis
  THEN IF Defect = "HeldnessProposal"
       THEN CanonicalGenesis \in held[node]
       ELSE TRUE
  ELSE latest[node][validator] \in held[node] /\
       Generation(latest[node][validator]) = ActiveGeneration(validator)

\* @type: Str => Bool;
Capture(node) ==
  /\ phase[node] = "Running"
  /\ exact' = [exact EXCEPT ![node] = latest[node]]
  /\ support' = [support EXCEPT
       ![node] = IF Defect = "DropGenesisSupport" /\ CanonicalGenesis \notin held[node]
                 THEN MapRange(latest[node]) \ {CanonicalGenesis}
                 ELSE MapRange(latest[node])]
  /\ firstProposal' = [firstProposal EXCEPT
       ![node] = [validator \in Validators |-> ProposalFor(node, validator)]]
  /\ phase' = [phase EXCEPT ![node] = "Captured"]
  /\ UNCHANGED <<held, rawLatest, latest>>

Idle ==
  /\ \A node \in Nodes : phase[node] = "Captured"
  /\ UNCHANGED vars

Next ==
  \/ \E node \in Nodes : Reconcile(node)
  \/ \E node \in Nodes : EnterRunning(node)
  \/ \E node \in Nodes : Capture(node)
  \/ Idle

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ held \in [Nodes -> SUBSET Blocks]
  /\ phase \in [Nodes -> {"Raw", "Reconciled", "Running", "Captured"}]
  /\ rawLatest \in [Nodes -> [Validators -> Blocks]]
  /\ \A node \in Nodes :
       /\ DOMAIN latest[node] \subseteq Validators
       /\ \A validator \in DOMAIN latest[node] : latest[node][validator] \in Blocks
  /\ \A node \in Nodes :
       /\ DOMAIN exact[node] \subseteq Validators
       /\ \A validator \in DOMAIN exact[node] : exact[node][validator] \in Blocks
  /\ support \in [Nodes -> SUBSET Blocks]
  /\ firstProposal \in [Nodes -> [Validators -> BOOLEAN]]

ReconciliationEliminatesStale ==
  \A node \in Nodes : phase[node] /= "Raw" => latest[node] = ReconciledFor(node)

RunningSlotsMaterialized ==
  \A node \in Nodes : phase[node] \in {"Running", "Captured"} =>
    SlotsReady(node, latest[node])

MonotonicIncarnationSequence ==
  \A node \in Nodes : phase[node] \in {"Reconciled", "Running", "Captured"} =>
    latest[node]["Live"] = CanonicalGenesis \/ SequenceNumber(latest[node]["Live"]) > 4

ExactSlotsComplete ==
  \A node \in Nodes : phase[node] = "Captured" => DOMAIN exact[node] = Validators

CanonicalSupportRetained ==
  \A node \in Nodes : phase[node] = "Captured" => CanonicalGenesis \in support[node]

CapturedContextsAgree ==
  phase["Full"] = "Captured" /\ phase["Restored"] = "Captured" =>
    /\ exact["Full"] = exact["Restored"]
    /\ support["Full"] = support["Restored"]
    /\ firstProposal["Full"] = firstProposal["Restored"]
    /\ firstProposal["Restored"]["Silent"]

Safety ==
  /\ TypeOK
  /\ ReconciliationEliminatesStale
  /\ RunningSlotsMaterialized
  /\ MonotonicIncarnationSequence
  /\ ExactSlotsComplete
  /\ CanonicalSupportRetained
  /\ CapturedContextsAgree

=============================================================================
