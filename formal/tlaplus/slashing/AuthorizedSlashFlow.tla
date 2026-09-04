------------------------ MODULE AuthorizedSlashFlow ------------------------
EXTENDS Integers, FiniteSets, TLC

CONSTANT
    \* @type: Set(Str);
    Validators,
    \* @type: Set(Str);
    Hashes,
    \* @type: Set(Str);
    Epochs,
    \* @type: Str -> Int;
    HashRank,
    \* @type: Str -> Int;
    InitialBonds,
    \* @type: Int;
    MaxGeneration,
    \* @type: Bool;
    ProposerUsesCanonicalPreState,
    \* @type: Bool;
    ReceiverUsesCanonicalPreState,
    \* @type: Bool;
    UseGenerationInAuthorization,
    \* @type: Bool;
    UseActivationWindow,
    \* @type: Bool;
    ReuseGenerationOnFreshBond,
    \* @type: Bool;
    ChangeGenerationOnEpochAdvance,
    \* @type: Bool;
    AllowGenerationChangeOutsideBond,
    \* @type: Bool;
    ReplayUsesCommittedGeneration,
    \* @type: Bool;
    CanonicalAuthorityAtomic,
    \* @type: Bool;
    AllowDetachedCanonicalPreState

Bonded == "Bonded"
PendingWithdraw == "PendingWithdraw"
Withdrawing == "Withdrawing"
Withdrawn == "Withdrawn"
Quarantined == "Quarantined"
Phases == {Bonded, PendingWithdraw, Withdrawing, Withdrawn, Quarantined}
GenerationValues == 0..MaxGeneration
BondValues == {0} \cup {InitialBonds[v] : v \in Validators}

VARIABLE
    \* @type: Str -> Int;
    bonds,
    \* @type: Str -> Int;
    ambientBonds,
    \* @type: Str -> Int;
    canonicalBonds,
    \* @type: Str -> Int;
    bondGeneration,
    \* @type: Str -> Int;
    ambientGeneration,
    \* @type: Str -> Int;
    canonicalGeneration,
    \* @type: Str -> Str;
    phase,
    \* @type: Str -> Set(Int);
    liveGenerations,
    \* @type: Str -> Int;
    successfulBonds,
    \* @type: Set(<<Str, Str, Int, Str>>);
    evidence,
    \* @type: Set(<<Str, Int, Str, Str>>);
    pendingSlashDeploys,
    \* @type: Set(<<Str, Int>>);
    slashedLifetimes,
    \* @type: Str;
    epoch,
    \* @type: Set(<<Str, Int, Str, Str>>);
    rejectedSlashDeploys,
    \* @type: Set(<<Str, Int, Str, Str>>);
    mergeRejectedSlashDeploys,
    \* @type: Bool;
    badAuthObserved,
    \* @type: Bool;
    generationChangedOutsideBond,
    \* @type: Bool;
    replayAcceptedStale,
    \* @type: Bool;
    rebondReached

vars ==
    <<bonds, ambientBonds, canonicalBonds, bondGeneration,
      ambientGeneration, canonicalGeneration, phase, liveGenerations,
      successfulBonds, evidence, pendingSlashDeploys, slashedLifetimes,
      epoch, rejectedSlashDeploys, mergeRejectedSlashDeploys,
      badAuthObserved, generationChangedOutsideBond, replayAcceptedStale,
      rebondReached>>

Evidence == Hashes \X Validators \X GenerationValues \X Epochs
SlashDeploy == Validators \X GenerationValues \X Epochs \X Hashes
Lifetime == Validators \X GenerationValues

\* @type: (Str, Str) => Bool;
ActivationMatches(evidenceEpoch, currentEpoch) ==
    ~UseActivationWindow \/ evidenceEpoch = currentEpoch

\* @type: (Str -> Int, Str, Int) => Bool;
GenerationMatches(generationView, validator, targetGeneration) ==
    ~UseGenerationInAuthorization
    \/ generationView[validator] = targetGeneration

\* @type: (Str, Int, Str, Str) => Bool;
AuthEvidence(v, g, e, h) ==
    /\ \E evidenceEpoch \in Epochs :
         /\ <<h, v, g, evidenceEpoch>> \in evidence
         /\ ActivationMatches(evidenceEpoch, e)
    /\ GenerationMatches(canonicalGeneration, v, g)

\* @type: (Str -> Int, Str -> Int, Str, Int, Str, Str) => Bool;
AuthorizedForView(bondView, generationView, v, g, e, h) ==
    /\ \E evidenceEpoch \in Epochs :
         /\ <<h, v, g, evidenceEpoch>> \in evidence
         /\ ActivationMatches(evidenceEpoch, e)
    /\ ActivationMatches(e, epoch)
    /\ GenerationMatches(generationView, v, g)
    /\ bondView[v] > 0

\* @type: (Str, Int, Str, Str) => Bool;
Authorized(v, g, e, h) ==
    AuthorizedForView(canonicalBonds, canonicalGeneration, v, g, e, h)

ProposerAuthorityBonds ==
    IF ProposerUsesCanonicalPreState THEN canonicalBonds ELSE ambientBonds

ProposerAuthorityGenerations ==
    IF ProposerUsesCanonicalPreState
    THEN canonicalGeneration
    ELSE ambientGeneration

ReceiverAuthorityBonds ==
    IF ReceiverUsesCanonicalPreState THEN canonicalBonds ELSE ambientBonds

ReceiverAuthorityGenerations ==
    IF ReceiverUsesCanonicalPreState
    THEN canonicalGeneration
    ELSE ambientGeneration

\* @type: (Str, Int, Str, Str) => Bool;
ProposerAuthorized(v, g, e, h) ==
    AuthorizedForView(
        ProposerAuthorityBonds,
        ProposerAuthorityGenerations,
        v, g, e, h)

\* @type: (Str, Int, Str, Str) => Bool;
ReceiverAuthorized(v, g, e, h) ==
    AuthorizedForView(
        ReceiverAuthorityBonds,
        ReceiverAuthorityGenerations,
        v, g, e, h)

\* @type: (Set(<<Str, Str, Int, Str>>), Str, Int, Str) => Set(Str);
EvidenceHashesFor(evs, v, g, e) ==
    {h \in Hashes :
      \E evidenceEpoch \in Epochs :
        /\ <<h, v, g, evidenceEpoch>> \in evs
        /\ ActivationMatches(evidenceEpoch, e)}

\* @type: (Set(<<Str, Str, Int, Str>>), Str, Int, Str) => Str;
CanonicalEvidenceHash(evs, v, g, e) ==
    CHOOSE h \in EvidenceHashesFor(evs, v, g, e) :
      \A other \in EvidenceHashesFor(evs, v, g, e) :
        HashRank[h] <= HashRank[other]

\* @type: (Str -> Int, Str -> Int, Set(<<Str, Str, Int, Str>>), Str) => Set(<<Str, Int>>);
AuthorizedTargetsForView(bondView, generationView, evs, currentEpoch) ==
    {<<v, g>> \in Validators \X GenerationValues :
        /\ EvidenceHashesFor(evs, v, g, currentEpoch) # {}
        /\ ActivationMatches(currentEpoch, currentEpoch)
        /\ GenerationMatches(generationView, v, g)
        /\ bondView[v] > 0}

\* @type: (Str -> Int, Str -> Int, Set(<<Str, Str, Int, Str>>), Str) => Set(<<Str, Int, Str, Str>>);
AuthorizedDeploysForView(bondView, generationView, evs, currentEpoch) ==
    {<<target[1], target[2], currentEpoch,
       CanonicalEvidenceHash(evs, target[1], target[2], currentEpoch)>> :
        target \in AuthorizedTargetsForView(
            bondView, generationView, evs, currentEpoch)}

\* @type: (Str, Int) => Bool;
PendingCoversTarget(v, g) ==
    \E deploy \in pendingSlashDeploys :
        deploy[1] = v /\ deploy[2] = g

\* @type: Str => Bool;
HashUnused(h) ==
    \A item \in evidence : item[1] # h

TypeOK ==
    /\ bonds \in [Validators -> Nat]
    /\ ambientBonds \in [Validators -> Nat]
    /\ canonicalBonds \in [Validators -> Nat]
    /\ bondGeneration \in [Validators -> GenerationValues]
    /\ ambientGeneration \in [Validators -> GenerationValues]
    /\ canonicalGeneration \in [Validators -> GenerationValues]
    /\ phase \in [Validators -> Phases]
    /\ liveGenerations \in [Validators -> SUBSET GenerationValues]
    /\ successfulBonds \in [Validators -> Nat]
    /\ evidence \in SUBSET Evidence
    /\ pendingSlashDeploys \in SUBSET SlashDeploy
    /\ slashedLifetimes \in SUBSET Lifetime
    /\ epoch \in Epochs
    /\ rejectedSlashDeploys \in SUBSET SlashDeploy
    /\ mergeRejectedSlashDeploys \in SUBSET SlashDeploy
    /\ badAuthObserved \in BOOLEAN
    /\ generationChangedOutsideBond \in BOOLEAN
    /\ replayAcceptedStale \in BOOLEAN
    /\ rebondReached \in BOOLEAN
    /\ HashRank \in [Hashes -> Nat]
    /\ \A first \in Hashes, second \in Hashes :
         HashRank[first] = HashRank[second] => first = second
    /\ ProposerUsesCanonicalPreState \in BOOLEAN
    /\ ReceiverUsesCanonicalPreState \in BOOLEAN
    /\ UseGenerationInAuthorization \in BOOLEAN
    /\ UseActivationWindow \in BOOLEAN
    /\ ReuseGenerationOnFreshBond \in BOOLEAN
    /\ ChangeGenerationOnEpochAdvance \in BOOLEAN
    /\ AllowGenerationChangeOutsideBond \in BOOLEAN
    /\ ReplayUsesCommittedGeneration \in BOOLEAN
    /\ CanonicalAuthorityAtomic \in BOOLEAN
    /\ AllowDetachedCanonicalPreState \in BOOLEAN

Init ==
    /\ bonds = InitialBonds
    /\ ambientBonds = InitialBonds
    /\ canonicalBonds = InitialBonds
    /\ bondGeneration = [v \in Validators |-> 0]
    /\ ambientGeneration = [v \in Validators |-> 0]
    /\ canonicalGeneration = [v \in Validators |-> 0]
    /\ phase = [v \in Validators |-> Bonded]
    /\ liveGenerations = [v \in Validators |-> {0}]
    /\ successfulBonds = [v \in Validators |-> 1]
    /\ evidence = {}
    /\ pendingSlashDeploys = {}
    /\ slashedLifetimes = {}
    /\ epoch \in Epochs
    /\ rejectedSlashDeploys = {}
    /\ mergeRejectedSlashDeploys = {}
    /\ badAuthObserved = FALSE
    /\ generationChangedOutsideBond = FALSE
    /\ replayAcceptedStale = FALSE
    /\ rebondReached = FALSE

\* @type: (Str, Int, Str, Str) => Bool;
RecordSlashableInvalid(v, g, e, h) ==
    /\ v \in Validators
    /\ g \in GenerationValues
    /\ e \in Epochs
    /\ h \in Hashes
    /\ HashUnused(h)
    /\ evidence' = evidence \cup {<<h, v, g, e>>}
    /\ pendingSlashDeploys' = AuthorizedDeploysForView(
        ProposerAuthorityBonds,
        ProposerAuthorityGenerations,
        evidence \cup {<<h, v, g, e>>},
        epoch)
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, bondGeneration,
                    ambientGeneration, canonicalGeneration, phase,
                    liveGenerations, successfulBonds, slashedLifetimes,
                    epoch, rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    badAuthObserved, generationChangedOutsideBond,
                    replayAcceptedStale, rebondReached>>

\* @type: Str => Bool;
AdvanceEpoch(nextEpoch) ==
    /\ nextEpoch \in Epochs
    /\ nextEpoch # epoch
    /\ epoch' = nextEpoch
    /\ IF ChangeGenerationOnEpochAdvance
       THEN
         /\ bondGeneration' =
              [v \in Validators |->
                IF bondGeneration[v] < MaxGeneration
                THEN bondGeneration[v] + 1
                ELSE bondGeneration[v]]
         /\ ambientGeneration' = bondGeneration'
         /\ canonicalGeneration' = bondGeneration'
         /\ generationChangedOutsideBond' = TRUE
       ELSE
         /\ UNCHANGED <<bondGeneration, ambientGeneration,
                         canonicalGeneration, generationChangedOutsideBond>>
    /\ pendingSlashDeploys' = AuthorizedDeploysForView(
        IF ProposerUsesCanonicalPreState
        THEN canonicalBonds ELSE ambientBonds,
        IF ProposerUsesCanonicalPreState
        THEN canonicalGeneration' ELSE ambientGeneration',
        evidence,
        nextEpoch)
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, phase,
                    liveGenerations, successfulBonds, evidence,
                    slashedLifetimes, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys, badAuthObserved,
                    replayAcceptedStale, rebondReached>>

\* @type: Str => Bool;
RequestWithdraw(v) ==
    /\ v \in Validators
    /\ phase[v] = Bonded
    /\ phase' = [phase EXCEPT ![v] = PendingWithdraw]
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, bondGeneration,
                    ambientGeneration, canonicalGeneration, liveGenerations,
                    successfulBonds, evidence, pendingSlashDeploys,
                    slashedLifetimes, epoch, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys, badAuthObserved,
                    generationChangedOutsideBond, replayAcceptedStale,
                    rebondReached>>

\* @type: Str => Bool;
BeginWithdraw(v) ==
    /\ v \in Validators
    /\ phase[v] = PendingWithdraw
    /\ phase' = [phase EXCEPT ![v] = Withdrawing]
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, bondGeneration,
                    ambientGeneration, canonicalGeneration, liveGenerations,
                    successfulBonds, evidence, pendingSlashDeploys,
                    slashedLifetimes, epoch, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys, badAuthObserved,
                    generationChangedOutsideBond, replayAcceptedStale,
                    rebondReached>>

\* @type: Str => Bool;
CompleteWithdrawal(v) ==
    /\ v \in Validators
    /\ phase[v] = Withdrawing
    /\ phase' = [phase EXCEPT ![v] = Withdrawn]
    /\ bonds' = [bonds EXCEPT ![v] = 0]
    /\ ambientBonds' = [ambientBonds EXCEPT ![v] = 0]
    /\ canonicalBonds' = [canonicalBonds EXCEPT ![v] = 0]
    /\ liveGenerations' = [liveGenerations EXCEPT ![v] = {}]
    /\ pendingSlashDeploys' = AuthorizedDeploysForView(
        canonicalBonds', canonicalGeneration, evidence, epoch)
    /\ UNCHANGED <<bondGeneration, ambientGeneration, canonicalGeneration,
                    successfulBonds, evidence, slashedLifetimes, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    badAuthObserved, generationChangedOutsideBond,
                    replayAcceptedStale, rebondReached>>

\* @type: Str => Bool;
FreshBond(v) ==
    /\ v \in Validators
    /\ phase[v] = Withdrawn
    /\ liveGenerations[v] = {}
    /\ ReuseGenerationOnFreshBond \/ bondGeneration[v] < MaxGeneration
    /\ LET nextGeneration ==
           IF ReuseGenerationOnFreshBond
           THEN bondGeneration[v]
           ELSE bondGeneration[v] + 1
       IN
         /\ bondGeneration' =
              [bondGeneration EXCEPT ![v] = nextGeneration]
         /\ ambientGeneration' =
              [ambientGeneration EXCEPT ![v] = nextGeneration]
         /\ canonicalGeneration' =
              [canonicalGeneration EXCEPT ![v] = nextGeneration]
         /\ liveGenerations' =
              [liveGenerations EXCEPT ![v] = {nextGeneration}]
         /\ rebondReached' =
              (rebondReached \/ nextGeneration = 1)
    /\ phase' = [phase EXCEPT ![v] = Bonded]
    /\ bonds' = [bonds EXCEPT ![v] = InitialBonds[v]]
    /\ ambientBonds' = [ambientBonds EXCEPT ![v] = InitialBonds[v]]
    /\ canonicalBonds' = [canonicalBonds EXCEPT ![v] = InitialBonds[v]]
    /\ successfulBonds' = [successfulBonds EXCEPT ![v] = @ + 1]
    /\ pendingSlashDeploys' = AuthorizedDeploysForView(
        canonicalBonds', canonicalGeneration', evidence, epoch)
    /\ UNCHANGED <<evidence, slashedLifetimes, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    badAuthObserved, generationChangedOutsideBond,
                    replayAcceptedStale>>

\* @type: Str => Bool;
MutateGenerationOutsideBond(v) ==
    /\ AllowGenerationChangeOutsideBond
    /\ v \in Validators
    /\ bondGeneration[v] < MaxGeneration
    /\ bondGeneration' = [bondGeneration EXCEPT ![v] = @ + 1]
    /\ ambientGeneration' = [ambientGeneration EXCEPT ![v] = @ + 1]
    /\ canonicalGeneration' = [canonicalGeneration EXCEPT ![v] = @ + 1]
    /\ liveGenerations' =
         [liveGenerations EXCEPT
           ![v] = IF @ = {} THEN {} ELSE {bondGeneration'[v]}]
    /\ generationChangedOutsideBond' = TRUE
    /\ pendingSlashDeploys' = AuthorizedDeploysForView(
         canonicalBonds, canonicalGeneration', evidence, epoch)
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, phase,
                    successfulBonds, evidence, slashedLifetimes, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    badAuthObserved, replayAcceptedStale, rebondReached>>

\* @type: (Str -> Int, Str -> Int) => Bool;
SelectAmbientSnapshot(bondView, generationView) ==
    /\ bondView \in [Validators -> BondValues]
    /\ generationView \in [Validators -> GenerationValues]
    /\ ambientBonds' = bondView
    /\ ambientGeneration' = generationView
    /\ UNCHANGED <<bonds, canonicalBonds, bondGeneration,
                    canonicalGeneration, phase, liveGenerations,
                    successfulBonds, evidence, pendingSlashDeploys,
                    slashedLifetimes, epoch, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys, badAuthObserved,
                    generationChangedOutsideBond, replayAcceptedStale,
                    rebondReached>>

\* @type: (Str -> Int, Str -> Int) => Bool;
SelectCanonicalPreState(bondView, generationView) ==
    /\ bondView \in [Validators -> BondValues]
    /\ generationView \in [Validators -> GenerationValues]
    /\ (AllowDetachedCanonicalPreState \/ ~CanonicalAuthorityAtomic)
    /\ canonicalBonds' = bondView
    /\ IF CanonicalAuthorityAtomic
       THEN
         /\ canonicalGeneration' = generationView
       ELSE
         /\ canonicalGeneration' = canonicalGeneration
    /\ pendingSlashDeploys' = AuthorizedDeploysForView(
        IF ProposerUsesCanonicalPreState
        THEN bondView ELSE ambientBonds,
        IF ProposerUsesCanonicalPreState
        THEN canonicalGeneration' ELSE ambientGeneration,
        evidence,
        epoch)
    /\ UNCHANGED <<bonds, ambientBonds, bondGeneration,
                    ambientGeneration, phase, liveGenerations,
                    successfulBonds, evidence, slashedLifetimes, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    badAuthObserved, generationChangedOutsideBond,
                    replayAcceptedStale, rebondReached>>

\* @type: (Str, Int, Str, Str) => Bool;
ReceiveUnauthorizedSlash(v, g, e, h) ==
    /\ v \in Validators
    /\ g \in GenerationValues
    /\ e \in Epochs
    /\ h \in Hashes
    /\ ~ReceiverAuthorized(v, g, e, h)
    /\ rejectedSlashDeploys' = {<<v, g, e, h>>}
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, bondGeneration,
                    ambientGeneration, canonicalGeneration, phase,
                    liveGenerations, successfulBonds, evidence,
                    pendingSlashDeploys, slashedLifetimes, epoch,
                    mergeRejectedSlashDeploys, badAuthObserved,
                    generationChangedOutsideBond, replayAcceptedStale,
                    rebondReached>>

\* @type: (Str, Int, Str, Str) => Bool;
ObserveMergeRejectedSlash(v, g, e, h) ==
    /\ <<h, v, g, e>> \in evidence
    /\ mergeRejectedSlashDeploys' = {<<v, g, e, h>>}
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, bondGeneration,
                    ambientGeneration, canonicalGeneration, phase,
                    liveGenerations, successfulBonds, evidence,
                    pendingSlashDeploys, slashedLifetimes, epoch,
                    rejectedSlashDeploys, badAuthObserved,
                    generationChangedOutsideBond, replayAcceptedStale,
                    rebondReached>>

ReceiveBadAuthSlash ==
    /\ badAuthObserved' = TRUE
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, bondGeneration,
                    ambientGeneration, canonicalGeneration, phase,
                    liveGenerations, successfulBonds, evidence,
                    pendingSlashDeploys, slashedLifetimes, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    generationChangedOutsideBond, replayAcceptedStale,
                    rebondReached>>

\* @type: (Str, Int, Str, Str) => Bool;
ExecuteSlash(v, g, e, h) ==
    /\ <<v, g, e, h>> \in pendingSlashDeploys
    /\ Authorized(v, g, e, h)
    /\ canonicalBonds = bonds
    /\ canonicalGeneration = bondGeneration
    /\ bonds[v] > 0
    /\ bondGeneration[v] = g
    /\ liveGenerations[v] = {g}
    /\ phase[v] \in {Bonded, PendingWithdraw, Withdrawing}
    /\ bonds' = [bonds EXCEPT ![v] = 0]
    /\ ambientBonds' = [ambientBonds EXCEPT ![v] = 0]
    /\ canonicalBonds' = [canonicalBonds EXCEPT ![v] = 0]
    /\ phase' = [phase EXCEPT ![v] = Quarantined]
    /\ pendingSlashDeploys' =
         {deploy \in pendingSlashDeploys :
            deploy[1] # v \/ deploy[2] # g}
    /\ slashedLifetimes' = slashedLifetimes \cup {<<v, g>>}
    /\ UNCHANGED <<bondGeneration, ambientGeneration,
                    canonicalGeneration, liveGenerations,
                    successfulBonds, evidence, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    badAuthObserved, generationChangedOutsideBond,
                    replayAcceptedStale, rebondReached>>

\* @type: (Str, Int, Str, Str) => Bool;
ReplaySlash(v, committedGeneration, e, h) ==
    /\ <<h, v, committedGeneration, e>> \in evidence
    /\ LET replayGeneration ==
           IF ReplayUsesCommittedGeneration
           THEN committedGeneration
           ELSE canonicalGeneration[v]
       IN replayAcceptedStale' =
            (replayAcceptedStale
             \/ (committedGeneration # canonicalGeneration[v]
                 /\ IF ReplayUsesCommittedGeneration
                    THEN AuthorizedForView(
                           canonicalBonds, canonicalGeneration,
                           v, replayGeneration, e, h)
                    ELSE /\ canonicalBonds[v] > 0
                         /\ ActivationMatches(e, epoch)))
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, bondGeneration,
                    ambientGeneration, canonicalGeneration, phase,
                    liveGenerations, successfulBonds, evidence,
                    pendingSlashDeploys, slashedLifetimes, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    badAuthObserved, generationChangedOutsideBond,
                    rebondReached>>

Next ==
    \/ \E v \in Validators, g \in GenerationValues,
          e \in Epochs, h \in Hashes : RecordSlashableInvalid(v, g, e, h)
    \/ \E nextEpoch \in Epochs : AdvanceEpoch(nextEpoch)
    \/ \E v \in Validators : RequestWithdraw(v)
    \/ \E v \in Validators : BeginWithdraw(v)
    \/ \E v \in Validators : CompleteWithdrawal(v)
    \/ \E v \in Validators : FreshBond(v)
    \/ \E v \in Validators : MutateGenerationOutsideBond(v)
    \/ \E bondView \in [Validators -> BondValues],
          generationView \in [Validators -> GenerationValues] :
         SelectAmbientSnapshot(bondView, generationView)
    \/ \E bondView \in [Validators -> BondValues],
          generationView \in [Validators -> GenerationValues] :
         SelectCanonicalPreState(bondView, generationView)
    \/ \E v \in Validators, g \in GenerationValues,
          e \in Epochs, h \in Hashes : ReceiveUnauthorizedSlash(v, g, e, h)
    \/ \E v \in Validators, g \in GenerationValues,
          e \in Epochs, h \in Hashes : ObserveMergeRejectedSlash(v, g, e, h)
    \/ ReceiveBadAuthSlash
    \/ \E v \in Validators, g \in GenerationValues,
          e \in Epochs, h \in Hashes : ExecuteSlash(v, g, e, h)
    \/ \E v \in Validators, g \in GenerationValues,
          e \in Epochs, h \in Hashes : ReplaySlash(v, g, e, h)

\* Each behavior selects one immutable block pre-state. The bonds and bond
\* generation maps are that state. The canonical maps are its atomic authority
\* projection. Ambient views can differ because validators observe concurrently.
\* A different block root is checked by a different behavior.
\* Rejected observations use a one-slot monitor. Their invariants are local to
\* each observation and do not depend on rejection history. Replacing the prior
\* slot preserves every observation transition and removes irrelevant powerset
\* history from explicit-state exploration.
Spec == Init /\ [][Next]_vars

Inv_GenerationEqualsSuccessfulBondCount ==
    \A v \in Validators : successfulBonds[v] = bondGeneration[v] + 1

Inv_AtMostOneLiveGenerationPerKey ==
    \A v \in Validators : Cardinality(liveGenerations[v]) <= 1

Inv_LivePhaseHasCurrentGeneration ==
    \A v \in Validators :
      phase[v] \in {Bonded, PendingWithdraw, Withdrawing, Quarantined}
      => liveGenerations[v] = {bondGeneration[v]}

Inv_EpochAdvancePreservesBondGeneration ==
    ~generationChangedOutsideBond

Inv_GenerationChangesOnlyOnSuccessfulFreshBond ==
    ~generationChangedOutsideBond

Inv_OnlyCurrentEpochCurrentGenerationSlashCanBePending ==
    \A deploy \in pendingSlashDeploys :
      /\ Authorized(deploy[1], deploy[2], deploy[3], deploy[4])
      /\ ActivationMatches(deploy[3], epoch)
      /\ GenerationMatches(canonicalGeneration, deploy[1], deploy[2])

Inv_StaleGenerationCannotSlashRebondedKey ==
    \A item \in evidence :
      item[3] # canonicalGeneration[item[2]]
      => ~PendingCoversTarget(item[2], item[3])

Inv_OldEpochEvidenceCannotAuthorizeCurrentWindow ==
    \A item \in evidence :
      item[4] # epoch
      => \A deploy \in pendingSlashDeploys : deploy[4] # item[1]

Inv_SlashedIdentityIsGenerationScoped ==
    \A lifetime \in slashedLifetimes :
      lifetime[2] = bondGeneration[lifetime[1]]
      => /\ phase[lifetime[1]] = Quarantined
         /\ liveGenerations[lifetime[1]] = {lifetime[2]}
         /\ bonds[lifetime[1]] = 0

Inv_SlashedGenerationNeverExceedsCurrent ==
    \A lifetime \in slashedLifetimes :
      lifetime[2] <= bondGeneration[lifetime[1]]

Inv_PendingSlashTargetUnique ==
    \A first \in pendingSlashDeploys :
      \A second \in pendingSlashDeploys :
        first[1] = second[1] /\ first[2] = second[2]
        => first = second

Inv_PendingSlashHashUnique ==
    \A first \in pendingSlashDeploys :
      \A second \in pendingSlashDeploys :
        first[4] = second[4] => first = second

Inv_CanonicalAuthorityMatchesLifecycleState ==
    /\ canonicalBonds = bonds
    /\ canonicalGeneration = bondGeneration

Inv_PendingSlashBoundToCurrentPreState ==
    \A deploy \in pendingSlashDeploys :
      AuthorizedForView(
        bonds,
        bondGeneration,
        deploy[1],
        deploy[2],
        deploy[3],
        deploy[4])

Inv_PendingSlashCompleteForCurrentPreState ==
    pendingSlashDeploys = AuthorizedDeploysForView(
      bonds, bondGeneration, evidence, epoch)

Inv_ProposerAuthorizationMatchesCanonical ==
    \A v \in Validators, g \in GenerationValues,
       e \in Epochs, h \in Hashes :
      ProposerAuthorized(v, g, e, h) <=> Authorized(v, g, e, h)

Inv_ReceiverAuthorizationMatchesCanonical ==
    \A v \in Validators, g \in GenerationValues,
       e \in Epochs, h \in Hashes :
      ReceiverAuthorized(v, g, e, h) <=> Authorized(v, g, e, h)

Inv_ProposerReceiverAuthorizationParity ==
    \A v \in Validators, g \in GenerationValues,
       e \in Epochs, h \in Hashes :
      ProposerAuthorized(v, g, e, h)
      <=> ReceiverAuthorized(v, g, e, h)

Inv_ReplayUsesCommittedSlashIdentity == ~replayAcceptedStale

Inv_RebondReachabilityWitnessSound ==
    rebondReached =>
      \E v \in Validators :
        bondGeneration[v] = 1 /\ successfulBonds[v] = 2

Inv_EvidenceHashUnique ==
    \A first \in evidence :
      \A second \in evidence :
        first[1] = second[1] => first = second

Inv_RejectedSlashWithoutEvidenceNoPending ==
    \A deploy \in rejectedSlashDeploys :
      <<deploy[4], deploy[1], deploy[2], deploy[3]>> \notin evidence
      => deploy \notin pendingSlashDeploys

Inv_RejectedObservationMonitorBounded ==
    Cardinality(rejectedSlashDeploys) <= 1

Inv_InvalidAuthSlashNoPending ==
    badAuthObserved =>
      \A deploy \in pendingSlashDeploys :
        Authorized(deploy[1], deploy[2], deploy[3], deploy[4])

Inv_MergeRejectedSlashCoveredByCanonicalScan ==
    \A deploy \in mergeRejectedSlashDeploys :
      Authorized(deploy[1], deploy[2], deploy[3], deploy[4])
      => PendingCoversTarget(deploy[1], deploy[2])
         \/ <<deploy[1], deploy[2]>> \in slashedLifetimes

Inv_MergeRejectedSlashCannotAuthorizeZeroBond ==
    \A deploy \in mergeRejectedSlashDeploys :
      canonicalBonds[deploy[1]] = 0
      => ~PendingCoversTarget(deploy[1], deploy[2])

Inv_MergeRejectedObservationMonitorBounded ==
    Cardinality(mergeRejectedSlashDeploys) <= 1

Safety ==
    /\ TypeOK
    /\ Inv_GenerationEqualsSuccessfulBondCount
    /\ Inv_AtMostOneLiveGenerationPerKey
    /\ Inv_LivePhaseHasCurrentGeneration
    /\ Inv_EpochAdvancePreservesBondGeneration
    /\ Inv_GenerationChangesOnlyOnSuccessfulFreshBond
    /\ Inv_OnlyCurrentEpochCurrentGenerationSlashCanBePending
    /\ Inv_StaleGenerationCannotSlashRebondedKey
    /\ Inv_OldEpochEvidenceCannotAuthorizeCurrentWindow
    /\ Inv_SlashedIdentityIsGenerationScoped
    /\ Inv_SlashedGenerationNeverExceedsCurrent
    /\ Inv_RejectedSlashWithoutEvidenceNoPending
    /\ Inv_RejectedObservationMonitorBounded
    /\ Inv_InvalidAuthSlashNoPending
    /\ Inv_EvidenceHashUnique
    /\ Inv_MergeRejectedSlashCoveredByCanonicalScan
    /\ Inv_MergeRejectedSlashCannotAuthorizeZeroBond
    /\ Inv_MergeRejectedObservationMonitorBounded
    /\ Inv_CanonicalAuthorityMatchesLifecycleState
    /\ Inv_PendingSlashBoundToCurrentPreState
    /\ Inv_PendingSlashCompleteForCurrentPreState
    /\ Inv_ProposerAuthorizationMatchesCanonical
    /\ Inv_ReceiverAuthorizationMatchesCanonical
    /\ Inv_ProposerReceiverAuthorizationParity
    /\ Inv_PendingSlashHashUnique
    /\ Inv_PendingSlashTargetUnique
    /\ Inv_ReplayUsesCommittedSlashIdentity
    /\ Inv_RebondReachabilityWitnessSound

============================================================================
