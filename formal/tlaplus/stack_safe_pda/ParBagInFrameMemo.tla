------------------------ MODULE ParBagInFrameMemo ------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

(***************************************************************************
Bounded refinement model for descriptor-certified response egress.

This model is the Par-owner projection of the sealed typed structural
transcript in `CertifiedEgressTranscript.tla`: the mandatory preflight records
every protobuf-visible Par occurrence in generated schema order and computes
an exact owner census. Only after that walk is complete may emission replay
those Par slots. Component-empty Pars and
singleton owners take the direct lane; only owners proved repeated may record
or copy response-local in-frame component ranges. The private frame is an
append-only byte transcript equal to direct encoding after every replay step.
***************************************************************************)

CONSTANT MaxOwners, MaxRoots, MaxOccurrences

ASSUME /\ MaxOwners \in Nat \ {0}
       /\ MaxRoots \in Nat \ {0}
       /\ MaxOccurrences \in Nat \ {0}

Owners == 0..(MaxOwners - 1)
Roots == 0..(MaxRoots - 1)
EmptyOwner == MaxOwners
Slots == Owners \cup {EmptyOwner}
NoRoot == MaxRoots
Phases == {"census", "emitting", "ready", "committed"}
Lanes == {"direct", "record", "reuse"}

Before(owner) == IF owner = EmptyOwner THEN <<>> ELSE <<4 * owner + 1>>
After(owner) == IF owner = EmptyOwner THEN <<>> ELSE <<4 * owner + 2>>
Root(root) == <<4 * MaxOwners + root + 1>>
DirectImage(owner, root) == Before(owner) \o Root(root) \o After(owner)

OccurrenceCount(items, owner) ==
    Cardinality({index \in 1..Len(items) : items[index] = owner})

VARIABLES phase,
          censusOwners, censusRoots, censusCounts, censusEmpty,
          replayCount, replayOwners, replayRoots, replayLanes,
          output, oracle, memoOwners, beforeMemo, afterMemo, firstRoot

vars == <<phase,
          censusOwners, censusRoots, censusCounts, censusEmpty,
          replayCount, replayOwners, replayRoots, replayLanes,
          output, oracle, memoOwners, beforeMemo, afterMemo, firstRoot>>

Init ==
    /\ phase = "census"
    /\ censusOwners = <<>>
    /\ censusRoots = <<>>
    /\ censusCounts = [owner \in Owners |-> 0]
    /\ censusEmpty = 0
    /\ replayCount = 0
    /\ replayOwners = <<>>
    /\ replayRoots = <<>>
    /\ replayLanes = <<>>
    /\ output = <<>>
    /\ oracle = <<>>
    /\ memoOwners = {}
    /\ beforeMemo = [owner \in Owners |-> <<>>]
    /\ afterMemo = [owner \in Owners |-> <<>>]
    /\ firstRoot = [owner \in Owners |-> NoRoot]

CensusAppend ==
    /\ phase = "census"
    /\ Len(censusOwners) < MaxOccurrences
    /\ \E owner \in Slots, root \in Roots:
        /\ censusOwners' = Append(censusOwners, owner)
        /\ censusRoots' = Append(censusRoots, root)
        /\ censusCounts' = IF owner = EmptyOwner THEN censusCounts
                              ELSE [censusCounts EXCEPT ![owner] = @ + 1]
        /\ censusEmpty' = IF owner = EmptyOwner THEN censusEmpty + 1
                            ELSE censusEmpty
        /\ UNCHANGED <<phase, replayCount, replayOwners, replayRoots,
                        replayLanes, output, oracle, memoOwners, beforeMemo,
                        afterMemo, firstRoot>>

FinishCensus ==
    /\ phase = "census"
    /\ Len(censusOwners) = MaxOccurrences
    /\ phase' = "emitting"
    /\ UNCHANGED <<censusOwners, censusRoots, censusCounts, censusEmpty,
                    replayCount, replayOwners, replayRoots, replayLanes,
                    output, oracle, memoOwners, beforeMemo, afterMemo,
                    firstRoot>>

LaneFor(owner, recorded) ==
    IF owner = EmptyOwner
      THEN "direct"
      ELSE IF censusCounts[owner] = 1
        THEN "direct"
        ELSE IF owner \in recorded THEN "reuse" ELSE "record"

RequiresDirect(owner) ==
    IF owner = EmptyOwner THEN TRUE ELSE censusCounts[owner] = 1

CorrectEmit ==
    /\ phase = "emitting"
    /\ replayCount < MaxOccurrences
    /\ LET next == replayCount + 1
           owner == censusOwners[next]
           root == censusRoots[next]
           lane == LaneFor(owner, memoOwners)
           hit == owner \in memoOwners
           image == IF lane = "reuse"
                      THEN beforeMemo[owner] \o Root(root) \o afterMemo[owner]
                      ELSE DirectImage(owner, root)
       IN /\ replayCount' = next
          /\ replayOwners' = Append(replayOwners, owner)
          /\ replayRoots' = Append(replayRoots, root)
          /\ replayLanes' = Append(replayLanes, lane)
          /\ output' = output \o image
          /\ oracle' = oracle \o DirectImage(owner, root)
          /\ memoOwners' = IF lane = "record" THEN memoOwners \cup {owner}
                             ELSE memoOwners
          /\ beforeMemo' = IF lane = "record" /\ ~hit
                              THEN [beforeMemo EXCEPT ![owner] = Before(owner)]
                              ELSE beforeMemo
          /\ afterMemo' = IF lane = "record" /\ ~hit
                             THEN [afterMemo EXCEPT ![owner] = After(owner)]
                             ELSE afterMemo
          /\ firstRoot' = IF lane = "record" /\ ~hit
                             THEN [firstRoot EXCEPT ![owner] = root]
                             ELSE firstRoot
          /\ UNCHANGED <<phase, censusOwners, censusRoots, censusCounts,
                          censusEmpty>>

FinishEmission ==
    /\ phase = "emitting"
    /\ replayCount = MaxOccurrences
    /\ phase' = "ready"
    /\ UNCHANGED <<censusOwners, censusRoots, censusCounts, censusEmpty,
                    replayCount, replayOwners, replayRoots, replayLanes,
                    output, oracle, memoOwners, beforeMemo, afterMemo,
                    firstRoot>>

Commit ==
    /\ phase = "ready"
    /\ phase' = "committed"
    /\ UNCHANGED <<censusOwners, censusRoots, censusCounts, censusEmpty,
                    replayCount, replayOwners, replayRoots, replayLanes,
                    output, oracle, memoOwners, beforeMemo, afterMemo,
                    firstRoot>>

Next == CensusAppend \/ FinishCensus \/ CorrectEmit \/ FinishEmission \/ Commit
Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ phase \in Phases
    /\ censusOwners \in Seq(Slots)
    /\ censusRoots \in Seq(Roots)
    /\ Len(censusOwners) = Len(censusRoots)
    /\ Len(censusOwners) \in 0..MaxOccurrences
    /\ censusCounts \in [Owners -> 0..MaxOccurrences]
    /\ censusEmpty \in 0..MaxOccurrences
    /\ replayCount \in 0..MaxOccurrences
    /\ replayOwners \in Seq(Slots)
    /\ replayRoots \in Seq(Roots)
    /\ replayLanes \in Seq(Lanes)
    /\ Len(replayOwners) = replayCount
    /\ Len(replayRoots) = replayCount
    /\ Len(replayLanes) = replayCount
    /\ output \in Seq(Nat)
    /\ oracle \in Seq(Nat)
    /\ memoOwners \subseteq Owners
    /\ beforeMemo \in [Owners -> Seq(Nat)]
    /\ afterMemo \in [Owners -> Seq(Nat)]
    /\ firstRoot \in [Owners -> (Roots \cup {NoRoot})]

CensusPrefixIsExact ==
    /\ \A owner \in Owners:
          censusCounts[owner] = OccurrenceCount(censusOwners, owner)
    /\ censusEmpty = OccurrenceCount(censusOwners, EmptyOwner)

CensusCompletesBeforeEmission ==
    phase # "census" => Len(censusOwners) = MaxOccurrences

ReplayConsumesExactOccurrencePrefix ==
    /\ replayCount <= Len(censusOwners)
    /\ replayOwners = SubSeq(censusOwners, 1, replayCount)
    /\ replayRoots = SubSeq(censusRoots, 1, replayCount)

ReplayExhaustionIffEmissionComplete ==
    (replayCount = MaxOccurrences)
    <=> (phase \in {"ready", "committed"}
         \/ (phase = "emitting" /\ replayCount = MaxOccurrences))

DirectLaneForEmptyAndSingleton ==
    \A index \in 1..replayCount:
      RequiresDirect(replayOwners[index])
      => replayLanes[index] = "direct"

MemoOnlyForCensusProvenRepeatedOwners ==
    /\ \A index \in 1..replayCount:
          replayLanes[index] \in {"record", "reuse"}
          => /\ replayOwners[index] \in Owners
             /\ censusCounts[replayOwners[index]] > 1
    /\ \A owner \in memoOwners: censusCounts[owner] > 1

OutputMatchesDirect == output = oracle

CommitRequiresExactReplay ==
    phase = "committed"
    => /\ replayCount = MaxOccurrences
       /\ replayOwners = censusOwners
       /\ replayRoots = censusRoots
       /\ output = oracle

MemoIsOwnerExact ==
    \A owner \in Owners:
      IF owner \in memoOwners
        THEN /\ beforeMemo[owner] = Before(owner)
             /\ afterMemo[owner] = After(owner)
             /\ firstRoot[owner] \in Roots
        ELSE /\ beforeMemo[owner] = <<>>
             /\ afterMemo[owner] = <<>>
             /\ firstRoot[owner] = NoRoot

(***************************************************************************
Calibrated negative controls. Each bad action has its own configuration and
must violate the named production invariant. The four original range mutants
remain and are enabled only after an owner-exact memo range exists.
***************************************************************************)

CensusSkipMutant ==
    /\ phase = "census"
    /\ Len(censusOwners) < MaxOccurrences
    /\ \E owner \in Owners, root \in Roots:
        /\ censusOwners' = Append(censusOwners, owner)
        /\ censusRoots' = Append(censusRoots, root)
        /\ UNCHANGED <<phase, censusCounts, censusEmpty, replayCount,
                        replayOwners, replayRoots, replayLanes, output, oracle,
                        memoOwners, beforeMemo, afterMemo, firstRoot>>

ReplaySwapMutant ==
    /\ phase = "emitting"
    /\ replayCount + 1 < MaxOccurrences
    /\ censusOwners[replayCount + 1] # censusOwners[replayCount + 2]
       \/ censusRoots[replayCount + 1] # censusRoots[replayCount + 2]
    /\ LET wrong == replayCount + 2
           owner == censusOwners[wrong]
           root == censusRoots[wrong]
       IN /\ replayCount' = replayCount + 1
          /\ replayOwners' = Append(replayOwners, owner)
          /\ replayRoots' = Append(replayRoots, root)
          /\ replayLanes' = Append(replayLanes, LaneFor(owner, memoOwners))
          /\ output' = output \o DirectImage(owner, root)
          /\ oracle' = oracle \o DirectImage(owner, root)
          /\ UNCHANGED <<phase, censusOwners, censusRoots, censusCounts,
                          censusEmpty, memoOwners, beforeMemo, afterMemo,
                          firstRoot>>

ReplayOverrunMutant ==
    /\ phase = "ready"
    /\ \E owner \in Slots, root \in Roots:
        /\ phase' = "emitting"
        /\ replayCount' = replayCount + 1
        /\ replayOwners' = Append(replayOwners, owner)
        /\ replayRoots' = Append(replayRoots, root)
        /\ replayLanes' = Append(replayLanes, "direct")
        /\ output' = output \o DirectImage(owner, root)
        /\ oracle' = oracle \o DirectImage(owner, root)
        /\ UNCHANGED <<censusOwners, censusRoots, censusCounts, censusEmpty,
                        memoOwners, beforeMemo, afterMemo, firstRoot>>

ReplayRemainingCommitMutant ==
    /\ phase = "emitting"
    /\ replayCount < MaxOccurrences
    /\ phase' = "committed"
    /\ UNCHANGED <<censusOwners, censusRoots, censusCounts, censusEmpty,
                    replayCount, replayOwners, replayRoots, replayLanes,
                    output, oracle, memoOwners, beforeMemo, afterMemo,
                    firstRoot>>

EmptyMemoMutant ==
    /\ phase = "emitting"
    /\ replayCount < MaxOccurrences
    /\ censusOwners[replayCount + 1] = EmptyOwner
    /\ LET next == replayCount + 1
           root == censusRoots[next]
       IN /\ replayCount' = next
          /\ replayOwners' = Append(replayOwners, EmptyOwner)
          /\ replayRoots' = Append(replayRoots, root)
          /\ replayLanes' = Append(replayLanes, "record")
          /\ output' = output \o DirectImage(EmptyOwner, root)
          /\ oracle' = oracle \o DirectImage(EmptyOwner, root)
          /\ UNCHANGED <<phase, censusOwners, censusRoots, censusCounts,
                          censusEmpty, memoOwners, beforeMemo, afterMemo,
                          firstRoot>>

SingletonMemoMutant ==
    /\ phase = "emitting"
    /\ replayCount < MaxOccurrences
    /\ LET next == replayCount + 1
           owner == censusOwners[next]
           root == censusRoots[next]
       IN /\ owner \in Owners
          /\ censusCounts[owner] = 1
          /\ replayCount' = next
          /\ replayOwners' = Append(replayOwners, owner)
          /\ replayRoots' = Append(replayRoots, root)
          /\ replayLanes' = Append(replayLanes, "record")
          /\ output' = output \o DirectImage(owner, root)
          /\ oracle' = oracle \o DirectImage(owner, root)
          /\ UNCHANGED <<phase, censusOwners, censusRoots, censusCounts,
                          censusEmpty, memoOwners, beforeMemo, afterMemo,
                          firstRoot>>

EarlyEmissionMutant ==
    /\ phase = "census"
    /\ Len(censusOwners) < MaxOccurrences
    /\ phase' = "emitting"
    /\ UNCHANGED <<censusOwners, censusRoots, censusCounts, censusEmpty,
                    replayCount, replayOwners, replayRoots, replayLanes,
                    output, oracle, memoOwners, beforeMemo, afterMemo,
                    firstRoot>>

SwappedRangesMutant ==
    /\ phase = "emitting"
    /\ replayCount < MaxOccurrences
    /\ LET next == replayCount + 1
           owner == censusOwners[next]
           root == censusRoots[next]
       IN /\ owner \in memoOwners
          /\ replayCount' = next
          /\ replayOwners' = Append(replayOwners, owner)
          /\ replayRoots' = Append(replayRoots, root)
          /\ replayLanes' = Append(replayLanes, "reuse")
          /\ output' = output \o afterMemo[owner] \o Root(root) \o beforeMemo[owner]
          /\ oracle' = oracle \o DirectImage(owner, root)
          /\ UNCHANGED <<phase, censusOwners, censusRoots, censusCounts,
                          censusEmpty, memoOwners, beforeMemo, afterMemo,
                          firstRoot>>

WholePriorParMutant ==
    /\ phase = "emitting"
    /\ replayCount < MaxOccurrences
    /\ LET next == replayCount + 1
           owner == censusOwners[next]
           root == censusRoots[next]
       IN /\ owner \in memoOwners
          /\ root # firstRoot[owner]
          /\ replayCount' = next
          /\ replayOwners' = Append(replayOwners, owner)
          /\ replayRoots' = Append(replayRoots, root)
          /\ replayLanes' = Append(replayLanes, "reuse")
          /\ output' = output \o beforeMemo[owner] \o Root(firstRoot[owner]) \o afterMemo[owner]
          /\ oracle' = oracle \o DirectImage(owner, root)
          /\ UNCHANGED <<phase, censusOwners, censusRoots, censusCounts,
                          censusEmpty, memoOwners, beforeMemo, afterMemo,
                          firstRoot>>

WrongOwnerMutant ==
    /\ phase = "emitting"
    /\ replayCount < MaxOccurrences
    /\ LET next == replayCount + 1
           owner == censusOwners[next]
           root == censusRoots[next]
       IN /\ owner \in Owners
          /\ \E wrong \in memoOwners:
              /\ owner # wrong
              /\ replayCount' = next
              /\ replayOwners' = Append(replayOwners, owner)
              /\ replayRoots' = Append(replayRoots, root)
              /\ replayLanes' = Append(replayLanes, LaneFor(owner, memoOwners))
              /\ output' = output \o beforeMemo[wrong] \o Root(root) \o afterMemo[wrong]
              /\ oracle' = oracle \o DirectImage(owner, root)
              /\ UNCHANGED <<phase, censusOwners, censusRoots, censusCounts,
                              censusEmpty, memoOwners, beforeMemo, afterMemo,
                              firstRoot>>

OffByOneRangeMutant ==
    /\ phase = "emitting"
    /\ replayCount < MaxOccurrences
    /\ LET next == replayCount + 1
           owner == censusOwners[next]
           root == censusRoots[next]
       IN /\ owner \in memoOwners
          /\ replayCount' = next
          /\ replayOwners' = Append(replayOwners, owner)
          /\ replayRoots' = Append(replayRoots, root)
          /\ replayLanes' = Append(replayLanes, "reuse")
          /\ output' = output \o beforeMemo[owner] \o Root(root)
          /\ oracle' = oracle \o DirectImage(owner, root)
          /\ UNCHANGED <<phase, censusOwners, censusRoots, censusCounts,
                          censusEmpty, memoOwners, beforeMemo, afterMemo,
                          firstRoot>>

CensusSkipMutantNext == Next \/ CensusSkipMutant
CensusSkipMutantSpec == Init /\ [][CensusSkipMutantNext]_vars
ReplaySwapMutantNext == Next \/ ReplaySwapMutant
ReplaySwapMutantSpec == Init /\ [][ReplaySwapMutantNext]_vars
ReplayOverrunMutantNext == Next \/ ReplayOverrunMutant
ReplayOverrunMutantSpec == Init /\ [][ReplayOverrunMutantNext]_vars
ReplayRemainingCommitMutantNext == Next \/ ReplayRemainingCommitMutant
ReplayRemainingCommitMutantSpec == Init /\ [][ReplayRemainingCommitMutantNext]_vars
EmptyMemoMutantNext == Next \/ EmptyMemoMutant
EmptyMemoMutantSpec == Init /\ [][EmptyMemoMutantNext]_vars
SingletonMemoMutantNext == Next \/ SingletonMemoMutant
SingletonMemoMutantSpec == Init /\ [][SingletonMemoMutantNext]_vars
EarlyEmissionMutantNext == Next \/ EarlyEmissionMutant
EarlyEmissionMutantSpec == Init /\ [][EarlyEmissionMutantNext]_vars
SwapMutantNext == Next \/ SwappedRangesMutant
SwapMutantSpec == Init /\ [][SwapMutantNext]_vars
WholeMutantNext == Next \/ WholePriorParMutant
WholeMutantSpec == Init /\ [][WholeMutantNext]_vars
WrongOwnerMutantNext == Next \/ WrongOwnerMutant
WrongOwnerMutantSpec == Init /\ [][WrongOwnerMutantNext]_vars
OffByOneMutantNext == Next \/ OffByOneRangeMutant
OffByOneMutantSpec == Init /\ [][OffByOneMutantNext]_vars

=============================================================================
