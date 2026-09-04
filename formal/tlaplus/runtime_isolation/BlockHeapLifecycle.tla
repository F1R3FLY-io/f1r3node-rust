------------------------- MODULE BlockHeapLifecycle -------------------------
EXTENDS FiniteSets, Naturals, Sequences

CONSTANT
    \* @type: Str;
    Defect,
    \* @type: Int;
    BlockCap,
    \* @type: Int;
    TrimInterval,
    \* @type: Int;
    MaxBlocks

ASSUME
    /\ Defect \in {"None", "MissingBoundaryReclamation"}
    /\ BlockCap \in Nat \ {0}
    /\ TrimInterval \in Nat \ {0}
    /\ MaxBlocks \in Nat \ {0}

Slots == {"Left", "Right"}
Phases == {"Idle", "Running"}

VARIABLES
    \* @type: Int;
    started,
    \* @type: Int;
    completed,
    \* @type: (Str -> Str);
    phase,
    \* @type: (Str -> Int);
    live,
    \* @type: Int;
    retained,
    \* @type: Int;
    completionsSinceTrim,
    \* @type: Seq(Str);
    committed,
    \* @type: Seq(Str);
    semanticReference

vars ==
    <<started, completed, phase, live, retained,
      completionsSinceTrim, committed, semanticReference>>

Init ==
    /\ started = 0
    /\ completed = 0
    /\ phase = [slot \in Slots |-> "Idle"]
    /\ live = [slot \in Slots |-> 0]
    /\ retained = 0
    /\ completionsSinceTrim = 0
    /\ committed = <<>>
    /\ semanticReference = <<>>

Start(slot) ==
    /\ slot \in Slots
    /\ phase[slot] = "Idle"
    /\ started < MaxBlocks
    /\ started' = started + 1
    /\ phase' = [phase EXCEPT ![slot] = "Running"]
    /\ live' = [live EXCEPT ![slot] = 1]
    /\ UNCHANGED
        <<completed, retained, completionsSinceTrim,
          committed, semanticReference>>

Allocate(slot) ==
    /\ slot \in Slots
    /\ phase[slot] = "Running"
    /\ live[slot] < BlockCap
    /\ live' = [live EXCEPT ![slot] = @ + 1]
    /\ UNCHANGED
        <<started, completed, phase, retained,
          completionsSinceTrim, committed, semanticReference>>

Complete(slot) ==
    /\ slot \in Slots
    /\ phase[slot] = "Running"
    /\ LET boundary == completionsSinceTrim = TrimInterval - 1
           reclaim == /\ Defect = "None"
                      /\ boundary
       IN /\ started' = started
          /\ completed' = completed + 1
          /\ phase' = [phase EXCEPT ![slot] = "Idle"]
          /\ live' = [live EXCEPT ![slot] = 0]
          /\ retained' = IF reclaim THEN 0 ELSE retained + live[slot]
          /\ completionsSinceTrim' =
                IF boundary THEN 0 ELSE completionsSinceTrim + 1
          /\ committed' = Append(committed, slot)
          /\ semanticReference' = Append(semanticReference, slot)

CompleteRun ==
    /\ started = MaxBlocks
    /\ \A slot \in Slots : phase[slot] = "Idle"

TerminalStutter ==
    /\ CompleteRun
    /\ UNCHANGED vars

Next ==
    \/ \E slot \in Slots : Start(slot)
    \/ \E slot \in Slots : Allocate(slot)
    \/ \E slot \in Slots : Complete(slot)
    \/ TerminalStutter

Resident == retained + live["Left"] + live["Right"]

TypeOK ==
    /\ started \in 0..MaxBlocks
    /\ completed \in 0..MaxBlocks
    /\ completed <= started
    /\ phase \in [Slots -> Phases]
    /\ live \in [Slots -> 0..BlockCap]
    /\ retained \in 0..(MaxBlocks * BlockCap)
    /\ completionsSinceTrim \in 0..(TrimInterval - 1)
    /\ Len(committed) <= MaxBlocks
    /\ Len(semanticReference) <= MaxBlocks
    /\ \A index \in 1..Len(committed) : committed[index] \in Slots
    /\ \A index \in 1..Len(semanticReference) : semanticReference[index] \in Slots

IdleSlotsOwnNoLiveHeap ==
    \A slot \in Slots : phase[slot] = "Idle" => live[slot] = 0

CompletedBlocksMatchHistory ==
    /\ completed = Len(committed)
    /\ started = completed + Cardinality({slot \in Slots : phase[slot] = "Running"})

ReclamationIsSemanticallyInvisible ==
    committed = semanticReference

ResidentWithinIntervalEnvelope ==
    Resident <= (Cardinality(Slots) + TrimInterval - 1) * BlockCap

=============================================================================
