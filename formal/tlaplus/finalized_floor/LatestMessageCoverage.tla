-------------------- MODULE LatestMessageCoverage --------------------
EXTENDS FiniteSets, Naturals

CONSTANT
  \* @type: Bool;
  UseDescendingSchedule

ASSUME UseDescendingSchedule \in BOOLEAN

Validators == {"v1", "v2", "v3"}
Blocks == {"G", "F", "A", "B", "M1", "M2", "M3"}

Tip == [validator \in Validators |->
  CASE validator = "v1" -> "M1"
    [] validator = "v2" -> "M2"
    [] OTHER -> "M3"]

Parents == [block \in Blocks |->
  CASE block = "G" -> {}
    [] block = "F" -> {"G"}
    [] block = "A" -> {"G"}
    [] block = "B" -> {"G"}
    [] block = "M1" -> {"A", "F"}
    [] block = "M2" -> {"B", "F"}
    [] OTHER -> {"A", "F"}]

Height == [block \in Blocks |->
  CASE block = "G" -> 0
    [] block \in {"F", "A", "B"} -> 1
    [] OTHER -> 2]

DagPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "F" -> {"G", "F"}
    [] block = "A" -> {"G", "A"}
    [] block = "B" -> {"G", "B"}
    [] block = "M1" -> {"G", "F", "A", "M1"}
    [] block = "M2" -> {"G", "F", "B", "M2"}
    [] OTHER -> {"G", "F", "A", "M3"}]

ExpectedCoverage == [block \in Blocks |->
  {validator \in Validators : block \in DagPast[Tip[validator]]}]

SeedCoverage == [block \in Blocks |->
  {validator \in Validators : Tip[validator] = block}]

VARIABLES
  \* @type: Set(Str);
  pending,
  \* @type: Set(Str);
  processed,
  \* @type: Str -> Set(Str);
  coverage,
  \* @type: Bool;
  error

vars == <<pending, processed, coverage, error>>

Init ==
  /\ pending = {Tip[validator] : validator \in Validators}
  /\ processed = {}
  /\ coverage = SeedCoverage
  /\ error = FALSE

Scheduled(block) ==
  IF UseDescendingSchedule
  THEN \A other \in pending : Height[block] >= Height[other]
  ELSE TRUE

Process(block) ==
  /\ ~error
  /\ block \in pending
  /\ Scheduled(block)
  /\ LET inherited == coverage[block]
         parents == Parents[block]
         late == \E parent \in parents : parent \in processed
     IN /\ pending' = (pending \ {block}) \union (parents \ processed)
        /\ processed' = processed \union {block}
        /\ coverage' = [candidate \in Blocks |->
             IF candidate \in parents
             THEN coverage[candidate] \union inherited
             ELSE coverage[candidate]]
        /\ error' = (error \/ late)

Done ==
  /\ pending = {}
  /\ UNCHANGED vars

Next ==
  \/ \E block \in Blocks : Process(block)
  \/ Done

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(Next)

TypeOK ==
  /\ pending \in SUBSET Blocks
  /\ processed \in SUBSET Blocks
  /\ coverage \in [Blocks -> SUBSET Validators]
  /\ error \in BOOLEAN

Inv_StrictEdgeDescent ==
  \A child \in Blocks :
    \A parent \in Parents[child] : Height[parent] < Height[child]

Inv_CoverageSound ==
  \A block \in Blocks : coverage[block] \subseteq ExpectedCoverage[block]

Inv_NoLateCoverage == ~error

Inv_ProcessedCoverageExact ==
  \A block \in processed : coverage[block] = ExpectedCoverage[block]

Inv_CompleteCoverageExact ==
  pending = {} => coverage = ExpectedCoverage

Live_CoverageCompletes == <>(pending = {})
=============================================================================
