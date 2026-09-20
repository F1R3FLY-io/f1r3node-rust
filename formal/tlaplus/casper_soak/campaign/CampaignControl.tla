------------------------- MODULE CampaignControl -------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS BreakReservation, BreakApproval, BreakSchedule, BreakTermination
Slots == {"preflight", "amd64", "arm64"}
Controllers == {"first", "second"}
VARIABLES phase, owner, submissions, approval, schedule, terminated, cleanup,
          productFailure, retainedFailure
vars == <<phase, owner, submissions, approval, schedule, terminated, cleanup,
          productFailure, retainedFailure>>
Init ==
  /\ phase = [s \in Slots |-> "empty"]
  /\ owner = [s \in Slots |-> "none"]
  /\ submissions = [s \in Slots |-> 0]
  /\ approval = [s \in Slots |-> FALSE]
  /\ schedule = [s \in Slots |-> FALSE]
  /\ terminated = [s \in Slots |-> FALSE]
  /\ cleanup = [s \in Slots |-> FALSE]
  /\ productFailure = [s \in Slots |-> FALSE]
  /\ retainedFailure = [s \in Slots |-> FALSE]
Reserve(s, c, approved) ==
  /\ phase[s] = "empty"
  /\ approved \/ BreakApproval
  /\ s = "preflight" \/ phase["preflight"] = "done"
  /\ phase' = [phase EXCEPT ![s] = "reserved"]
  /\ owner' = [owner EXCEPT ![s] = c]
  /\ approval' = [approval EXCEPT ![s] = approved]
  /\ UNCHANGED <<submissions, schedule, terminated, cleanup,
                 productFailure, retainedFailure>>
Arm(s) ==
  /\ phase[s] = "reserved"
  /\ phase' = [phase EXCEPT ![s] = "armed"]
  /\ schedule' = [schedule EXCEPT ![s] = TRUE]
  /\ UNCHANGED <<owner, submissions, approval, terminated, cleanup,
                 productFailure, retainedFailure>>
Submit(s, c) ==
  /\ (phase[s] = "armed") \/ (BreakSchedule /\ phase[s] = "reserved")
       \/ (BreakReservation /\ phase[s] = "submitted")
  /\ owner[s] = c \/ BreakReservation
  /\ submissions[s] < 2
  /\ phase' = [phase EXCEPT ![s] = "submitted"]
  /\ submissions' = [submissions EXCEPT ![s] = @ + 1]
  /\ UNCHANGED <<owner, approval, schedule, terminated, cleanup,
                 productFailure, retainedFailure>>
LoseResponse(s) ==
  /\ phase[s] = "submitted"
  /\ phase' = [phase EXCEPT ![s] = "unknown"]
  /\ UNCHANGED <<owner, submissions, approval, schedule, terminated, cleanup,
                 productFailure, retainedFailure>>
Observe(s) ==
  /\ phase[s] \in {"submitted", "unknown"}
  /\ phase' = [phase EXCEPT ![s] = "running"]
  /\ UNCHANGED <<owner, submissions, approval, schedule, terminated, cleanup,
                 productFailure, retainedFailure>>
ProductFailure(s) ==
  /\ phase[s] = "running"
  /\ ~productFailure[s]
  /\ productFailure' = [productFailure EXCEPT ![s] = TRUE]
  /\ retainedFailure' = [retainedFailure EXCEPT ![s] = TRUE]
  /\ UNCHANGED <<phase, owner, submissions, approval, schedule, terminated, cleanup>>
Terminate(s, observed) ==
  /\ phase[s] \in {"submitted", "unknown", "running"}
  /\ observed \/ BreakTermination
  /\ phase' = [phase EXCEPT ![s] = "done"]
  /\ terminated' = [terminated EXCEPT ![s] = observed]
  /\ cleanup' = [cleanup EXCEPT ![s] = TRUE]
  /\ UNCHANGED <<owner, submissions, approval, schedule,
                 productFailure, retainedFailure>>
Next ==
  \/ \E s \in Slots, c \in Controllers, a \in BOOLEAN: Reserve(s,c,a)
  \/ \E s \in Slots: Arm(s)
  \/ \E s \in Slots, c \in Controllers: Submit(s,c)
  \/ \E s \in Slots: LoseResponse(s) \/ Observe(s) \/ ProductFailure(s)
  \/ \E s \in Slots, observed \in BOOLEAN: Terminate(s,observed)
Spec == Init /\ [][Next]_vars
OneSubmission == \A s \in Slots: submissions[s] <= 1
AuthorizedLaunch == \A s \in Slots: submissions[s] > 0 => approval[s]
ScheduledLaunch == \A s \in Slots: submissions[s] > 0 => schedule[s]
ConfirmedCleanup == \A s \in Slots: cleanup[s] => terminated[s]
PreservedFailure == \A s \in Slots: productFailure[s] => retainedFailure[s]
=============================================================================
