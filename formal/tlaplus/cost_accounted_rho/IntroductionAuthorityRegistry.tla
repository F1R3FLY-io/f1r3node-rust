---------------- MODULE IntroductionAuthorityRegistry ----------------
EXTENDS Integers

CONSTANTS
    \* @type: Set(Str);
    Payers,
    \* @type: Str;
    FallbackPayer,
    \* @type: Str;
    ExplicitPayer,
    \* @type: Str;
    NoPayer,
    \* @type: Str;
    Defect

ASSUME FallbackPayer \in Payers
ASSUME ExplicitPayer \in Payers
ASSUME FallbackPayer # ExplicitPayer
ASSUME NoPayer \notin Payers
ASSUME Defect \in {"None", "SplitFallbackResolution"}

VARIABLES
    \* @type: Str;
    registry,
    \* @type: Str;
    resolved,
    \* @type: Bool;
    fallbackRead,
    \* @type: Bool;
    resolutionDone,
    \* @type: Bool;
    registrationDone,
    \* @type: Str;
    registrationResult

vars == <<
    registry,
    resolved,
    fallbackRead,
    resolutionDone,
    registrationDone,
    registrationResult
>>

Init ==
    /\ registry = NoPayer
    /\ resolved = NoPayer
    /\ fallbackRead = FALSE
    /\ resolutionDone = FALSE
    /\ registrationDone = FALSE
    /\ registrationResult = "Pending"

ResolveAtomic ==
    /\ Defect = "None"
    /\ ~resolutionDone
    /\ IF registry = NoPayer
       THEN /\ registry' = FallbackPayer
            /\ resolved' = FallbackPayer
       ELSE /\ UNCHANGED registry
            /\ resolved' = registry
    /\ resolutionDone' = TRUE
    /\ UNCHANGED <<fallbackRead, registrationDone, registrationResult>>

ReadFallback ==
    /\ Defect = "SplitFallbackResolution"
    /\ ~resolutionDone
    /\ ~fallbackRead
    /\ registry = NoPayer
    /\ fallbackRead' = TRUE
    /\ UNCHANGED <<registry, resolved, resolutionDone, registrationDone, registrationResult>>

CommitSplitFallback ==
    /\ Defect = "SplitFallbackResolution"
    /\ fallbackRead
    /\ ~resolutionDone
    /\ resolved' = FallbackPayer
    /\ resolutionDone' = TRUE
    /\ UNCHANGED <<registry, fallbackRead, registrationDone, registrationResult>>

ResolveRegistered ==
    /\ Defect = "SplitFallbackResolution"
    /\ ~resolutionDone
    /\ ~fallbackRead
    /\ registry \in Payers
    /\ resolved' = registry
    /\ resolutionDone' = TRUE
    /\ UNCHANGED <<registry, fallbackRead, registrationDone, registrationResult>>

RegisterExplicit ==
    /\ ~registrationDone
    /\ IF registry = NoPayer
       THEN /\ registry' = ExplicitPayer
            /\ registrationResult' = "Inserted"
       ELSE /\ UNCHANGED registry
            /\ registrationResult' =
                  IF registry = ExplicitPayer THEN "Idempotent" ELSE "Conflict"
    /\ registrationDone' = TRUE
    /\ UNCHANGED <<resolved, fallbackRead, resolutionDone>>

Done ==
    /\ resolutionDone
    /\ registrationDone
    /\ UNCHANGED vars

Next ==
    \/ ResolveAtomic
    \/ ReadFallback
    \/ CommitSplitFallback
    \/ ResolveRegistered
    \/ RegisterExplicit
    \/ Done

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(ResolveAtomic \/ ReadFallback \/ ResolveRegistered)
    /\ WF_vars(CommitSplitFallback)
    /\ WF_vars(RegisterExplicit)

TypeOK ==
    /\ registry \in Payers \cup {NoPayer}
    /\ resolved \in Payers \cup {NoPayer}
    /\ fallbackRead \in BOOLEAN
    /\ resolutionDone \in BOOLEAN
    /\ registrationDone \in BOOLEAN
    /\ registrationResult \in {"Pending", "Inserted", "Idempotent", "Conflict"}

ResolvedMatchesCommittedRegistry ==
    resolutionDone => resolved = registry

RegistrationResultMatchesCommittedRegistry ==
    /\ (registrationResult = "Inserted" => registry = ExplicitPayer)
    /\ (registrationResult = "Idempotent" => registry = ExplicitPayer)
    /\ (registrationResult = "Conflict" => registry = FallbackPayer)

FallbackResolutionIsPermanent ==
    resolutionDone /\ resolved = FallbackPayer => registry = FallbackPayer

ExplicitFirstResolutionUsesExplicitPayer ==
    registrationResult = "Inserted" /\ resolutionDone => resolved = ExplicitPayer

EventuallyBothComplete == <>(resolutionDone /\ registrationDone)

=============================================================================
