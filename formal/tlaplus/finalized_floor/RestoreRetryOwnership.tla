------------------------ MODULE RestoreRetryOwnership ------------------------
EXTENDS Naturals, TLC

CONSTANTS
    \* @type: Int;
    MaxFailures,
    \* @type: Bool;
    LoseRetryOwnership,
    \* @type: Bool;
    AllowDuplicateRestore,
    \* @type: Bool;
    ReopenAfterCommit,
    \* @type: Bool;
    AllowStaleRequestTermination

ASSUME /\ MaxFailures \in Nat \ {0}
       /\ LoseRetryOwnership \in BOOLEAN
       /\ AllowDuplicateRestore \in BOOLEAN
       /\ ReopenAfterCommit \in BOOLEAN
       /\ AllowStaleRequestTermination \in BOOLEAN

VARIABLES
    \* @type: Str;
    phase,
    \* @type: Int;
    failures,
    \* @type: Int;
    generation,
    \* @type: Bool;
    requestPending,
    \* @type: Bool;
    channelsReady,
    \* @type: Int;
    activeRestores,
    \* @type: Int;
    duplicateMessages,
    \* @type: Set(Int);
    retryRequests,
    \* @type: Bool;
    runningPublished,
    \* @type: Bool;
    terminalPublished,
    \* @type: Int;
    terminalGeneration,
    \* @type: Int;
    terminalPublications,
    \* @type: Bool;
    noticeFailed,
    \* @type: Bool;
    restartAvailable

vars ==
    <<phase, failures, generation, requestPending, channelsReady,
      activeRestores, duplicateMessages, retryRequests, runningPublished,
      terminalPublished, terminalGeneration, terminalPublications,
      noticeFailed, restartAvailable>>

Init ==
    /\ phase = "idle"
    /\ failures = 0
    /\ generation = 0
    /\ requestPending = TRUE
    /\ channelsReady = TRUE
    /\ activeRestores = 0
    /\ duplicateMessages = 1
    /\ retryRequests = {}
    /\ runningPublished = FALSE
    /\ terminalPublished = FALSE
    /\ terminalGeneration = 0
    /\ terminalPublications = 0
    /\ noticeFailed = FALSE
    /\ restartAvailable = TRUE

ReceiveApproved ==
    /\ phase = "idle"
    /\ requestPending
    /\ channelsReady
    /\ phase' = "restoring"
    /\ generation' = generation + 1
    /\ requestPending' = FALSE
    /\ channelsReady' = FALSE
    /\ activeRestores' = 1
    /\ UNCHANGED <<failures, duplicateMessages, retryRequests,
                    runningPublished, terminalPublished, terminalGeneration,
                    terminalPublications, noticeFailed, restartAvailable>>

ReceiveDuplicate ==
    /\ phase = "restoring"
    /\ duplicateMessages > 0
    /\ duplicateMessages' = duplicateMessages - 1
    /\ activeRestores' = IF AllowDuplicateRestore THEN activeRestores + 1
                         ELSE activeRestores
    /\ UNCHANGED <<phase, failures, generation, requestPending, channelsReady,
                    retryRequests, runningPublished, terminalPublished,
                    terminalGeneration, terminalPublications, noticeFailed,
                    restartAvailable>>

RestoreSucceeds ==
    /\ phase = "restoring"
    /\ phase' = "running"
    /\ requestPending' = FALSE
    /\ activeRestores' = 0
    /\ runningPublished' = TRUE
    /\ terminalPublished' = FALSE
    /\ UNCHANGED <<failures, generation, channelsReady, duplicateMessages,
                    retryRequests, terminalGeneration, terminalPublications,
                    noticeFailed, restartAvailable>>

RestoreFails ==
    /\ phase = "restoring"
    /\ failures' = failures + 1
    /\ activeRestores' = 0
    /\ IF failures' < MaxFailures
       THEN /\ phase' = "idle"
            /\ requestPending' = ~LoseRetryOwnership
            /\ channelsReady' = ~LoseRetryOwnership
            /\ retryRequests' = retryRequests \cup {generation}
            /\ terminalPublished' = FALSE
            /\ UNCHANGED <<terminalGeneration, terminalPublications>>
       ELSE /\ phase' = "terminal"
            /\ requestPending' = FALSE
            /\ channelsReady' = FALSE
            /\ retryRequests' = retryRequests
            /\ terminalPublished' = TRUE
            /\ terminalGeneration' = generation
            /\ terminalPublications' = terminalPublications + 1
    /\ UNCHANGED <<generation, duplicateMessages, runningPublished,
                    noticeFailed, restartAvailable>>

RetryRequestSucceeds(token) ==
    /\ token \in retryRequests
    /\ retryRequests' = retryRequests \ {token}
    /\ UNCHANGED <<phase, failures, generation, requestPending,
                    channelsReady, activeRestores, duplicateMessages,
                    runningPublished, terminalPublished, terminalGeneration,
                    terminalPublications, noticeFailed, restartAvailable>>

RetryRequestFails(token) ==
    /\ token \in retryRequests
    /\ retryRequests' = retryRequests \ {token}
    /\ IF /\ phase = "idle"
           /\ generation = token
       THEN /\ phase' = "terminal"
            /\ requestPending' = FALSE
            /\ terminalPublished' = TRUE
            /\ terminalGeneration' = token
            /\ terminalPublications' = terminalPublications + 1
       ELSE IF /\ AllowStaleRequestTermination
                    /\ phase # "running"
            THEN /\ phase' = "terminal"
                 /\ requestPending' = FALSE
                 /\ terminalPublished' = TRUE
                 /\ terminalGeneration' = token
                 /\ terminalPublications' = terminalPublications + 1
            ELSE /\ UNCHANGED <<phase, requestPending, terminalPublished,
                                 terminalGeneration, terminalPublications>>
    /\ UNCHANGED <<failures, generation, channelsReady, activeRestores,
                    duplicateMessages, runningPublished, noticeFailed,
                    restartAvailable>>

RetryRequestOutcome ==
    \E token \in retryRequests :
        RetryRequestSucceeds(token) \/ RetryRequestFails(token)

PostCommitNoticeFails ==
    /\ phase = "running"
    /\ ~noticeFailed
    /\ noticeFailed' = TRUE
    /\ IF ReopenAfterCommit
       THEN /\ phase' = "idle"
            /\ requestPending' = TRUE
            /\ channelsReady' = TRUE
       ELSE /\ UNCHANGED <<phase, requestPending, channelsReady>>
    /\ UNCHANGED <<failures, generation, activeRestores, duplicateMessages,
                    retryRequests, runningPublished, terminalPublished,
                    terminalGeneration, terminalPublications,
                    restartAvailable>>

Restart ==
    /\ phase = "terminal"
    /\ restartAvailable
    /\ phase' = "idle"
    /\ failures' = 0
    /\ generation' = 0
    /\ requestPending' = TRUE
    /\ channelsReady' = TRUE
    /\ activeRestores' = 0
    /\ duplicateMessages' = 1
    /\ retryRequests' = {}
    /\ runningPublished' = FALSE
    /\ terminalPublished' = FALSE
    /\ terminalGeneration' = 0
    /\ terminalPublications' = 0
    /\ noticeFailed' = FALSE
    /\ restartAvailable' = FALSE

Outcome == RestoreSucceeds \/ RestoreFails

Next ==
    ReceiveApproved
    \/ ReceiveDuplicate
    \/ Outcome
    \/ RetryRequestOutcome
    \/ PostCommitNoticeFails
    \/ Restart

Spec ==
    Init
    /\ [][Next]_vars
    /\ WF_vars(ReceiveApproved)
    /\ WF_vars(Outcome)
    /\ WF_vars(RetryRequestOutcome)

TypeOK ==
    /\ phase \in {"idle", "restoring", "running", "terminal"}
    /\ failures \in Nat
    /\ generation \in Nat
    /\ requestPending \in BOOLEAN
    /\ channelsReady \in BOOLEAN
    /\ activeRestores \in 0..2
    /\ duplicateMessages \in 0..1
    /\ retryRequests \subseteq 0..MaxFailures
    /\ runningPublished \in BOOLEAN
    /\ terminalPublished \in BOOLEAN
    /\ terminalGeneration \in Nat
    /\ terminalPublications \in Nat
    /\ noticeFailed \in BOOLEAN
    /\ restartAvailable \in BOOLEAN

AtMostOneRestore == activeRestores <= 1

RestoreLeaseExact ==
    (phase = "restoring") <=> (activeRestores = 1)

IdleRetainsProgress ==
    (phase = "idle") => requestPending /\ channelsReady

GenerationBoundsFailures ==
    /\ failures <= generation
    /\ generation <= failures + 1

RunningCommitIsPermanent ==
    runningPublished => phase = "running"

TerminalFailureIsExplicit ==
    terminalPublished <=> phase = "terminal"

TerminalMatchesGeneration ==
    terminalPublished => terminalGeneration = generation

OneTerminalPublisher == terminalPublications <= 1

FailureBudgetIsBounded == failures <= MaxFailures

EventuallyRunningOrTerminal ==
    <> (phase = "running" \/ phase = "terminal")

=============================================================================
