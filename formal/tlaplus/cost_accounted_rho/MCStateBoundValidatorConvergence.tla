-------------------- MODULE MCStateBoundValidatorConvergence --------------------
EXTENDS StateBoundValidatorConvergence

ValidatorsDef == {"boot", "validator1", "validator2"}
EventsDef == {"submitted", "resident", "branch"}
SchedulesDef == {
  <<"submitted", "resident">>,
  <<"resident", "submitted">>,
  <<"submitted", "branch">>
}
EventCostDef ==
  [event \in EventsDef |->
    IF event = "resident" THEN 3 ELSE IF event = "branch" THEN 5 ELSE 1]
RootChoicesDef == {"merged-root", "stale-root"}
ContextChoicesDef == {"block-context", "other-context"}
DeployOrdersDef == {
  <<"deploy-a", "deploy-b">>,
  <<"deploy-b", "deploy-a">>
}
CorrectRootDef == "merged-root"
CorrectContextDef == "block-context"
CanonicalDeployOrderDef == <<"deploy-a", "deploy-b">>
CanonicalDeployOrdersOnlyDef == {CanonicalDeployOrderDef}
CertifiedScheduleDef == <<"submitted", "resident">>

=============================================================================
