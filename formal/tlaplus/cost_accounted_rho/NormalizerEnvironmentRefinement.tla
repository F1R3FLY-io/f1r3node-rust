---------------- MODULE NormalizerEnvironmentRefinement ----------------
EXTENDS Naturals, TLC

CONSTANTS Environments, AuthenticatedEnvironment, EmptyEnvironment,
          ProgramNeedsDeployerIdentity, UseEmptyAdmissionEnvironment

ASSUME /\ Environments # {}
       /\ AuthenticatedEnvironment \in Environments
       /\ EmptyEnvironment \in Environments
       /\ AuthenticatedEnvironment # EmptyEnvironment
       /\ ProgramNeedsDeployerIdentity \in BOOLEAN
       /\ UseEmptyAdmissionEnvironment \in BOOLEAN

VARIABLES phase, admissionEnvironment, executionEnvironment,
          replayEnvironment, admitted, executed, replayed

vars == <<phase, admissionEnvironment, executionEnvironment,
          replayEnvironment, admitted, executed, replayed>>

Init ==
    /\ phase = "Certify"
    /\ admissionEnvironment =
         IF UseEmptyAdmissionEnvironment
         THEN EmptyEnvironment
         ELSE AuthenticatedEnvironment
    /\ executionEnvironment = AuthenticatedEnvironment
    /\ replayEnvironment = AuthenticatedEnvironment
    /\ admitted = FALSE
    /\ executed = FALSE
    /\ replayed = FALSE

Certify ==
    /\ phase = "Certify"
    /\ admitted' =
         IF ProgramNeedsDeployerIdentity
         THEN admissionEnvironment = AuthenticatedEnvironment
         ELSE TRUE
    /\ phase' = "Execute"
    /\ UNCHANGED <<admissionEnvironment, executionEnvironment,
                    replayEnvironment, executed, replayed>>

Execute ==
    /\ phase = "Execute"
    /\ executed' = admitted /\ executionEnvironment = AuthenticatedEnvironment
    /\ phase' = "Replay"
    /\ UNCHANGED <<admissionEnvironment, executionEnvironment,
                    replayEnvironment, admitted, replayed>>

Replay ==
    /\ phase = "Replay"
    /\ replayed' = executed /\ replayEnvironment = executionEnvironment
    /\ phase' = "Done"
    /\ UNCHANGED <<admissionEnvironment, executionEnvironment,
                    replayEnvironment, admitted, executed>>

Next == Certify \/ Execute \/ Replay

Spec == /\ Init
        /\ [][Next]_vars
        /\ WF_vars(Certify)
        /\ WF_vars(Execute)
        /\ WF_vars(Replay)

TypeOK ==
    /\ phase \in {"Certify", "Execute", "Replay", "Done"}
    /\ admissionEnvironment \in Environments
    /\ executionEnvironment \in Environments
    /\ replayEnvironment \in Environments
    /\ admitted \in BOOLEAN
    /\ executed \in BOOLEAN
    /\ replayed \in BOOLEAN

CertificationExecutionReplayUseSameEnvironment ==
    /\ admissionEnvironment = executionEnvironment
    /\ executionEnvironment = replayEnvironment

AuthenticatedProgramIsAdmitted ==
    phase # "Certify" => admitted

ExecutionRequiresAdmission == executed => admitted

ReplayMatchesExecution == phase = "Done" => replayed = executed

EventuallyReplayCompletes == <> (phase = "Done")

=============================================================================
