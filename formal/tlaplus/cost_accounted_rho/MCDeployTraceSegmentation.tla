-------------------------- MODULE MCDeployTraceSegmentation --------------------------
EXTENDS DeployTraceSegmentation

DeployOrderDef == <<1, 2, 3>>
EventsByDeployDef == [deploy \in {1, 2, 3} |-> <<deploy, deploy + 10>>]

=============================================================================
