-------------------- MODULE RSpaceStateImportFfiBoundary --------------------
EXTENDS FiniteSets, Naturals, TLC

(***************************************************************************
Bounded formal-first model of the affine state-import FFI boundary.

The six exported operations are modeled as a sequence of ordinary Rust
operation, primary result encoding, and (after an ordinary unwind) fallback
error encoding.  Open and sealed capabilities produced by begin/seal remain
private until the primary success result exists.  Publication is different:
the root catalog may become authoritative before result encoding, so a later
unwind must never be reported as an unpublished failure.  The boundary yields
Null after an observed publication effect; catalog authority, rather than an
invented response, then decides the published fact.  Filesystem durability is
a separate persistence-backend obligation.

This is deliberately an ordinary-unwind model.  Abort-mode panics, native
stack overflow, OOM abort, and faults already crossing a non-unwinding ABI are
outside catch_unwind's contract and require subprocess evidence.
***************************************************************************)

Operations == {"Begin", "StagePage", "Seal", "Publish", "AbortOpen", "AbortSealed"}
Capabilities == {"None", "Open", "PoisonedOpen", "Sealed"}
PrivateCapabilities == {"None", "Open", "Sealed"}
Outputs == {"Null", "Open", "Sealed"}
Phases == {"Ready", "Operation", "PrimaryEncoding", "FallbackEncoding", "Done"}
Payloads == {"None", "Success", "Error"}
Responses == {"None", "Success", "Error", "PanicError", "Null", "Escaped"}
Effects == {"None", "Opened", "PageStaged", "Sealed", "Published", "OpenAborted", "SealedAborted"}

CONSTANT MaxCalls

VARIABLES
    capability,
    privateCapability,
    output,
    phase,
    operation,
    payload,
    response,
    effect,
    callPublished,
    inputConsumed,
    published,
    publicationCount,
    callPublicationCount,
    stagedPages,
    calls,
    observedOperations

vars == <<capability, privateCapability, output, phase, operation, payload,
          response, effect, callPublished, inputConsumed, published, publicationCount,
          callPublicationCount, stagedPages, calls, observedOperations>>

Init ==
    /\ capability = "None"
    /\ privateCapability = "None"
    /\ output = "Null"
    /\ phase = "Ready"
    /\ operation = "None"
    /\ payload = "None"
    /\ response = "None"
    /\ effect = "None"
    /\ callPublished = FALSE
    /\ inputConsumed = FALSE
    /\ published = FALSE
    /\ publicationCount = 0
    /\ callPublicationCount = 0
    /\ stagedPages = 0
    /\ calls = 0
    /\ observedOperations = {}

EnabledOperation(op) ==
    CASE op = "Begin"       -> capability = "None"
      [] op = "StagePage"   -> capability = "Open"
      [] op = "Seal"       -> capability = "Open"
      [] op = "Publish"    -> capability = "Sealed"
      [] op = "AbortOpen"  -> capability \in {"Open", "PoisonedOpen"}
      [] op = "AbortSealed"-> capability = "Sealed"

Start(op) ==
    /\ phase \in {"Ready", "Done"}
    /\ calls < MaxCalls
    /\ op \in Operations
    /\ EnabledOperation(op)
    /\ phase' = "Operation"
    /\ operation' = op
    /\ payload' = "None"
    /\ response' = "None"
    /\ effect' = "None"
    /\ callPublished' = FALSE
    /\ inputConsumed' = FALSE
    /\ privateCapability' = "None"
    /\ output' = "Null"
    /\ calls' = calls + 1
    /\ callPublicationCount' = 0
    /\ observedOperations' = observedOperations \union {op}
    /\ UNCHANGED <<capability, published, publicationCount, stagedPages>>

StartAny == \E op \in Operations: Start(op)

OperationSucceeds ==
    /\ phase = "Operation"
    /\ payload' = "Success"
    /\ phase' = "PrimaryEncoding"
    /\ response' = "None"
    /\ callPublished' = (operation = "Publish")
    /\ inputConsumed' = (operation \in {"Seal", "Publish", "AbortOpen", "AbortSealed"})
    /\ CASE operation = "Begin" ->
         /\ privateCapability' = "Open"
         /\ effect' = "Opened"
         /\ UNCHANGED <<capability, published, publicationCount, stagedPages>>
       [] operation = "StagePage" ->
         /\ privateCapability' = "None"
         /\ effect' = "PageStaged"
         /\ stagedPages' = stagedPages + 1
         /\ UNCHANGED <<capability, published, publicationCount>>
       [] operation = "Seal" ->
         /\ privateCapability' = "Sealed"
         /\ effect' = "Sealed"
         /\ capability' = "None"
         /\ UNCHANGED <<published, publicationCount, stagedPages>>
       [] operation = "Publish" ->
         /\ privateCapability' = "None"
         /\ effect' = "Published"
         /\ capability' = "None"
         /\ published' = TRUE
         /\ publicationCount' = publicationCount + 1
         /\ callPublicationCount' = callPublicationCount + 1
         /\ UNCHANGED stagedPages
       [] operation = "AbortOpen" ->
         /\ privateCapability' = "None"
         /\ effect' = "OpenAborted"
         /\ capability' = "None"
         /\ UNCHANGED <<published, publicationCount, stagedPages>>
       [] operation = "AbortSealed" ->
         /\ privateCapability' = "None"
         /\ effect' = "SealedAborted"
         /\ capability' = "None"
         /\ UNCHANGED <<published, publicationCount, stagedPages>>
    /\ IF operation = "Publish"
          THEN UNCHANGED <<output, operation, calls, observedOperations>>
          ELSE UNCHANGED <<output, operation, callPublicationCount, calls,
                           observedOperations>>

OperationReturnsError ==
    /\ phase = "Operation"
    /\ operation \in {"Begin", "StagePage", "Seal", "Publish"}
    /\ phase' = "PrimaryEncoding"
    /\ payload' = "Error"
    /\ response' = "None"
    /\ effect' = "None"
    /\ callPublished' = FALSE
    /\ inputConsumed' = (operation \in {"Seal", "Publish"})
    /\ privateCapability' = "None"
    /\ IF operation \in {"Seal", "Publish"}
          THEN capability' = "None"
          ELSE UNCHANGED capability
    /\ UNCHANGED <<output, published, publicationCount, callPublicationCount, stagedPages,
                    operation, calls, observedOperations>>

BoundaryPanicsBeforeOperation ==
    /\ phase = "Operation"
    /\ phase' = "FallbackEncoding"
    /\ payload' = "None"
    /\ response' = "None"
    /\ effect' = "None"
    /\ callPublished' = FALSE
    /\ inputConsumed' = FALSE
    /\ privateCapability' = "None"
    /\ UNCHANGED <<capability, output, published, publicationCount,
                    callPublicationCount, stagedPages, operation, calls,
                    observedOperations>>

OperationBodyPanicsBeforeEffect ==
    /\ phase = "Operation"
    /\ phase' = "FallbackEncoding"
    /\ payload' = "None"
    /\ response' = "None"
    /\ effect' = "None"
    /\ callPublished' = FALSE
    /\ inputConsumed' = (operation \in {"Seal", "Publish", "AbortOpen", "AbortSealed"})
    /\ privateCapability' = "None"
    /\ CASE operation = "StagePage" -> capability' = "PoisonedOpen"
       [] operation \in {"Seal", "Publish", "AbortOpen", "AbortSealed"} ->
            capability' = "None"
       [] OTHER -> UNCHANGED capability
    /\ UNCHANGED <<output, published, publicationCount, callPublicationCount, stagedPages,
                    operation, calls, observedOperations>>

PublishPanicsAfterPublication ==
    /\ phase = "Operation"
    /\ operation = "Publish"
    /\ phase' = "FallbackEncoding"
    /\ payload' = "None"
    /\ response' = "None"
    /\ effect' = "Published"
    /\ callPublished' = TRUE
    /\ inputConsumed' = TRUE
    /\ privateCapability' = "None"
    /\ capability' = "None"
    /\ published' = TRUE
    /\ publicationCount' = publicationCount + 1
    /\ callPublicationCount' = callPublicationCount + 1
    /\ UNCHANGED <<output, stagedPages, operation, calls, observedOperations>>

PrimaryEncodingSucceeds ==
    /\ phase = "PrimaryEncoding"
    /\ phase' = "Done"
    /\ response' = IF payload = "Success" THEN "Success" ELSE "Error"
    /\ CASE privateCapability = "Open" ->
         /\ capability' = "Open"
         /\ output' = "Open"
       [] privateCapability = "Sealed" ->
         /\ capability' = "Sealed"
         /\ output' = "Sealed"
       [] OTHER ->
         /\ UNCHANGED capability
         /\ output' = "Null"
    /\ privateCapability' = "None"
    /\ UNCHANGED <<operation, payload, effect, callPublished, inputConsumed, published,
                    publicationCount, callPublicationCount, stagedPages, calls,
                    observedOperations>>

PrimaryEncodingPanics ==
    /\ phase = "PrimaryEncoding"
    /\ phase' = "FallbackEncoding"
    /\ response' = "None"
    /\ privateCapability' = "None"
    /\ output' = "Null"
    /\ IF operation = "StagePage" /\ effect = "PageStaged"
          THEN capability' = "PoisonedOpen"
          ELSE UNCHANGED capability
    /\ UNCHANGED <<operation, payload, effect, callPublished, inputConsumed, published,
                    publicationCount, callPublicationCount, stagedPages, calls,
                    observedOperations>>

FallbackEncodingSucceeds ==
    /\ phase = "FallbackEncoding"
    /\ phase' = "Done"
    /\ response' = IF callPublished THEN "Null" ELSE "PanicError"
    /\ output' = "Null"
    /\ UNCHANGED <<capability, privateCapability, operation, payload, effect,
                    callPublished, inputConsumed, published, publicationCount,
                    callPublicationCount, stagedPages, calls,
                    observedOperations>>

FallbackEncodingPanics ==
    /\ phase = "FallbackEncoding"
    /\ phase' = "Done"
    /\ response' = "Null"
    /\ output' = "Null"
    /\ UNCHANGED <<capability, privateCapability, operation, payload, effect,
                    callPublished, inputConsumed, published, publicationCount,
                    callPublicationCount, stagedPages, calls,
                    observedOperations>>

OperationStep == OperationSucceeds
     \/ OperationReturnsError
     \/ BoundaryPanicsBeforeOperation
     \/ OperationBodyPanicsBeforeEffect
     \/ PublishPanicsAfterPublication

EncodingStep == PrimaryEncodingSucceeds
     \/ PrimaryEncodingPanics
     \/ FallbackEncodingSucceeds
     \/ FallbackEncodingPanics

Next == StartAny \/ OperationStep \/ EncodingStep

Spec == Init /\ [][Next]_vars /\ WF_vars(OperationStep) /\ WF_vars(EncodingStep)

TypeInvariant ==
    /\ capability \in Capabilities
    /\ privateCapability \in PrivateCapabilities
    /\ output \in Outputs
    /\ phase \in Phases
    /\ operation \in Operations \union {"None"}
    /\ payload \in Payloads
    /\ response \in Responses
    /\ effect \in Effects
    /\ callPublished \in BOOLEAN
    /\ inputConsumed \in BOOLEAN
    /\ published \in BOOLEAN
    /\ publicationCount \in 0..MaxCalls
    /\ callPublicationCount \in 0..1
    /\ stagedPages \in 0..MaxCalls
    /\ calls \in 0..MaxCalls
    /\ observedOperations \subseteq Operations

AtMostOneAffineCapability ==
    ~(capability # "None" /\ privateCapability # "None")

PreparedCapabilityIsPrivate ==
    privateCapability # "None" =>
      /\ capability = "None"
      /\ output = "Null"
      /\ phase = "PrimaryEncoding"
      /\ payload = "Success"

CapabilityOutputRequiresEncodedSuccess ==
    output # "Null" =>
      /\ phase = "Done"
      /\ response = "Success"
      /\ capability = output

FailureLeavesNoNewCapabilityOutput ==
    phase = "Done" /\ response # "Success" => output = "Null"

NoOrdinaryUnwindEscapesBoundary == response # "Escaped"

PublicationIsMonotoneAndUnique ==
    /\ (publicationCount > 0) = published
    /\ callPublicationCount <= publicationCount
    /\ callPublished = (callPublicationCount = 1)

PublicationResponseIsTruthful ==
    phase = "Done" /\ operation = "Publish" /\ callPublished =>
      response \in {"Success", "Null"}

PrePublicationPanicIsStructured ==
    phase = "Done" /\ operation = "Publish" /\ response = "PanicError" =>
      ~callPublished

PublishSuccessRequiresPublication ==
    phase = "Done" /\ operation = "Publish" /\ response = "Success" =>
      /\ callPublished
      /\ published

StageEncodingFailurePoisonsMutatedOpen ==
    operation = "StagePage" /\ effect = "PageStaged"
      /\ phase \in {"FallbackEncoding", "Done"}
      /\ response # "Success" => capability = "PoisonedOpen"

AbortConsumesWithoutPublishing ==
    phase = "Done" /\ operation \in {"AbortOpen", "AbortSealed"} =>
      /\ ~callPublished
      /\ (inputConsumed => capability = "None")
      /\ (response = "Success" => inputConsumed)

EveryStartedCallReturns ==
    [](phase \in {"Operation", "PrimaryEncoding", "FallbackEncoding"} =>
       <>(phase = "Done"))

NeverTwoPublications == publicationCount < 2

(***************************************************************************
Negative controls. Each introduces exactly the target defect while retaining
the control transitions, and each dedicated cfg checks the named invariant.
***************************************************************************)

ExposeCapabilityBeforeEncodingMutant ==
    /\ phase = "PrimaryEncoding"
    /\ privateCapability \in {"Open", "Sealed"}
    /\ capability' = privateCapability
    /\ output' = privateCapability
    /\ privateCapability' = "None"
    /\ UNCHANGED <<phase, operation, payload, response, effect, callPublished,
                    inputConsumed,
                    published, publicationCount, callPublicationCount,
                    stagedPages, calls,
                    observedOperations>>

MisclassifyPublishedPanicMutant ==
    /\ phase = "FallbackEncoding"
    /\ callPublished
    /\ phase' = "Done"
    /\ response' = "PanicError"
    /\ output' = "Null"
    /\ UNCHANGED <<capability, privateCapability, operation, payload, effect,
                    callPublished, inputConsumed, published, publicationCount,
                    callPublicationCount, stagedPages, calls,
                    observedOperations>>

EscapeOrdinaryUnwindMutant ==
    /\ phase = "FallbackEncoding"
    /\ phase' = "Done"
    /\ response' = "Escaped"
    /\ output' = "Null"
    /\ UNCHANGED <<capability, privateCapability, operation, payload, effect,
                    callPublished, inputConsumed, published, publicationCount,
                    callPublicationCount, stagedPages, calls,
                    observedOperations>>

KeepStagedOpenUsableAfterEncodingPanicMutant ==
    /\ phase = "PrimaryEncoding"
    /\ operation = "StagePage"
    /\ effect = "PageStaged"
    /\ phase' = "FallbackEncoding"
    /\ response' = "None"
    /\ capability' = "Open"
    /\ privateCapability' = "None"
    /\ output' = "Null"
    /\ UNCHANGED <<operation, payload, effect, callPublished, inputConsumed, published,
                    publicationCount, callPublicationCount, stagedPages, calls,
                    observedOperations>>

DuplicatePublicationWithinCallMutant ==
    /\ phase = "PrimaryEncoding"
    /\ operation = "Publish"
    /\ effect = "Published"
    /\ callPublicationCount = 1
    /\ publicationCount' = publicationCount + 1
    /\ callPublicationCount' = callPublicationCount + 1
    /\ UNCHANGED <<capability, privateCapability, output, phase, operation,
                    payload, response, effect, callPublished, inputConsumed, published,
                    stagedPages, calls, observedOperations>>

ExposeMutantNext == Next \/ ExposeCapabilityBeforeEncodingMutant
ExposeMutantSpec == Init /\ [][ExposeMutantNext]_vars

MisclassifyMutantNext == Next \/ MisclassifyPublishedPanicMutant
MisclassifyMutantSpec == Init /\ [][MisclassifyMutantNext]_vars

EscapeMutantNext == Next \/ EscapeOrdinaryUnwindMutant
EscapeMutantSpec == Init /\ [][EscapeMutantNext]_vars

UsableStagedOpenMutantNext == Next \/ KeepStagedOpenUsableAfterEncodingPanicMutant
UsableStagedOpenMutantSpec == Init /\ [][UsableStagedOpenMutantNext]_vars

DuplicatePublicationMutantNext == Next \/ DuplicatePublicationWithinCallMutant
DuplicatePublicationMutantSpec == Init /\ [][DuplicatePublicationMutantNext]_vars

NoFairnessMutantSpec == Init /\ [][Next]_vars

=============================================================================
