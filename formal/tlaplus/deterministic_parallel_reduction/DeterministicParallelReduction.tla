---------------- MODULE DeterministicParallelReduction ----------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS
    \* @type: Bool;
    WaitForCompleteFrontier,
    \* @type: Bool;
    CanonicalCommit,
    \* @type: Bool;
    CheckpointOnlyQuiescent,
    \* @type: Bool;
    PreserveDisjointParallelism,
    \* @type: Bool;
    IncludeAuthorityConflicts

B1 == "produce-1"
B2 == "produce-2"
B3 == "consume-x"
B4 == "produce-y"
B5 == "produce-z"
Branches == {B1, B2, B3, B4, B5}
ConflictComponentBranches == {B1, B2, B3, B4}
Channels == {"x", "y", "z"}
Values == {1, 2, 8, 9}

Kind(branch) == IF branch = B3 THEN "consume" ELSE "produce"
Channel(branch) ==
    CASE branch = B4 -> "y"
      [] branch = B5 -> "z"
      [] OTHER -> "x"
Value(branch) ==
    CASE branch = B1 -> 1
      [] branch = B2 -> 2
      [] branch = B4 -> 9
      [] branch = B5 -> 8
      [] OTHER -> 0
Order(branch) ==
    CASE branch = B1 -> 1
      [] branch = B2 -> 2
      [] branch = B3 -> 3
      [] branch = B4 -> 4
      [] OTHER -> 5

AuthorityFootprint(branch) ==
    CASE branch = B1 -> {"authority-a", "authority-b"}
      [] branch = B2 -> {"authority-b", "authority-c"}
      [] branch = B3 -> {"authority-b"}
      [] branch = B4 -> {"authority-b"}
      [] OTHER -> {"authority-d"}

ChannelFootprint(branch) == {Channel(branch)}

Footprint(branch) ==
    ChannelFootprint(branch) \union
      IF IncludeAuthorityConflicts THEN AuthorityFootprint(branch) ELSE {}

PhysicalConflict(left, right) ==
    ChannelFootprint(left) \intersect ChannelFootprint(right) /= {} \/
    AuthorityFootprint(left) \intersect AuthorityFootprint(right) /= {}

CanonicalFirst(branches) ==
    CHOOSE branch \in branches :
        \A other \in branches : Order(branch) <= Order(other)

Selected(branches, branch) ==
    IF CanonicalCommit
    THEN branch = CanonicalFirst(branches)
    ELSE branch \in branches

Minimum(values) ==
    CHOOSE value \in values : \A other \in values : value <= other

ApplyData(branch, currentData) ==
    IF Kind(branch) = "produce"
    THEN [currentData EXCEPT ![Channel(branch)] = @ \union {Value(branch)}]
    ELSE IF currentData[Channel(branch)] = {}
         THEN currentData
         ELSE [currentData EXCEPT
                 ![Channel(branch)] =
                    @ \ {Minimum(currentData[Channel(branch)])}]

ApplyOutput(branch, currentData, currentOutput) ==
    IF Kind(branch) = "consume" /\ currentData[Channel(branch)] /= {}
    THEN Minimum(currentData[Channel(branch)])
    ELSE currentOutput

VARIABLES
    \* @type: Set(Str);
    submitted,
    \* @type: Set(Str);
    pending,
    \* @type: Set(Str);
    committed,
    \* @type: Str -> Set(Int);
    data,
    \* @type: Int;
    output,
    \* @type: Set(Str);
    eventSet,
    \* @type: Bool;
    checkpointed,
    \* @type: Str -> Set(Int);
    checkpointData,
    \* @type: Set(Set(Str));
    parallelPairs

vars == <<
    submitted,
    pending,
    committed,
    data,
    output,
    eventSet,
    checkpointed,
    checkpointData,
    parallelPairs
>>

Init ==
    /\ submitted = {}
    /\ pending = {}
    /\ committed = {}
    /\ data = [channel \in Channels |-> {}]
    /\ output = 0
    /\ eventSet = {}
    /\ checkpointed = FALSE
    /\ checkpointData = [channel \in Channels |-> {}]
    /\ parallelPairs = {}

Submit(branch) ==
    /\ branch \in Branches \ submitted
    /\ submitted' = submitted \union {branch}
    /\ pending' = pending \union {branch}
    /\ UNCHANGED <<committed, data, output, eventSet,
                    checkpointed, checkpointData, parallelPairs>>

FrontierReady ==
    IF WaitForCompleteFrontier
    THEN submitted = Branches
    ELSE pending /= {}

HasDisjointPair ==
    \E left, right \in pending :
        left /= right /\ Footprint(left) \intersect Footprint(right) = {}

CommitOne(branch) ==
    /\ FrontierReady
    /\ pending /= {}
    /\ ~PreserveDisjointParallelism \/ ~HasDisjointPair
    /\ Selected(pending, branch)
    /\ data' = ApplyData(branch, data)
    /\ output' = ApplyOutput(branch, data, output)
    /\ pending' = pending \ {branch}
    /\ committed' = committed \union {branch}
    /\ eventSet' = eventSet \union {branch}
    /\ UNCHANGED <<submitted, checkpointed, checkpointData, parallelPairs>>

CommitPair(left, right) ==
    /\ PreserveDisjointParallelism
    /\ FrontierReady
    /\ left \in pending
    /\ right \in pending \ {left}
    /\ Selected(pending, left)
    /\ Footprint(left) \intersect Footprint(right) = {}
    /\ right = CanonicalFirst(
         {candidate \in pending \ {left} :
            Footprint(left) \intersect Footprint(candidate) = {}})
    /\ LET afterLeft == ApplyData(left, data)
           afterOutput == ApplyOutput(left, data, output)
       IN /\ data' = ApplyData(right, afterLeft)
          /\ output' = ApplyOutput(right, afterLeft, afterOutput)
    /\ pending' = pending \ {left, right}
    /\ committed' = committed \union {left, right}
    /\ eventSet' = eventSet \union {left, right}
    /\ parallelPairs' = parallelPairs \union {{left, right}}
    /\ UNCHANGED <<submitted, checkpointed, checkpointData>>

Done == committed = Branches

Checkpoint ==
    /\ ~checkpointed
    /\ IF CheckpointOnlyQuiescent THEN Done ELSE TRUE
    /\ checkpointed' = TRUE
    /\ checkpointData' = data
    /\ UNCHANGED <<submitted, pending, committed, data, output,
                    eventSet, parallelPairs>>

Next ==
    (\E branch \in Branches : Submit(branch))
    \/ (\E branch \in Branches : CommitOne(branch))
    \/ (\E left, right \in Branches : CommitPair(left, right))
    \/ Checkpoint

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ submitted \subseteq Branches
    /\ pending \subseteq Branches
    /\ committed \subseteq Branches
    /\ data \in [Channels -> SUBSET Values]
    /\ output \in Values \union {0}
    /\ eventSet \subseteq Branches
    /\ checkpointed \in BOOLEAN
    /\ checkpointData \in [Channels -> SUBSET Values]
    /\ parallelPairs \subseteq SUBSET Branches
    /\ \A pair \in parallelPairs : Cardinality(pair) = 2

Inv_ExactlyOnce ==
    /\ pending \intersect committed = {}
    /\ pending \union committed = submitted
    /\ eventSet = committed

Inv_CommitRequiresCompleteFrontier ==
    committed /= {} => submitted = Branches

Inv_ConflictComponentCommitsInOrder ==
    \A branch \in committed \intersect ConflictComponentBranches :
        \A earlier \in ConflictComponentBranches :
            Order(earlier) < Order(branch) => earlier \in committed

Inv_CanonicalTerminalState ==
    Done =>
        /\ data["x"] = {2}
        /\ data["y"] = {9}
        /\ data["z"] = {8}
        /\ output = 1

Inv_CheckpointAtQuiescence == checkpointed => Done

Inv_CheckpointIsCanonical ==
    checkpointed =>
        /\ checkpointData["x"] = {2}
        /\ checkpointData["y"] = {9}
        /\ checkpointData["z"] = {8}

Inv_DisjointWorkRemainsParallel ==
    Done => {B1, B5} \in parallelPairs

Inv_FirstCommitRetainsDisjointParallelism ==
    committed /= {} => parallelPairs /= {}

Inv_SharedAuthorityNeverRunsAsDisjoint ==
    ~\E pair \in parallelPairs :
        \E left, right \in pair :
            left /= right /\ PhysicalConflict(left, right)

=============================================================================
