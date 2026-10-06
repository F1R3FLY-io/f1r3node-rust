-------------------------- MODULE DeployTraceSegmentation --------------------------
EXTENDS Naturals, Sequences, TLC

CONSTANTS DeployOrder, EventsByDeploy, RetainCheckpointTrace

Deploys == {DeployOrder[index] : index \in 1..Len(DeployOrder)}

ASSUME /\ DeployOrder \in Seq(Nat)
       /\ Len(DeployOrder) > 1
       /\ EventsByDeploy \in [Deploys -> Seq(Nat)]
       /\ \A deploy \in Deploys : Len(EventsByDeploy[deploy]) > 0
       /\ RetainCheckpointTrace \in BOOLEAN

VARIABLES phase, cursor, activeTrace, checkpointSegments

vars == <<phase, cursor, activeTrace, checkpointSegments>>

Init ==
  /\ phase = "Execute"
  /\ cursor = 1
  /\ activeTrace = <<>>
  /\ checkpointSegments = <<>>

Execute ==
  /\ phase = "Execute"
  /\ cursor <= Len(DeployOrder)
  /\ activeTrace' = activeTrace \o EventsByDeploy[DeployOrder[cursor]]
  /\ phase' = "Checkpoint"
  /\ UNCHANGED <<cursor, checkpointSegments>>

Checkpoint ==
  /\ phase = "Checkpoint"
  /\ checkpointSegments' = Append(checkpointSegments, activeTrace)
  /\ activeTrace' = IF RetainCheckpointTrace THEN activeTrace ELSE <<>>
  /\ IF cursor = Len(DeployOrder)
        THEN /\ cursor' = cursor
             /\ phase' = "Done"
        ELSE /\ cursor' = cursor + 1
             /\ phase' = "Execute"

Next == Execute \/ Checkpoint

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(Execute)
  /\ WF_vars(Checkpoint)

TypeOK ==
  /\ phase \in {"Execute", "Checkpoint", "Done"}
  /\ cursor \in 1..Len(DeployOrder)
  /\ activeTrace \in Seq(Nat)
  /\ checkpointSegments \in Seq(Seq(Nat))
  /\ Len(checkpointSegments) <= Len(DeployOrder)

CheckpointContainsOnlyItsDeploy ==
  \A index \in 1..Len(checkpointSegments) :
    checkpointSegments[index] = EventsByDeploy[DeployOrder[index]]

CheckpointClearsActiveTrace ==
  phase \in {"Execute", "Done"} => activeTrace = <<>>

EventuallyAllDeploysCheckpointed ==
  <>(phase = "Done" /\ Len(checkpointSegments) = Len(DeployOrder))

=============================================================================
