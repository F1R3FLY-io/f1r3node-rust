-------------------------- MODULE ReplayRootMaterialization --------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
  \* @type: Set(Str);
  Nodes,
  \* @type: Str;
  Producer,
  \* @type: Int;
  DeployCount,
  \* @type: Bool;
  EagerSnapshots,
  \* @type: Bool;
  ReplaySpaceQuery

ASSUME /\ Nodes /= {}
       /\ Producer \in Nodes
       /\ DeployCount \in Nat \ {0}
       /\ EagerSnapshots \in BOOLEAN
       /\ ReplaySpaceQuery \in BOOLEAN

Roots == 0..DeployCount
PreRoots == [index \in 1..DeployCount |-> index - 1]
Terminal == {"Accepted", "Rejected"}

VARIABLES
  \* @type: Str -> Str;
  status,
  \* @type: Str -> Str;
  stage,
  \* @type: Str -> Int;
  cursor,
  \* @type: Str -> Set(Int);
  materialized,
  \* @type: Str -> Seq(Int);
  snapshotRoots,
  \* @type: Str -> Seq(Str);
  snapshotSources

vars == <<status, stage, cursor, materialized, snapshotRoots, snapshotSources>>

Init ==
  /\ status = [node \in Nodes |-> "Pending"]
  /\ stage = [node \in Nodes |-> "Snapshot"]
  /\ cursor = [node \in Nodes |-> 1]
  /\ materialized = [node \in Nodes |-> IF node = Producer THEN Roots ELSE {0}]
  /\ snapshotRoots = [node \in Nodes |-> <<>>]
  /\ snapshotSources = [node \in Nodes |-> <<>>]

ReadSnapshot(node) ==
  /\ ~EagerSnapshots
  /\ status[node] = "Pending"
  /\ stage[node] = "Snapshot"
  /\ cursor[node] <= DeployCount
  /\ PreRoots[cursor[node]] \in materialized[node]
  /\ stage' = [stage EXCEPT ![node] = "Replay"]
  /\ snapshotRoots' = [snapshotRoots EXCEPT
       ![node] = Append(@, PreRoots[cursor[node]])]
  /\ snapshotSources' = [snapshotSources EXCEPT
       ![node] = Append(@, IF ReplaySpaceQuery THEN "Replay" ELSE "Ordinary")]
  /\ UNCHANGED <<status, cursor, materialized>>

ReplayDeploy(node) ==
  /\ ~EagerSnapshots
  /\ status[node] = "Pending"
  /\ stage[node] = "Replay"
  /\ cursor[node] <= DeployCount
  /\ materialized' = [materialized EXCEPT ![node] = @ \cup {cursor[node]}]
  /\ IF cursor[node] = DeployCount
       THEN /\ status' = [status EXCEPT ![node] = "Accepted"]
            /\ stage' = [stage EXCEPT ![node] = "Done"]
            /\ cursor' = [cursor EXCEPT ![node] = @ + 1]
       ELSE /\ status' = status
            /\ stage' = [stage EXCEPT ![node] = "Snapshot"]
            /\ cursor' = [cursor EXCEPT ![node] = @ + 1]
  /\ UNCHANGED <<snapshotRoots, snapshotSources>>

EagerSnapshot(node) ==
  /\ EagerSnapshots
  /\ status[node] = "Pending"
  /\ stage[node] = "Snapshot"
  /\ cursor[node] <= DeployCount
  /\ snapshotRoots' = [snapshotRoots EXCEPT
       ![node] = Append(@, PreRoots[cursor[node]])]
  /\ snapshotSources' = [snapshotSources EXCEPT
       ![node] = Append(@, IF ReplaySpaceQuery THEN "Replay" ELSE "Ordinary")]
  /\ IF cursor[node] = DeployCount
       THEN /\ status' = [status EXCEPT
                    ![node] = IF {PreRoots[index] : index \in 1..DeployCount}
                                   \subseteq materialized[node]
                                 THEN "Accepted"
                                 ELSE "Rejected"]
            /\ stage' = [stage EXCEPT ![node] = "Done"]
            /\ cursor' = [cursor EXCEPT ![node] = @ + 1]
       ELSE /\ UNCHANGED <<status, stage>>
            /\ cursor' = [cursor EXCEPT ![node] = @ + 1]
  /\ UNCHANGED materialized

NodeStep(node) == ReadSnapshot(node) \/ ReplayDeploy(node) \/ EagerSnapshot(node)

Next == \E node \in Nodes : NodeStep(node)

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A node \in Nodes : WF_vars(NodeStep(node))

TypeOK ==
  /\ status \in [Nodes -> {"Pending", "Accepted", "Rejected"}]
  /\ stage \in [Nodes -> {"Snapshot", "Replay", "Done"}]
  /\ cursor \in [Nodes -> 1..(DeployCount + 1)]
  /\ materialized \in [Nodes -> SUBSET Roots]
  /\ \A node \in Nodes :
       /\ Len(snapshotRoots[node]) <= DeployCount
       /\ \A index \in 1..Len(snapshotRoots[node]) :
            snapshotRoots[node][index] \in Roots
  /\ \A node \in Nodes :
       /\ Len(snapshotSources[node]) <= DeployCount
       /\ \A index \in 1..Len(snapshotSources[node]) :
            snapshotSources[node][index] \in {"Ordinary", "Replay"}

SnapshotsFollowCanonicalChain ==
  \A node \in Nodes :
    /\ Len(snapshotRoots[node]) <= DeployCount
    /\ \A index \in 1..Len(snapshotRoots[node]) :
         snapshotRoots[node][index] = PreRoots[index]

SnapshotReadsMaterializedRoot ==
  \A node \in Nodes :
    \A index \in 1..Len(snapshotRoots[node]) :
      snapshotRoots[node][index] \in materialized[node]

SnapshotsUseOrdinaryRuntime ==
  \A node \in Nodes :
    \A index \in 1..Len(snapshotSources[node]) :
      snapshotSources[node][index] = "Ordinary"

CursorMatchesReplayPrefix ==
  \A node \in Nodes :
    CASE stage[node] = "Snapshot" -> cursor[node] = Len(snapshotRoots[node]) + 1
      [] stage[node] = "Replay" -> cursor[node] = Len(snapshotRoots[node])
      [] OTHER -> /\ cursor[node] = DeployCount + 1
                  /\ Len(snapshotRoots[node]) = DeployCount

AcceptedMaterializedPostState ==
  \A node \in Nodes : status[node] = "Accepted" => materialized[node] = Roots

CompletedValidatorsAgree ==
  \A left \in Nodes :
    \A right \in Nodes :
      /\ status[left] \in Terminal
      /\ status[right] \in Terminal
      => status[left] = status[right]

EventuallyAllComplete == <> (\A node \in Nodes : status[node] \in Terminal)

=============================================================================
