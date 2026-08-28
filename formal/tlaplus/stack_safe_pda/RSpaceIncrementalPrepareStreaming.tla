---------------- MODULE RSpaceIncrementalPrepareStreaming ----------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

(***************************************************************************)
(* Formal-first refinement for bounded recovery of an ordinary kind-1 WAL  *)
(* PREPARE record. The contiguous oracle first obtains the complete byte    *)
(* image, gives I/O failure precedence over all parsing, gives the record   *)
(* checksum precedence over semantic failures, authenticates and binds the  *)
(* adjacent COMMIT, and only then exposes DATA/REUSE layouts to a private    *)
(* index builder. The streaming machine must produce the same outcome and   *)
(* identity without retaining a record-sized byte image.                    *)
(***************************************************************************)

CONSTANTS ChunkCount, RecordBytes, ScratchCapacity, ObjectCount, Mutation

ASSUME ChunkCount \in Nat \ {0}
ASSUME RecordBytes \in Nat \ {0}
ASSUME ScratchCapacity \in Nat \ {0}
ASSUME ScratchCapacity < RecordBytes
ASSUME ObjectCount \in Nat \ {0}

NoError == "none"
Errors == {
  NoError, "header", "resource", "pair", "read", "crc", "mutation",
  "object", "commit", "chain", "source", "transcript"
}
Outcomes == (Errors \ {NoError}) \cup {"success"}
Phases == {
  "Discriminator", "Header", "Preflight", "Pair", "Scan", "Trailer",
  "Commit", "Chain", "ReplayBytes", "ReplayLayout", "Done"
}

Scenario(name, headerValid, resourceCase, pairValid, readFault,
         crcValid, semanticFault, commitValid, chainValid, sourceStable) ==
  [name |-> name,
   headerValid |-> headerValid,
   resourceCase |-> resourceCase,
   pairValid |-> pairValid,
   readFault |-> readFault,
   crcValid |-> crcValid,
   semanticFault |-> semanticFault,
   commitValid |-> commitValid,
   chainValid |-> chainValid,
   sourceStable |-> sourceStable]

(***************************************************************************)
(* Single-fault scenarios establish independent rejection behavior. The two *)
(* combined scenarios make the observable precedence non-vacuous: read      *)
(* dominates CRC and CRC dominates semantic parsing.                        *)
(***************************************************************************)
Scenarios == {
  Scenario("success", TRUE, "exact", TRUE, 0, TRUE, NoError, TRUE, TRUE, TRUE),
  Scenario("header", FALSE, "exact", TRUE, 0, TRUE, NoError, TRUE, TRUE, TRUE),
  Scenario("resource", TRUE, "minus-one", TRUE, 0, TRUE, NoError, TRUE, TRUE, TRUE),
  Scenario("pair", TRUE, "exact", FALSE, 0, TRUE, NoError, TRUE, TRUE, TRUE),
  Scenario("read", TRUE, "exact", TRUE, 2, TRUE, NoError, TRUE, TRUE, TRUE),
  Scenario("crc", TRUE, "exact", TRUE, 0, FALSE, NoError, TRUE, TRUE, TRUE),
  Scenario("mutation", TRUE, "exact", TRUE, 0, TRUE, "mutation", TRUE, TRUE, TRUE),
  Scenario("object", TRUE, "exact", TRUE, 0, TRUE, "object", TRUE, TRUE, TRUE),
  Scenario("commit", TRUE, "exact", TRUE, 0, TRUE, NoError, FALSE, TRUE, TRUE),
  Scenario("chain", TRUE, "exact", TRUE, 0, TRUE, NoError, TRUE, FALSE, TRUE),
  Scenario("source", TRUE, "exact", TRUE, 0, TRUE, NoError, TRUE, TRUE, FALSE),
  Scenario("read-over-crc", TRUE, "exact", TRUE, 2, FALSE, NoError, TRUE, TRUE, TRUE),
  Scenario("crc-over-mutation", TRUE, "exact", TRUE, 0, FALSE, "mutation", TRUE, TRUE, TRUE)
}

ExpectedTranscript == [i \in 1..ChunkCount |-> i]
ExpectedLayouts == [i \in 1..ObjectCount |-> i]

OracleOutcome(s) ==
  IF ~s.headerValid THEN "header"
  ELSE IF s.resourceCase = "minus-one" THEN "resource"
  ELSE IF ~s.pairValid THEN "pair"
  ELSE IF s.readFault # 0 THEN "read"
  ELSE IF ~s.crcValid THEN "crc"
  ELSE IF s.semanticFault = "mutation" THEN "mutation"
  ELSE IF s.semanticFault = "object" THEN "object"
  ELSE IF ~s.commitValid THEN "commit"
  ELSE IF ~s.chainValid THEN "chain"
  ELSE IF ~s.sourceStable THEN "source"
  ELSE "success"

Min2(a, b) == IF a <= b THEN a ELSE b
PrefixOf(prefix, whole) ==
  /\ Len(prefix) <= Len(whole)
  /\ \A i \in 1..Len(prefix): prefix[i] = whole[i]

VARIABLES scenario, phase, cursor, authenticated, semantic, deferred,
          scratchUsed, recordBuffer, variableReads, identity, validated,
          commitBound, chainBound, firstTranscript, replayCursor,
          replayAuthenticated, replayTranscript, privateLayouts, publicLayouts, outcome

vars == <<scenario, phase, cursor, authenticated, semantic, deferred,
          scratchUsed, recordBuffer, variableReads, identity, validated,
          commitBound, chainBound, firstTranscript, replayCursor,
          replayAuthenticated, replayTranscript, privateLayouts, publicLayouts, outcome>>

Init ==
  /\ scenario \in Scenarios
  /\ phase = "Discriminator"
  /\ cursor = 0
  /\ authenticated = <<>>
  /\ semantic = <<>>
  /\ deferred = NoError
  /\ scratchUsed = 0
  /\ recordBuffer = 0
  /\ variableReads = 0
  /\ identity = "none"
  /\ validated = FALSE
  /\ commitBound = FALSE
  /\ chainBound = FALSE
  /\ firstTranscript = <<>>
  /\ replayCursor = 0
  /\ replayAuthenticated = <<>>
  /\ replayTranscript = <<>>
  /\ privateLayouts = <<>>
  /\ publicLayouts = <<>>
  /\ outcome = NoError

ReadDiscriminator ==
  /\ phase = "Discriminator"
  /\ phase' = "Header"
  /\ UNCHANGED <<scenario, cursor, authenticated, semantic, deferred,
                  scratchUsed, recordBuffer, variableReads, identity,
                  validated, commitBound, chainBound, firstTranscript, replayCursor,
                  replayAuthenticated, replayTranscript, privateLayouts, publicLayouts, outcome>>

AuthenticateHeader ==
  /\ phase = "Header"
  /\ IF scenario.headerValid
        THEN /\ phase' = "Preflight"
             /\ outcome' = outcome
        ELSE /\ phase' = "Done"
             /\ outcome' = "header"
  /\ UNCHANGED <<scenario, cursor, authenticated, semantic, deferred,
                  scratchUsed, recordBuffer, variableReads, identity,
                  validated, commitBound, chainBound, firstTranscript, replayCursor,
                  replayAuthenticated, replayTranscript, privateLayouts, publicLayouts>>

ResourceAdmitted ==
  IF Mutation = "AcceptMinusOne" THEN TRUE
  ELSE IF Mutation = "RejectExact" THEN FALSE
  ELSE scenario.resourceCase = "exact"

Preflight ==
  /\ phase = "Preflight"
  /\ recordBuffer' = IF Mutation = "WholeRecordBuffer" THEN RecordBytes ELSE 0
  /\ variableReads' =
       IF Mutation = "ReadBeforeAdmission" THEN variableReads + 1 ELSE variableReads
  /\ IF ResourceAdmitted
        THEN /\ phase' = "Pair"
             /\ outcome' = outcome
        ELSE /\ phase' = "Done"
             /\ outcome' = "resource"
  /\ UNCHANGED <<scenario, cursor, authenticated, semantic, deferred,
                  scratchUsed, identity, validated, commitBound, chainBound,
                  firstTranscript, replayCursor, replayAuthenticated, replayTranscript, privateLayouts,
                  publicLayouts>>

CheckPair ==
  /\ phase = "Pair"
  /\ IF scenario.pairValid
        THEN /\ phase' = "Scan"
             /\ outcome' = outcome
        ELSE /\ phase' = "Done"
             /\ outcome' = "pair"
  /\ UNCHANGED <<scenario, cursor, authenticated, semantic, deferred,
                  scratchUsed, recordBuffer, variableReads, identity,
                  validated, commitBound, chainBound, firstTranscript, replayCursor,
                  replayAuthenticated, replayTranscript, privateLayouts, publicLayouts>>

SemanticFaultAt(next) ==
  IF scenario.semanticFault = "mutation" /\ next = 1 THEN "mutation"
  ELSE IF scenario.semanticFault = "object" /\ next = ChunkCount THEN "object"
  ELSE NoError

ScanChunk ==
  /\ phase = "Scan"
  /\ cursor < ChunkCount
  /\ LET next == cursor + 1 IN
       IF scenario.readFault = next
       THEN /\ phase' = "Done"
            /\ outcome' = "read"
            /\ UNCHANGED <<cursor, authenticated, semantic, deferred,
                            scratchUsed, identity, validated, commitBound, chainBound,
                            firstTranscript, replayCursor, replayAuthenticated, replayTranscript, privateLayouts,
                            publicLayouts>>
       ELSE /\ cursor' = IF Mutation = "CursorOffByOne" THEN cursor + 2 ELSE next
            /\ authenticated' =
                 IF Mutation = "AcceptIncompleteTranscript" /\ next = ChunkCount
                 THEN authenticated ELSE Append(authenticated, next)
            /\ semantic' =
                 IF deferred = NoError THEN Append(semantic, next) ELSE semantic
            /\ deferred' =
                 IF deferred # NoError THEN deferred ELSE SemanticFaultAt(next)
            /\ scratchUsed' =
                 IF Mutation = "ScratchIsRecordSized" THEN RecordBytes
                 ELSE Min2(ScratchCapacity, RecordBytes)
            /\ phase' = IF next = ChunkCount THEN "Trailer" ELSE "Scan"
            /\ outcome' = outcome
            /\ publicLayouts' =
                 IF Mutation = "EarlyIndexExposure" /\ next = 1
                 THEN <<1>> ELSE publicLayouts
            /\ UNCHANGED <<identity, validated, commitBound, chainBound, firstTranscript,
                            replayCursor, replayAuthenticated, replayTranscript, privateLayouts>>
  /\ variableReads' = variableReads + 1
  /\ UNCHANGED <<scenario, recordBuffer>>

FinishTrailer ==
  /\ phase = "Trailer"
  /\ firstTranscript' = authenticated
  /\ IF Mutation = "SemanticBeforeCrc" /\ deferred # NoError
        THEN /\ phase' = "Done"
             /\ outcome' = deferred
             /\ identity' = "none"
             /\ validated' = FALSE
        ELSE IF ~scenario.crcValid
        THEN /\ phase' = "Done"
             /\ outcome' = "crc"
             /\ identity' = "none"
             /\ validated' = FALSE
        ELSE IF deferred # NoError
        THEN /\ phase' = "Done"
             /\ outcome' = IF Mutation = "DropSemanticError" THEN "success" ELSE deferred
             /\ identity' = IF Mutation = "DropSemanticError" THEN "exact" ELSE "none"
             /\ validated' = (Mutation = "DropSemanticError")
        ELSE IF authenticated # ExpectedTranscript /\ Mutation # "AcceptIncompleteTranscript"
        THEN /\ phase' = "Done"
             /\ outcome' = "transcript"
             /\ identity' = "none"
             /\ validated' = FALSE
        ELSE /\ phase' = "Commit"
             /\ outcome' = outcome
             /\ identity' = IF Mutation = "WrongIdentity" THEN "wrong" ELSE "exact"
             /\ validated' = TRUE
  /\ UNCHANGED <<scenario, cursor, authenticated, semantic, deferred,
                  scratchUsed, recordBuffer, variableReads, commitBound, chainBound,
                  replayCursor, replayAuthenticated, replayTranscript, privateLayouts, publicLayouts>>

BindCommit ==
  /\ phase = "Commit"
  /\ IF scenario.commitValid
        THEN /\ phase' = "Chain"
             /\ outcome' = outcome
             /\ commitBound' = (Mutation # "SkipCommitBinding")
        ELSE /\ phase' = "Done"
             /\ outcome' = "commit"
             /\ commitBound' = FALSE
  /\ UNCHANGED <<scenario, cursor, authenticated, semantic, deferred,
                  scratchUsed, recordBuffer, variableReads, identity, validated,
                  chainBound, firstTranscript, replayCursor, replayAuthenticated,
                  replayTranscript, privateLayouts, publicLayouts>>

BindChain ==
  /\ phase = "Chain"
  /\ IF scenario.chainValid
        THEN /\ phase' = "ReplayBytes"
             /\ outcome' = outcome
             /\ chainBound' = (Mutation # "SkipChainBinding")
        ELSE /\ phase' = "Done"
             /\ outcome' = "chain"
             /\ chainBound' = FALSE
  /\ UNCHANGED <<scenario, cursor, authenticated, semantic, deferred,
                  scratchUsed, recordBuffer, variableReads, identity, validated,
                  commitBound, firstTranscript, replayCursor, replayAuthenticated, replayTranscript,
                  privateLayouts, publicLayouts>>

ReplaySourceChunk ==
  /\ phase = "ReplayBytes"
  /\ replayCursor < ChunkCount
  /\ LET next == replayCursor + 1 IN
       /\ replayCursor' = next
       /\ replayAuthenticated' = Append(replayAuthenticated, next)
       /\ IF next = ChunkCount
             THEN IF scenario.sourceStable \/ Mutation = "AcceptUnstableSource"
                  THEN /\ phase' = "ReplayLayout"
                       /\ outcome' = outcome
                  ELSE /\ phase' = "Done"
                       /\ outcome' = "source"
             ELSE /\ phase' = "ReplayBytes"
                  /\ outcome' = outcome
  /\ variableReads' = variableReads + 1
  /\ UNCHANGED <<scenario, cursor, authenticated, semantic, deferred,
                  scratchUsed, recordBuffer, identity, validated, commitBound,
                  chainBound, firstTranscript, replayTranscript, privateLayouts,
                  publicLayouts>>

ReplayLayout ==
  /\ phase = "ReplayLayout"
  /\ Len(privateLayouts) < ObjectCount
  /\ LET next == Len(privateLayouts) + 1 IN
       /\ privateLayouts' =
            IF Mutation = "SwapReplayLayouts" /\ next = ObjectCount
            THEN Append(privateLayouts, 1) ELSE Append(privateLayouts, next)
       /\ replayTranscript' =
            IF Mutation = "SkipReplayTranscript" /\ next = ObjectCount
            THEN replayTranscript ELSE Append(replayTranscript, next)
       /\ IF next = ObjectCount
             THEN IF scenario.sourceStable \/ Mutation = "AcceptUnstableSource"
                  THEN /\ phase' = "Done"
                       /\ outcome' = "success"
                  ELSE /\ phase' = "Done"
                       /\ outcome' = "source"
             ELSE /\ phase' = "ReplayLayout"
                  /\ outcome' = outcome
  /\ UNCHANGED <<scenario, cursor, authenticated, semantic, deferred,
                  scratchUsed, recordBuffer, variableReads, identity, validated,
                  commitBound, chainBound, firstTranscript, replayCursor,
                  replayAuthenticated, publicLayouts>>

Done ==
  /\ phase = "Done"
  /\ UNCHANGED vars

Next == ReadDiscriminator \/ AuthenticateHeader \/ Preflight \/ CheckPair \/
        ScanChunk \/ FinishTrailer \/ BindCommit \/ BindChain \/ ReplaySourceChunk \/
        ReplayLayout \/ Done

Spec == Init /\ [][Next]_vars

TypeInvariant ==
  /\ scenario \in Scenarios
  /\ phase \in Phases
  /\ cursor \in 0..(ChunkCount + 1)
  /\ authenticated \in Seq(1..ChunkCount)
  /\ semantic \in Seq(1..ChunkCount)
  /\ deferred \in Errors
  /\ scratchUsed \in 0..RecordBytes
  /\ recordBuffer \in 0..RecordBytes
  /\ variableReads \in Nat
  /\ identity \in {"none", "exact", "wrong"}
  /\ validated \in BOOLEAN
  /\ commitBound \in BOOLEAN
  /\ chainBound \in BOOLEAN
  /\ firstTranscript \in Seq(1..ChunkCount)
  /\ replayCursor \in 0..ChunkCount
  /\ replayAuthenticated \in Seq(1..ChunkCount)
  /\ replayTranscript \in Seq(1..ObjectCount)
  /\ privateLayouts \in Seq(1..ObjectCount)
  /\ publicLayouts \in Seq(1..ObjectCount)
  /\ outcome \in Errors \cup {"success"}

CursorProgressIsExact == cursor <= ChunkCount
ScratchIsBounded == scratchUsed <= ScratchCapacity
NoWholeRecordBuffer == recordBuffer = 0
VariableReadsFollowAdmission ==
  variableReads > 0 => scenario.headerValid /\ ResourceAdmitted /\ scenario.pairValid
AuthenticationTranscriptIsExact ==
  validated => authenticated = ExpectedTranscript
SemanticTranscriptIsAPrefix == PrefixOf(semantic, ExpectedTranscript)
CrcDominatesSemanticError ==
  phase = "Done" /\ ~scenario.crcValid /\ scenario.readFault = 0 /\
  scenario.headerValid /\ ResourceAdmitted /\ scenario.pairValid
    => outcome = "crc"
NoIndexExposureBeforeAuthentication == publicLayouts = <<>>
NoPrivateIndexBeforeCommitBinding == Len(privateLayouts) > 0 => validated /\ commitBound
NoPrivateIndexBeforeChainBinding == Len(privateLayouts) > 0 => chainBound
CommitBindsExactIdentity == commitBound => identity = "exact"
ReplayTranscriptMatchesFirstPass ==
  outcome = "success" =>
    replayAuthenticated = firstTranscript /\ replayTranscript = ExpectedLayouts /\
    privateLayouts = ExpectedLayouts
FullSourceReplayPrecedesPrivateIndex ==
  Len(privateLayouts) > 0 =>
    replayCursor = ChunkCount /\ replayAuthenticated = firstTranscript
StableSourceRequiredForSuccess == outcome = "success" => scenario.sourceStable
AcceptedIdentityIsExact ==
  outcome = "success" => identity = "exact" /\ commitBound /\ chainBound
OutcomeRefinesContiguousOracle == phase = "Done" => outcome = OracleOutcome(scenario)
MinusOneIsRejectedBeforeVariableRead ==
  scenario.resourceCase = "minus-one" /\ phase = "Done"
    => outcome = "resource" /\ variableReads = 0
ExactBoundaryIsNotRejected ==
  scenario.resourceCase = "exact" /\ phase = "Done" => outcome # "resource"

NeverSuccess == outcome # "success"
NeverHeaderRejection == outcome # "header"
NeverResourceRejection == outcome # "resource"
NeverPairRejection == outcome # "pair"
NeverReadRejection == outcome # "read"
NeverCrcRejection == outcome # "crc"
NeverMutationRejection == outcome # "mutation"
NeverObjectRejection == outcome # "object"
NeverCommitRejection == outcome # "commit"
NeverChainRejection == outcome # "chain"
NeverSourceRejection == outcome # "source"

=============================================================================
