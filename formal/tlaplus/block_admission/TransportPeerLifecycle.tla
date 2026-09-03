---------------------- MODULE TransportPeerLifecycle ----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
    \* @type: Set(Str);
    Work,
    \* @type: Set(Str);
    Networks,
    \* @type: Str;
    ExpectedNetwork,
    \* @type: Bool;
    GuardInitialization,
    \* @type: Bool;
    IdleOnlyRetirement,
    \* @type: Bool;
    RequestScopedValidation

ASSUME Work # {} /\ Networks # {} /\ ExpectedNetwork \in Networks

Phases == {"new", "initializing", "ready", "sending", "resident",
           "handling", "done", "aborted"}
Decisions == {"none", "accept", "reject"}

VARIABLES
    \* @type: Str -> Str;
    phase,
    \* @type: Str -> Str;
    headerNetwork,
    \* @type: Str -> Str;
    validationDecision,
    \* @type: Str;
    globalValidationNetwork,
    \* @type: Bool;
    mapPresent,
    \* @type: Bool;
    slotReady,
    \* @type: Bool;
    accepting,
    \* @type: Set(Str);
    initGuards,
    \* @type: Set(Str);
    sendGuards,
    \* @type: Set(Str);
    orphanOwners,
    \* @type: Set(Str);
    acknowledged

vars == <<phase, headerNetwork, validationDecision, globalValidationNetwork,
          mapPresent, slotReady, accepting, initGuards, sendGuards,
          orphanOwners, acknowledged>>

Initializing == {work \in Work : phase[work] = "initializing"}
Sending == {work \in Work : phase[work] = "sending"}
Resident == {work \in Work : phase[work] = "resident"}
Handling == {work \in Work : phase[work] = "handling"}
LookupOwned == {work \in Work : phase[work] \in {"initializing", "ready"}}
Completed == {work \in Work : phase[work] = "done"}
Aborted == {work \in Work : phase[work] = "aborted"}

Init ==
    /\ phase = [work \in Work |-> "new"]
    /\ headerNetwork = [work \in Work |-> ExpectedNetwork]
    /\ validationDecision = [work \in Work |-> "none"]
    /\ globalValidationNetwork = ExpectedNetwork
    /\ mapPresent = FALSE
    /\ slotReady = FALSE
    /\ accepting = TRUE
    /\ initGuards = {}
    /\ sendGuards = {}
    /\ orphanOwners = {}
    /\ acknowledged = {}

BeginLookup(work, network) ==
    /\ phase[work] = "new"
    /\ network \in Networks
    /\ phase' = [phase EXCEPT ![work] = IF mapPresent /\ slotReady
                                              THEN "ready"
                                              ELSE "initializing"]
    /\ headerNetwork' = [headerNetwork EXCEPT ![work] = network]
    /\ globalValidationNetwork' = network
    /\ mapPresent' = TRUE
    /\ accepting' = TRUE
    /\ initGuards' = IF GuardInitialization
                      THEN initGuards \cup {work}
                      ELSE initGuards
    /\ UNCHANGED <<validationDecision, slotReady, sendGuards,
                    orphanOwners, acknowledged>>

FinishInitialization(work) ==
    /\ phase[work] = "initializing"
    /\ phase' = [phase EXCEPT ![work] = "ready"]
    /\ slotReady' = IF mapPresent THEN TRUE ELSE slotReady
    /\ initGuards' = initGuards
    /\ orphanOwners' = IF mapPresent
                        THEN orphanOwners
                        ELSE orphanOwners \cup {work}
    /\ UNCHANGED <<headerNetwork, validationDecision, globalValidationNetwork,
                    mapPresent, accepting, sendGuards, acknowledged>>

EnterSend(work) ==
    /\ phase[work] = "ready"
    /\ accepting
    /\ mapPresent \/ work \in orphanOwners
    /\ phase' = [phase EXCEPT ![work] = "sending"]
    /\ initGuards' = initGuards \ {work}
    /\ sendGuards' = sendGuards \cup {work}
    /\ UNCHANGED <<headerNetwork, validationDecision, globalValidationNetwork,
                    mapPresent, slotReady, accepting,
                    orphanOwners, acknowledged>>

EnqueueAndAcknowledge(work) ==
    /\ phase[work] = "sending"
    /\ work \in sendGuards
    /\ phase' = [phase EXCEPT ![work] = "resident"]
    /\ sendGuards' = sendGuards \ {work}
    /\ acknowledged' = acknowledged \cup {work}
    /\ UNCHANGED <<headerNetwork, validationDecision, globalValidationNetwork,
                    mapPresent, slotReady, accepting, initGuards, orphanOwners>>

StartHandling(work) ==
    /\ phase[work] = "resident"
    /\ phase' = [phase EXCEPT ![work] = "handling"]
    /\ UNCHANGED <<headerNetwork, validationDecision, globalValidationNetwork,
                    mapPresent, slotReady, accepting, initGuards, sendGuards,
                    orphanOwners, acknowledged>>

CompleteHandling(work) ==
    /\ phase[work] = "handling"
    /\ phase' = [phase EXCEPT ![work] = "done"]
    /\ orphanOwners' = orphanOwners \ {work}
    /\ UNCHANGED <<headerNetwork, validationDecision, globalValidationNetwork,
                    mapPresent, slotReady, accepting, initGuards, sendGuards,
                    acknowledged>>

CleanupUninitializedSlot ==
    /\ mapPresent
    /\ ~slotReady
    /\ (~GuardInitialization \/ initGuards = {})
    /\ mapPresent' = FALSE
    /\ UNCHANGED <<phase, headerNetwork, validationDecision,
                    globalValidationNetwork, slotReady, accepting, initGuards,
                    sendGuards, orphanOwners, acknowledged>>

CleanupReadySlot ==
    /\ mapPresent
    /\ slotReady
    /\ IF IdleOnlyRetirement
       THEN initGuards = {} /\ sendGuards = {}
            /\ Resident = {} /\ Handling = {}
       ELSE Resident # {} \/ Handling # {}
    /\ mapPresent' = FALSE
    /\ slotReady' = FALSE
    /\ accepting' = FALSE
    /\ phase' = [work \in Work |->
                    IF ~IdleOnlyRetirement
                       /\ phase[work] \in {"resident", "handling"}
                    THEN "aborted"
                    ELSE phase[work]]
    /\ sendGuards' = IF IdleOnlyRetirement THEN sendGuards ELSE {}
    /\ UNCHANGED <<headerNetwork, validationDecision,
                    globalValidationNetwork, initGuards, orphanOwners,
                    acknowledged>>

DropOrphan(work) ==
    /\ work \in orphanOwners
    /\ phase[work] \in {"resident", "handling"}
    /\ phase' = [phase EXCEPT ![work] = "aborted"]
    /\ orphanOwners' = orphanOwners \ {work}
    /\ UNCHANGED <<headerNetwork, validationDecision, globalValidationNetwork,
                    mapPresent, slotReady, accepting, initGuards, sendGuards,
                    acknowledged>>

Validate(work) ==
    /\ phase[work] # "new"
    /\ validationDecision[work] = "none"
    /\ LET observed == IF RequestScopedValidation
                       THEN headerNetwork[work]
                       ELSE globalValidationNetwork
       IN validationDecision' =
            [validationDecision EXCEPT
                ![work] = IF observed = ExpectedNetwork THEN "accept" ELSE "reject"]
    /\ UNCHANGED <<phase, headerNetwork, globalValidationNetwork, mapPresent,
                    slotReady, accepting, initGuards, sendGuards, orphanOwners,
                    acknowledged>>

Quiescent ==
    /\ \A work \in Work : phase[work] \in {"done", "aborted"}
    /\ \A work \in Work : validationDecision[work] # "none"
    /\ UNCHANGED vars

Next ==
    \/ \E work \in Work, network \in Networks : BeginLookup(work, network)
    \/ \E work \in Work : FinishInitialization(work)
    \/ \E work \in Work : EnterSend(work)
    \/ \E work \in Work : EnqueueAndAcknowledge(work)
    \/ \E work \in Work : StartHandling(work)
    \/ \E work \in Work : CompleteHandling(work)
    \/ CleanupUninitializedSlot
    \/ CleanupReadySlot
    \/ \E work \in Work : DropOrphan(work)
    \/ \E work \in Work : Validate(work)
    \/ Quiescent

Fairness ==
    /\ \A work \in Work : WF_vars(FinishInitialization(work))
    /\ \A work \in Work : WF_vars(EnterSend(work))
    /\ \A work \in Work : WF_vars(EnqueueAndAcknowledge(work))
    /\ \A work \in Work : WF_vars(StartHandling(work))
    /\ \A work \in Work : WF_vars(CompleteHandling(work))
    /\ \A work \in Work : WF_vars(DropOrphan(work))
    /\ \A work \in Work : WF_vars(Validate(work))

Spec == Init /\ [][Next]_vars /\ Fairness

TypeOK ==
    /\ phase \in [Work -> Phases]
    /\ headerNetwork \in [Work -> Networks]
    /\ validationDecision \in [Work -> Decisions]
    /\ globalValidationNetwork \in Networks
    /\ mapPresent \in BOOLEAN
    /\ slotReady \in BOOLEAN
    /\ accepting \in BOOLEAN
    /\ initGuards \subseteq Work
    /\ sendGuards \subseteq Work
    /\ orphanOwners \subseteq Work
    /\ acknowledged \subseteq Work
    /\ initGuards \subseteq LookupOwned
    /\ sendGuards \subseteq Sending
    /\ orphanOwners \cap Completed = {}

Inv_InitializingOwnsMappedSlot == LookupOwned = {} \/ mapPresent
Inv_AcknowledgedWorkPreserved == acknowledged \cap Aborted = {}
Inv_NoOrphanedQueueOwners == orphanOwners = {}
Inv_ActiveWorkPreventsRetirement ==
    sendGuards # {} \/ Resident # {} \/ Handling # {} => accepting /\ mapPresent
Inv_ValidationUsesRequestContext ==
    \A work \in Work :
        validationDecision[work] # "none" =>
            (validationDecision[work] = "accept")
                = (headerNetwork[work] = ExpectedNetwork)

Safety ==
    /\ TypeOK
    /\ Inv_InitializingOwnsMappedSlot
    /\ Inv_AcknowledgedWorkPreserved
    /\ Inv_NoOrphanedQueueOwners
    /\ Inv_ActiveWorkPreventsRetirement
    /\ Inv_ValidationUsesRequestContext

Live_AcknowledgedWorkCompletes ==
    \A work \in Work : work \in acknowledged ~> work \in Completed

=============================================================================
