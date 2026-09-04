--------------------- MODULE StartupMetadataPreflight ---------------------
EXTENDS FiniteSets, Naturals, Sequences

CONSTANTS VerifyBeforePublish, SupervisorEnabled

DirectMatching == "direct-matching"
DirectMismatch == "direct-mismatch"
AsyncMatching == "async-matching"
AsyncMismatch == "async-mismatch"
Nodes == {DirectMatching, DirectMismatch, AsyncMatching, AsyncMismatch}
DirectNodes == {DirectMatching, DirectMismatch}
AsyncNodes == {AsyncMatching, AsyncMismatch}
MatchingNodes == {DirectMatching, AsyncMatching}
MismatchNodes == Nodes \ MatchingNodes
NoFailure == "none"
MetadataFailure == "metadata"
DuplicateFailure == "duplicate"
FailureKinds == {MetadataFailure, DuplicateFailure}

VARIABLES
    phase,
    verified,
    engineRunning,
    enteredRunning,
    alive,
    nonzeroExit,
    failureAttempts,
    failureSignal

vars == <<
    phase,
    verified,
    engineRunning,
    enteredRunning,
    alive,
    nonzeroExit,
    failureAttempts,
    failureSignal
>>

Init ==
    /\ phase = [node \in Nodes |-> "ready"]
    /\ verified = {}
    /\ engineRunning = {}
    /\ enteredRunning = {}
    /\ alive = Nodes
    /\ nonzeroExit = {}
    /\ failureAttempts = [node \in Nodes |-> <<>>]
    /\ failureSignal = [node \in Nodes |-> NoFailure]

Verify(node) ==
    /\ node \in Nodes
    /\ phase[node] \in {"ready", "running"}
    /\ IF node \in MatchingNodes
       THEN
           /\ verified' = verified \union {node}
           /\ phase' = [phase EXCEPT
                ![node] = IF @ = "running" THEN "running" ELSE "verified"]
           /\ UNCHANGED <<engineRunning, enteredRunning, alive, nonzeroExit,
                           failureAttempts, failureSignal>>
       ELSE
           /\ phase' = [phase EXCEPT
                ![node] = IF @ = "running" THEN "running" ELSE "rejected"]
           /\ IF node \in DirectNodes
              THEN
                  /\ alive' = alive \ {node}
                  /\ nonzeroExit' = nonzeroExit \union {node}
                  /\ UNCHANGED <<failureAttempts, failureSignal>>
              ELSE
                  /\ failureAttempts' = [failureAttempts EXCEPT
                       ![node] = Append(@, MetadataFailure)]
                  /\ failureSignal' = [failureSignal EXCEPT
                       ![node] = IF @ = NoFailure THEN MetadataFailure ELSE @]
                  /\ UNCHANGED <<alive, nonzeroExit>>
           /\ UNCHANGED <<verified, engineRunning, enteredRunning>>

PublishRunning(node) ==
    /\ node \in Nodes
    /\ phase[node] \in {"ready", "verified"}
    /\ (~VerifyBeforePublish \/ node \in verified)
    /\ phase' = [phase EXCEPT ![node] = "running"]
    /\ engineRunning' = engineRunning \union {node}
    /\ enteredRunning' = enteredRunning \union {node}
    /\ UNCHANGED <<verified, alive, nonzeroExit, failureAttempts, failureSignal>>

DuplicateReport(node, failure) ==
    /\ node \in AsyncNodes
    /\ failure \in FailureKinds
    /\ failureSignal[node] # NoFailure
    /\ Len(failureAttempts[node]) < 3
    /\ failureAttempts' = [failureAttempts EXCEPT ![node] = Append(@, failure)]
    /\ UNCHANGED <<phase, verified, engineRunning, enteredRunning, alive,
                    nonzeroExit, failureSignal>>

Supervise(node) ==
    /\ SupervisorEnabled
    /\ node \in AsyncNodes
    /\ node \in alive
    /\ failureSignal[node] # NoFailure
    /\ phase' = [phase EXCEPT ![node] = "stopped"]
    /\ engineRunning' = engineRunning \ {node}
    /\ alive' = alive \ {node}
    /\ nonzeroExit' = nonzeroExit \union {node}
    /\ UNCHANGED <<verified, enteredRunning, failureAttempts, failureSignal>>

Next ==
    (\E node \in Nodes : Verify(node))
    \/ (\E node \in Nodes : PublishRunning(node))
    \/ (\E node \in AsyncNodes, failure \in FailureKinds :
          DuplicateReport(node, failure))
    \/ (\E node \in AsyncNodes : Supervise(node))

Spec ==
    Init
    /\ [][Next]_vars
    /\ \A node \in AsyncNodes : WF_vars(Supervise(node))

TypeOK ==
    /\ phase \in [Nodes -> {"ready", "verified", "rejected", "running", "stopped"}]
    /\ verified \subseteq Nodes
    /\ engineRunning \subseteq Nodes
    /\ enteredRunning \subseteq Nodes
    /\ alive \subseteq Nodes
    /\ nonzeroExit \subseteq Nodes
    /\ failureAttempts \in [Nodes -> Seq(FailureKinds)]
    /\ failureSignal \in [Nodes -> FailureKinds \union {NoFailure}]

Inv_RunningImpliesVerified == engineRunning \subseteq verified

Inv_RunningEventImpliesVerified == enteredRunning \subseteq verified

Inv_MismatchNeverPublishesRunning == MismatchNodes \intersect enteredRunning = {}

Inv_DirectMismatchFailsNonzero ==
    phase[DirectMismatch] \in {"rejected", "stopped"}
    => DirectMismatch \in nonzeroExit /\ DirectMismatch \notin alive

Inv_AsyncMismatchSignalsBeforeExit ==
    phase[AsyncMismatch] = "rejected"
    => failureSignal[AsyncMismatch] = MetadataFailure

Inv_FailureSignalFirstWins ==
    \A node \in AsyncNodes :
        IF Len(failureAttempts[node]) = 0
        THEN failureSignal[node] = NoFailure
        ELSE failureSignal[node] = Head(failureAttempts[node])

Liveness_AsyncMismatchTerminates ==
    (phase[AsyncMismatch] = "rejected") ~>
        (AsyncMismatch \notin alive /\ AsyncMismatch \in nonzeroExit)
=============================================================================
