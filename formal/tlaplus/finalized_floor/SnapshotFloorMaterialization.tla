-------------------- MODULE SnapshotFloorMaterialization --------------------
EXTENDS FiniteSets

CONSTANT
  \* @type: Bool;
  MaterializeLatest

ASSUME MaterializeLatest \in BOOLEAN

Blocks == {"G", "P", "A", "L", "F"}
Parents == {"P"}
LatestMessages == {"L"}
FinalizerBlocks == {"F"}
RequiredBlocks == Parents \union LatestMessages

ProvenanceClosure == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "P" -> {"G", "P"}
    [] block = "A" -> {"G", "A"}
    [] block = "L" -> {"G", "A", "L"}
    [] OTHER -> {"G", "F"}]

ClosureOf(blocks) == UNION {ProvenanceClosure[block] : block \in blocks}

SnapshotTargets == IF MaterializeLatest THEN RequiredBlocks ELSE Parents

VARIABLES
  \* @type: Set(Str);
  snapshotPending,
  \* @type: Set(Str);
  snapshotDone,
  \* @type: Set(Str);
  finalizerPending,
  \* @type: Set(Str);
  finalizerDone,
  \* @type: Set(Str);
  cache,
  \* @type: Str;
  phase

vars == <<snapshotPending, snapshotDone,
  finalizerPending, finalizerDone, cache, phase>>

Init ==
  /\ snapshotPending = SnapshotTargets
  /\ snapshotDone = {}
  /\ finalizerPending = FinalizerBlocks
  /\ finalizerDone = {}
  /\ cache = {}
  /\ phase = "materializing"

SnapshotStep ==
  \E block \in snapshotPending :
    /\ phase = "materializing"
    /\ snapshotPending' = snapshotPending \ {block}
    /\ snapshotDone' = snapshotDone \union {block}
    /\ cache' = cache \union ProvenanceClosure[block]
    /\ UNCHANGED <<finalizerPending, finalizerDone, phase>>

FinalizerStep ==
  \E block \in finalizerPending :
    /\ finalizerPending' = finalizerPending \ {block}
    /\ finalizerDone' = finalizerDone \union {block}
    /\ cache' = cache \union ProvenanceClosure[block]
    /\ UNCHANGED <<snapshotPending, snapshotDone, phase>>

Select ==
  /\ phase = "materializing"
  /\ snapshotPending = {}
  /\ phase' = "selected"
  /\ UNCHANGED <<snapshotPending, snapshotDone,
    finalizerPending, finalizerDone, cache>>

Done ==
  /\ phase = "selected"
  /\ UNCHANGED vars

Next == SnapshotStep \/ FinalizerStep \/ Select \/ Done

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(SnapshotStep)
  /\ WF_vars(Select)

TypeOK ==
  /\ snapshotPending \in SUBSET Blocks
  /\ snapshotDone \in SUBSET Blocks
  /\ finalizerPending \in SUBSET Blocks
  /\ finalizerDone \in SUBSET Blocks
  /\ cache \in SUBSET Blocks
  /\ phase \in {"materializing", "selected"}

Inv_PartitionsExact ==
  /\ snapshotPending \cap snapshotDone = {}
  /\ snapshotPending \union snapshotDone = SnapshotTargets
  /\ finalizerPending \cap finalizerDone = {}
  /\ finalizerPending \union finalizerDone = FinalizerBlocks

Inv_CacheIsExactUnion ==
  cache = ClosureOf(snapshotDone \union finalizerDone)

Inv_SelectedSnapshotHasCompleteProvenance ==
  phase = "selected" => ClosureOf(RequiredBlocks) \subseteq cache

Live_SnapshotSelects == <>(phase = "selected")
=============================================================================
