--------------------------- MODULE MCStateBoundAdmission ---------------------------
EXTENDS StateBoundAdmission

EventsDef == {"registry", "vault", "continuation"}
SchedulesDef == {
  <<"registry", "vault", "continuation">>,
  <<"continuation", "registry", "vault">>,
  <<"vault", "continuation", "registry">>
}
EventCostDef == [event \in EventsDef |-> 1]
AmbientEventCostDef ==
  [event \in EventsDef |-> IF event = "continuation" THEN 3 ELSE 1]

=============================================================================
