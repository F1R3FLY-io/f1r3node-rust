------------------------ MODULE CarrierIndexSoundness ------------------------
EXTENDS FiniteSets, Integers, Naturals

CONSTANTS
    \* @type: Bool;
    TypedKeys,
    \* @type: Bool;
    AtomicAdmission,
    \* @type: Bool;
    PruneGate,
    \* @type: Bool;
    StoredBodyGate,
    \* @type: Bool;
    CompleteWatermarkDomain

ASSUME TypedKeys \in BOOLEAN
ASSUME AtomicAdmission \in BOOLEAN
ASSUME PruneGate \in BOOLEAN
ASSUME StoredBodyGate \in BOOLEAN
ASSUME CompleteWatermarkDomain \in BOOLEAN

Validators == {"v1", "v2"}
Blocks == {"legacy0", "v6one", "invalidTwo", "legacyThree"}
PreExisting == {"legacy0", "invalidTwo"}
Domains == {"legacy", "v6"}
Payloads == {"same", "other"}
\* @type: (Str, Str) => <<Str, Str>>;
Id(domain, payload) == <<domain, payload>>
\* @type: Set(<<Str, Str>>);
Identities == {Id(domain, payload) : domain \in Domains, payload \in Payloads}
\* @type: <<Str, Str>>;
NoCache == <<"none", "none">>
CacheValues == Identities \union {NoCache}
BlockStates == {"absent", "staged", "published"}
Results == {"fresh", "duplicate", "unknown"}
ScanStarts == 0..4

BlockHeight ==
    [block \in Blocks |->
        CASE block = "legacy0" -> 0
          [] block = "v6one" -> 1
          [] block = "invalidTwo" -> 2
          [] OTHER -> 3]

BlockIdentity ==
    [block \in Blocks |->
        CASE block = "legacy0" -> Id("legacy", "same")
          [] block = "v6one" -> Id("v6", "same")
          [] block = "invalidTwo" -> Id("v6", "other")
          [] OTHER -> Id("legacy", "other")]

BlockValid == [block \in Blocks |-> block /= "invalidTwo"]

\* @type: <<Str, Str>> => <<Str, Str>>;
Key(identity) ==
    IF TypedKeys
    THEN identity
    ELSE <<"raw", identity[2]>>

VARIABLES
    \* @type: Str -> Str;
    blockState,
    \* @type: Set(<<Str, Str, Str>>);
    carrierRows,
    \* @type: Set(Str);
    durableBodies,
    \* @type: Str -> (Str -> <<Str, Str>>);
    cache,
    \* @type: Int;
    watermark,
    \* @type: Str -> Int;
    watermarkRead,
    \* @type: Set(Str);
    watermarkWinners,
    \* @type: Int;
    pruneCutoff

vars ==
    <<blockState, carrierRows, durableBodies, cache,
      watermark, watermarkRead, watermarkWinners, pruneCutoff>>

Published == {block \in Blocks : blockState[block] = "published"}

MaxPublishedHeight ==
    LET domain ==
          {block \in Published : CompleteWatermarkDomain \/ BlockValid[block]}
    IN CHOOSE height \in {BlockHeight[block] : block \in domain} :
         \A other \in {BlockHeight[block] : block \in domain} : other <= height

CarrierRecorded(block) ==
    <<BlockIdentity[block][1], BlockIdentity[block][2], block>> \in carrierRows

LookupBlocks(identity) ==
    {block \in Blocks :
        \E row \in carrierRows :
            /\ row[3] = block
            /\ Key(<<row[1], row[2]>>) = Key(identity)}

IndexAbsent(identity) == LookupBlocks(identity) = {}

BodyKnown(validator, block) ==
    block \in durableBodies \/
      (~StoredBodyGate /\ cache[validator][block] /= NoCache)

OperationalIdentity(validator, block) ==
    IF cache[validator][block] /= NoCache
    THEN cache[validator][block]
    ELSE BlockIdentity[block]

Relevant(scope, start) ==
    {block \in scope : BlockHeight[block] >= start}

TruthDuplicate(scope, identity, start) ==
    \E block \in Relevant(scope, start) : BlockIdentity[block] = identity

ExactKnown(validator, scope, start) ==
    \A block \in Relevant(scope, start) : BodyKnown(validator, block)

ExactDuplicate(validator, scope, identity, start) ==
    \E block \in Relevant(scope, start) :
        /\ BodyKnown(validator, block)
        /\ Key(OperationalIdentity(validator, block)) = Key(identity)

ExactResult(validator, scope, identity, start) ==
    IF ~ExactKnown(validator, scope, start)
    THEN "unknown"
    ELSE IF ExactDuplicate(validator, scope, identity, start)
         THEN "duplicate"
         ELSE "fresh"

FastEligible(start) ==
    /\ watermark /= -1
    /\ watermark <= start
    /\ IF PruneGate THEN pruneCutoff <= start ELSE TRUE

FastResult(validator, scope, identity, start) ==
    IF FastEligible(start) /\ IndexAbsent(identity)
    THEN "fresh"
    ELSE ExactResult(validator, scope, identity, start)

TruthResult(scope, identity, start) ==
    IF TruthDuplicate(scope, identity, start)
    THEN "duplicate"
    ELSE "fresh"

Init ==
    /\ blockState =
        [block \in Blocks |-> IF block \in PreExisting THEN "published" ELSE "absent"]
    /\ carrierRows = {}
    /\ durableBodies = PreExisting
    /\ cache = [validator \in Validators |-> [block \in Blocks |-> NoCache]]
    /\ watermark = -1
    /\ watermarkRead = [validator \in Validators |-> -1]
    /\ watermarkWinners = {}
    /\ pruneCutoff = 0

ReadWatermark(validator) ==
    /\ validator \in Validators
    /\ watermark = -1
    /\ watermarkRead[validator] = -1
    /\ watermarkRead' =
        [watermarkRead EXCEPT ![validator] = MaxPublishedHeight + 1]
    /\ UNCHANGED
        <<blockState, carrierRows, durableBodies, cache,
          watermark, watermarkWinners, pruneCutoff>>

CasWatermark(validator) ==
    /\ validator \in Validators
    /\ watermarkRead[validator] /= -1
    /\ IF watermark = -1
       THEN /\ watermark' = watermarkRead[validator]
            /\ watermarkWinners' = watermarkWinners \union {validator}
       ELSE /\ UNCHANGED watermark
            /\ UNCHANGED watermarkWinners
    /\ watermarkRead' = [watermarkRead EXCEPT ![validator] = -1]
    /\ UNCHANGED
        <<blockState, carrierRows, durableBodies, cache, pruneCutoff>>

Admit(validator, block) ==
    /\ validator \in Validators
    /\ block \in Blocks \ PreExisting
    /\ blockState[block] = "absent"
    /\ blockState' =
        [blockState EXCEPT ![block] =
          IF AtomicAdmission /\ BlockIdentity[block][1] = "legacy"
          THEN "staged"
          ELSE "published"]
    /\ durableBodies' = durableBodies \union {block}
    /\ IF AtomicAdmission
       THEN carrierRows' =
            carrierRows \union
              {<<BlockIdentity[block][1], BlockIdentity[block][2], block>>}
       ELSE UNCHANGED carrierRows
    /\ UNCHANGED <<cache, watermark, watermarkRead, watermarkWinners, pruneCutoff>>

PublishStaged(validator, block) ==
    /\ validator \in Validators
    /\ block \in Blocks
    /\ blockState[block] = "staged"
    /\ CarrierRecorded(block)
    /\ blockState' = [blockState EXCEPT ![block] = "published"]
    /\ UNCHANGED
        <<carrierRows, durableBodies, cache, watermark,
          watermarkRead, watermarkWinners, pruneCutoff>>

FinishCarrier(block) ==
    /\ ~AtomicAdmission
    /\ block \in Blocks \ PreExisting
    /\ blockState[block] = "published"
    /\ ~CarrierRecorded(block)
    /\ carrierRows' =
        carrierRows \union
          {<<BlockIdentity[block][1], BlockIdentity[block][2], block>>}
    /\ UNCHANGED
        <<blockState, durableBodies, cache, watermark,
          watermarkRead, watermarkWinners, pruneCutoff>>

CacheBody(validator, block) ==
    /\ validator \in Validators
    /\ block = "legacy0"
    /\ block \in Published \cap durableBodies
    /\ cache[validator][block] = NoCache
    /\ cache' = [cache EXCEPT ![validator][block] = BlockIdentity[block]]
    /\ UNCHANGED
        <<blockState, carrierRows, durableBodies, watermark,
          watermarkRead, watermarkWinners, pruneCutoff>>

LoseBody(block) ==
    /\ block \in durableBodies
    /\ durableBodies' = durableBodies \ {block}
    /\ UNCHANGED
        <<blockState, carrierRows, cache, watermark,
          watermarkRead, watermarkWinners, pruneCutoff>>

Prune(newCutoff) ==
    /\ newCutoff \in ScanStarts
    /\ newCutoff > pruneCutoff
    /\ pruneCutoff' = newCutoff
    /\ carrierRows' =
        {row \in carrierRows : BlockHeight[row[3]] >= newCutoff}
    /\ UNCHANGED
        <<blockState, durableBodies, cache, watermark,
          watermarkRead, watermarkWinners>>

Idle == UNCHANGED vars

Next ==
    \/ \E validator \in Validators : ReadWatermark(validator)
    \/ \E validator \in Validators : CasWatermark(validator)
    \/ \E validator \in Validators, block \in Blocks : Admit(validator, block)
    \/ \E validator \in Validators, block \in Blocks : PublishStaged(validator, block)
    \/ \E block \in Blocks : FinishCarrier(block)
    \/ \E validator \in Validators, block \in Blocks : CacheBody(validator, block)
    \/ \E block \in Blocks : LoseBody(block)
    \/ \E newCutoff \in ScanStarts : Prune(newCutoff)
    \/ Idle

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ blockState \in [Blocks -> BlockStates]
    /\ carrierRows \subseteq
        {<<identity[1], identity[2], block>> :
            identity \in Identities, block \in Blocks}
    /\ durableBodies \subseteq Blocks
    /\ cache \in [Validators -> [Blocks -> CacheValues]]
    /\ watermark \in {-1} \union ScanStarts
    /\ watermarkRead \in [Validators -> {-1} \union ScanStarts]
    /\ watermarkWinners \subseteq Validators
    /\ pruneCutoff \in ScanStarts

Inv_TypedDomainsAreDisjoint ==
    Key(Id("legacy", "same")) /= Key(Id("v6", "same"))

Inv_CacheFactsAreAuthentic ==
    \A validator \in Validators, block \in Blocks :
        cache[validator][block] /= NoCache =>
          cache[validator][block] = BlockIdentity[block]

Inv_MissingBodyIsUnknown ==
    \A validator \in Validators :
      \A scope \in SUBSET Published, start \in ScanStarts :
        (\E block \in Relevant(scope, start) : block \notin durableBodies) =>
          \A identity \in Identities :
            ExactResult(validator, scope, identity, start) = "unknown"

Inv_OneWatermarkCasWins ==
    /\ Cardinality(watermarkWinners) <= 1
    /\ (watermark = -1) = (watermarkWinners = {})

Inv_WatermarkCoversPreexistingDomain ==
    watermark /= -1 =>
      \A block \in PreExisting : BlockHeight[block] < watermark

Inv_WatermarkCoverage ==
    watermark /= -1 =>
      \A block \in Published :
        BlockHeight[block] >= watermark /\ BlockHeight[block] >= pruneCutoff =>
          CarrierRecorded(block)

Inv_PruneRetainsFutureCoverage ==
    watermark /= -1 =>
      \A block \in Published :
        BlockHeight[block] >= watermark /\ BlockHeight[block] >= pruneCutoff =>
          CarrierRecorded(block)

Inv_ExactScanUsesTypedIdentity ==
    \A validator \in Validators :
      \A scope \in SUBSET Published,
         identity \in Identities,
         start \in ScanStarts :
        ExactKnown(validator, scope, start) =>
          ExactResult(validator, scope, identity, start) =
            TruthResult(scope, identity, start)

Inv_IndexAbsenceIsSound ==
    \A validator \in Validators :
      \A scope \in SUBSET Published,
         identity \in Identities,
         start \in ScanStarts :
        FastEligible(start) /\ IndexAbsent(identity) =>
          ~TruthDuplicate(scope, identity, start)

Inv_FastPathIsSound ==
    \A validator \in Validators :
      \A scope \in SUBSET Published,
         identity \in Identities,
         start \in ScanStarts :
        FastResult(validator, scope, identity, start) /= "unknown" =>
          FastResult(validator, scope, identity, start) =
            TruthResult(scope, identity, start)

Inv_FastEqualsExactWhenExactIsKnown ==
    \A validator \in Validators :
      \A scope \in SUBSET Published,
         identity \in Identities,
         start \in ScanStarts :
        ExactKnown(validator, scope, start) =>
          FastResult(validator, scope, identity, start) =
            ExactResult(validator, scope, identity, start)

Inv_ParallelValidatorsAgreeWhenDecisive ==
    \A left \in Validators, right \in Validators :
      \A scope \in SUBSET Published,
         identity \in Identities,
         start \in ScanStarts :
        FastResult(left, scope, identity, start) /= "unknown" /\
        FastResult(right, scope, identity, start) /= "unknown" =>
          FastResult(left, scope, identity, start) =
            FastResult(right, scope, identity, start)
=============================================================================
