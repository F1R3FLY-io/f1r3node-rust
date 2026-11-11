---------- MODULE RSpaceRPB3CanonicalAdmissionPublication ----------
EXTENDS Naturals, TLC

CONSTANT MaxGeneration
ASSUME MaxGeneration \in Nat \ {0}

Inputs == {"first", "second", "malformed"}
Modes == {"checkpoint", "import-activate", "import-preserve"}
Roots == {"base", "first-root", "second-root"}
Observations == {"none", "base-observation", "first-observation",
                 "second-observation"}
Phases == {"idle", "submitted", "accepted", "history-staged",
           "staged", "sealed",
           "rejected", "published"}

Canonical(input) == input \in {"first", "second"}
OracleRoot(input) ==
    CASE input = "first" -> "first-root"
      [] input = "second" -> "second-root"
      [] OTHER -> "base"
OracleObservation(input) ==
    CASE input = "first" -> "first-observation"
      [] input = "second" -> "second-observation"
      [] OTHER -> "none"
RootObservation(root) ==
    CASE root = "base" -> "base-observation"
      [] root = "first-root" -> "first-observation"
      [] OTHER -> "second-observation"
ImportMode(mode) == mode \in {"import-activate", "import-preserve"}
ActivatingMode(mode) == mode \in {"checkpoint", "import-activate"}

VARIABLES phase, input, mode, admitted, sealed, candidateRoot,
          candidateObservation, historyObjects, coldObjects, knownRoots,
          currentRoot, generation, activationCount, rootObservations,
          requestPublicationCount, previousHistory, previousCold,
          previousKnown, previousCurrent, previousGeneration

vars == <<phase, input, mode, admitted, sealed, candidateRoot,
          candidateObservation, historyObjects, coldObjects, knownRoots,
          currentRoot, generation, activationCount, rootObservations,
          requestPublicationCount, previousHistory, previousCold,
          previousKnown, previousCurrent, previousGeneration>>

Init ==
    /\ phase = "idle"
    /\ input = "malformed"
    /\ mode = "checkpoint"
    /\ admitted = FALSE
    /\ sealed = FALSE
    /\ candidateRoot = "base"
    /\ candidateObservation = "none"
    /\ historyObjects = {"base"}
    /\ coldObjects = {"base"}
    /\ knownRoots = {"base"}
    /\ currentRoot = "base"
    /\ generation = 0
    /\ activationCount = 0
    /\ rootObservations =
        [root \in Roots |-> IF root = "base"
                           THEN "base-observation" ELSE "none"]
    /\ requestPublicationCount = 0
    /\ previousHistory = historyObjects
    /\ previousCold = coldObjects
    /\ previousKnown = knownRoots
    /\ previousCurrent = currentRoot
    /\ previousGeneration = generation

Begin(nextInput, nextMode) ==
    /\ phase \in {"idle", "rejected", "published"}
    /\ nextInput \in Inputs
    /\ nextMode \in Modes
    /\ generation < MaxGeneration
    /\ phase' = "submitted"
    /\ input' = nextInput
    /\ mode' = nextMode
    /\ admitted' = FALSE
    /\ sealed' = FALSE
    /\ candidateRoot' = "base"
    /\ candidateObservation' = "none"
    /\ requestPublicationCount' = 0
    /\ previousHistory' = historyObjects
    /\ previousCold' = coldObjects
    /\ previousKnown' = knownRoots
    /\ previousCurrent' = currentRoot
    /\ previousGeneration' = generation
    /\ UNCHANGED <<historyObjects, coldObjects, knownRoots, currentRoot,
                    generation, activationCount, rootObservations>>

BeginAny == \E nextInput \in Inputs, nextMode \in Modes:
                Begin(nextInput, nextMode)

RejectMalformed ==
    /\ phase = "submitted"
    /\ ~Canonical(input)
    /\ phase' = "rejected"
    /\ UNCHANGED <<input, mode, admitted, sealed, candidateRoot,
                    candidateObservation, historyObjects, coldObjects,
                    knownRoots, currentRoot, generation, activationCount,
                    rootObservations, requestPublicationCount,
                    previousHistory, previousCold, previousKnown,
                    previousCurrent, previousGeneration>>

AcceptCanonical ==
    /\ phase = "submitted"
    /\ Canonical(input)
    /\ phase' = "accepted"
    /\ admitted' = TRUE
    /\ candidateRoot' = OracleRoot(input)
    /\ candidateObservation' = OracleObservation(input)
    /\ UNCHANGED <<input, mode, sealed, historyObjects, coldObjects,
                    knownRoots, currentRoot, generation, activationCount,
                    rootObservations, requestPublicationCount,
                    previousHistory, previousCold, previousKnown,
                    previousCurrent, previousGeneration>>

StageImportHistory ==
    /\ phase = "accepted"
    /\ ImportMode(mode)
    /\ phase' = "history-staged"
    /\ historyObjects' = historyObjects \cup {candidateRoot}
    /\ UNCHANGED <<input, mode, admitted, sealed, candidateRoot,
                    candidateObservation, coldObjects, knownRoots,
                    currentRoot, generation, activationCount,
                    rootObservations, requestPublicationCount,
                    previousHistory, previousCold, previousKnown,
                    previousCurrent, previousGeneration>>

StageImportCold ==
    /\ phase = "history-staged"
    /\ phase' = "staged"
    /\ coldObjects' = coldObjects \cup {candidateRoot}
    /\ UNCHANGED <<input, mode, admitted, sealed, candidateRoot,
                    candidateObservation, historyObjects, knownRoots,
                    currentRoot, generation, activationCount, rootObservations,
                    requestPublicationCount, previousHistory, previousCold,
                    previousKnown, previousCurrent, previousGeneration>>

SealImport ==
    /\ phase = "staged"
    /\ candidateRoot \in historyObjects \cap coldObjects
    /\ phase' = "sealed"
    /\ sealed' = TRUE
    /\ UNCHANGED <<input, mode, admitted, candidateRoot,
                    candidateObservation, historyObjects, coldObjects,
                    knownRoots, currentRoot, generation, activationCount,
                    rootObservations, requestPublicationCount,
                    previousHistory, previousCold, previousKnown,
                    previousCurrent, previousGeneration>>

PublishCheckpoint ==
    /\ phase = "accepted"
    /\ mode = "checkpoint"
    /\ generation < MaxGeneration
    /\ phase' = "published"
    /\ historyObjects' = historyObjects \cup {candidateRoot}
    /\ coldObjects' = coldObjects \cup {candidateRoot}
    /\ knownRoots' = knownRoots \cup {candidateRoot}
    /\ currentRoot' = candidateRoot
    /\ generation' = generation + 1
    /\ activationCount' = activationCount + 1
    /\ rootObservations' =
        [rootObservations EXCEPT ![candidateRoot] = candidateObservation]
    /\ requestPublicationCount' = 1
    /\ UNCHANGED <<input, mode, admitted, sealed, candidateRoot,
                    candidateObservation, previousHistory, previousCold,
                    previousKnown, previousCurrent, previousGeneration>>

PublishImport ==
    /\ phase = "sealed"
    /\ ImportMode(mode)
    /\ (mode = "import-preserve" \/ generation < MaxGeneration)
    /\ phase' = "published"
    /\ knownRoots' = knownRoots \cup {candidateRoot}
    /\ currentRoot' = IF mode = "import-activate"
                       THEN candidateRoot ELSE currentRoot
    /\ generation' = IF mode = "import-activate"
                     THEN generation + 1 ELSE generation
    /\ activationCount' = IF mode = "import-activate"
                          THEN activationCount + 1 ELSE activationCount
    /\ rootObservations' =
        [rootObservations EXCEPT ![candidateRoot] = candidateObservation]
    /\ requestPublicationCount' = 1
    /\ UNCHANGED <<input, mode, admitted, sealed, candidateRoot,
                    candidateObservation, historyObjects, coldObjects,
                    previousHistory, previousCold, previousKnown,
                    previousCurrent, previousGeneration>>

ActivateKnown(root) ==
    /\ phase \in {"idle", "rejected", "published"}
    /\ root \in knownRoots
    /\ generation < MaxGeneration
    /\ phase' = "idle"
    /\ currentRoot' = root
    /\ generation' = generation + 1
    /\ activationCount' = activationCount + 1
    /\ requestPublicationCount' = 0
    /\ UNCHANGED <<input, mode, admitted, sealed, candidateRoot,
                    candidateObservation, historyObjects, coldObjects,
                    knownRoots, rootObservations, previousHistory,
                    previousCold, previousKnown, previousCurrent,
                    previousGeneration>>

ActivateAnyKnown == \E root \in knownRoots: ActivateKnown(root)

BoundedHorizonDone ==
    /\ generation = MaxGeneration
    /\ phase \in {"idle", "rejected", "published"}
    /\ UNCHANGED vars

Next == BeginAny \/ RejectMalformed \/ AcceptCanonical
        \/ StageImportHistory \/ StageImportCold
        \/ SealImport \/ PublishCheckpoint \/ PublishImport
        \/ ActivateAnyKnown \/ BoundedHorizonDone
Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ phase \in Phases
    /\ input \in Inputs
    /\ mode \in Modes
    /\ admitted \in BOOLEAN
    /\ sealed \in BOOLEAN
    /\ candidateRoot \in Roots
    /\ candidateObservation \in Observations
    /\ historyObjects \subseteq Roots
    /\ coldObjects \subseteq Roots
    /\ knownRoots \subseteq Roots
    /\ currentRoot \in Roots
    /\ generation \in 0..MaxGeneration
    /\ activationCount \in Nat
    /\ rootObservations \in [Roots -> Observations]
    /\ requestPublicationCount \in 0..1
    /\ previousHistory \subseteq Roots
    /\ previousCold \subseteq Roots
    /\ previousKnown \subseteq Roots
    /\ previousCurrent \in Roots
    /\ previousGeneration \in 0..MaxGeneration

MalformedInputCannotBeAdmitted ==
    input = "malformed" =>
        phase \notin {"accepted", "history-staged", "staged",
                      "sealed", "published"}

MalformedRefusalPreservesEveryNamespace ==
    phase = "rejected" =>
        /\ historyObjects = previousHistory
        /\ coldObjects = previousCold
        /\ knownRoots = previousKnown
        /\ currentRoot = previousCurrent
        /\ generation = previousGeneration

UnpublishedImportHasNoRootAuthority ==
    ImportMode(mode) /\ phase \in {"accepted", "history-staged",
                                   "staged", "sealed"} =>
        /\ knownRoots = previousKnown
        /\ currentRoot = previousCurrent
        /\ generation = previousGeneration

UnpublishedCheckpointPreservesEveryNamespace ==
    mode = "checkpoint" /\ phase \in {"submitted", "accepted"} =>
        /\ historyObjects = previousHistory
        /\ coldObjects = previousCold
        /\ knownRoots = previousKnown
        /\ currentRoot = previousCurrent
        /\ generation = previousGeneration

PublicationRequiresAdmissionAndSeal ==
    phase = "published" =>
        /\ admitted
        /\ Canonical(input)
        /\ (ImportMode(mode) => sealed)
        /\ requestPublicationCount = 1

PublicationMatchesTypedOracle ==
    phase = "published" =>
        /\ candidateRoot = OracleRoot(input)
        /\ candidateObservation = OracleObservation(input)
        /\ candidateRoot \in knownRoots
        /\ rootObservations[candidateRoot] = OracleObservation(input)
        /\ (ActivatingMode(mode) => currentRoot = OracleRoot(input))

EveryKnownRootHasCompleteClosureAndExactObservation ==
    \A root \in knownRoots:
        /\ root \in historyObjects \cap coldObjects
        /\ rootObservations[root] = RootObservation(root)

CurrentRootIsKnown == currentRoot \in knownRoots
GenerationAdvancesOnlyOnActivation == generation = activationCount

AdmitMalformedMutant ==
    /\ phase = "submitted"
    /\ ~Canonical(input)
    /\ phase' = "accepted"
    /\ admitted' = TRUE
    /\ candidateRoot' = "first-root"
    /\ candidateObservation' = "first-observation"
    /\ UNCHANGED <<input, mode, sealed, historyObjects, coldObjects,
                    knownRoots, currentRoot, generation, activationCount,
                    rootObservations, requestPublicationCount,
                    previousHistory, previousCold, previousKnown,
                    previousCurrent, previousGeneration>>

PublishUnsealedImportMutant ==
    /\ phase = "staged"
    /\ ImportMode(mode)
    /\ (mode = "import-preserve" \/ generation < MaxGeneration)
    /\ phase' = "published"
    /\ knownRoots' = knownRoots \cup {candidateRoot}
    /\ currentRoot' = IF mode = "import-activate"
                       THEN candidateRoot ELSE currentRoot
    /\ generation' = IF mode = "import-activate"
                     THEN generation + 1 ELSE generation
    /\ activationCount' = IF mode = "import-activate"
                          THEN activationCount + 1 ELSE activationCount
    /\ rootObservations' =
        [rootObservations EXCEPT ![candidateRoot] = candidateObservation]
    /\ requestPublicationCount' = 1
    /\ UNCHANGED <<input, mode, admitted, sealed, candidateRoot,
                    candidateObservation, historyObjects, coldObjects,
                    previousHistory, previousCold, previousKnown,
                    previousCurrent, previousGeneration>>

PublishWrongObservationMutant ==
    /\ phase = "accepted"
    /\ mode = "checkpoint"
    /\ generation < MaxGeneration
    /\ phase' = "published"
    /\ historyObjects' = historyObjects \cup {candidateRoot}
    /\ coldObjects' = coldObjects \cup {candidateRoot}
    /\ knownRoots' = knownRoots \cup {candidateRoot}
    /\ currentRoot' = candidateRoot
    /\ generation' = generation + 1
    /\ activationCount' = activationCount + 1
    /\ rootObservations' =
        [rootObservations EXCEPT ![candidateRoot] = "none"]
    /\ requestPublicationCount' = 1
    /\ UNCHANGED <<input, mode, admitted, sealed, candidateRoot,
                    candidateObservation, previousHistory, previousCold,
                    previousKnown, previousCurrent, previousGeneration>>

MalformedAdmissionMutantSpec == Init /\ [][Next \/ AdmitMalformedMutant]_vars
UnsealedImportMutantSpec == Init /\ [][Next \/ PublishUnsealedImportMutant]_vars
WrongObservationMutantSpec == Init /\ [][Next \/ PublishWrongObservationMutant]_vars

=============================================================================
