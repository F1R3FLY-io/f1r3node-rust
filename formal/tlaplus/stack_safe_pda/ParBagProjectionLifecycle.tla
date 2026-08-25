-------------------- MODULE ParBagProjectionLifecycle --------------------
EXTENDS Naturals, TLC

(***************************************************************************
Bounded lifecycle model for the explicitly prepared ParBag protobuf
projection. The authoritative RPB3 identity remains readable while a private
projection is being built. The `OnceLock`-refining public state changes only
from absent to complete. Copy-on-write mutation detaches the writer, clears
its versioned projection, and preserves the untouched clone.
***************************************************************************)

CONSTANT MaxBytes

ASSUME MaxBytes \in Nat

NoCache == 2
Phases == {"cold", "building", "ready", "refused"}
VisibleImages == {"none", "complete", "partial"}
ReadImages == {"none", "complete", "partial"}

VARIABLES target, limit, built, phase, visible,
          writerVersion, writerCache,
          hasClone, cloneVersion, cloneCache, sharedAux,
          mutated, savedCloneVersion, savedCloneCache,
          lastRead

vars == <<target, limit, built, phase, visible,
          writerVersion, writerCache,
          hasClone, cloneVersion, cloneCache, sharedAux,
          mutated, savedCloneVersion, savedCloneCache,
          lastRead>>

Init ==
    /\ target \in 0..MaxBytes
    /\ limit \in 0..MaxBytes
    /\ built = 0
    /\ phase = "cold"
    /\ visible = "none"
    /\ writerVersion = 0
    /\ writerCache = NoCache
    /\ hasClone = FALSE
    /\ cloneVersion = 0
    /\ cloneCache = NoCache
    /\ sharedAux = TRUE
    /\ mutated = FALSE
    /\ savedCloneVersion = 0
    /\ savedCloneCache = NoCache
    /\ lastRead = "none"

StartBuild ==
    /\ phase = "cold"
    /\ target <= limit
    /\ phase' = "building"
    /\ UNCHANGED <<target, limit, built, visible,
                    writerVersion, writerCache,
                    hasClone, cloneVersion, cloneCache, sharedAux,
                    mutated, savedCloneVersion, savedCloneCache, lastRead>>

RefuseTooLarge ==
    /\ phase = "cold"
    /\ target > limit
    /\ phase' = "refused"
    /\ UNCHANGED <<target, limit, built, visible,
                    writerVersion, writerCache,
                    hasClone, cloneVersion, cloneCache, sharedAux,
                    mutated, savedCloneVersion, savedCloneCache, lastRead>>

BuildPrivateByte ==
    /\ phase = "building"
    /\ built < target
    /\ built' = built + 1
    /\ UNCHANGED <<target, limit, phase, visible,
                    writerVersion, writerCache,
                    hasClone, cloneVersion, cloneCache, sharedAux,
                    mutated, savedCloneVersion, savedCloneCache, lastRead>>

PublishComplete ==
    /\ phase = "building"
    /\ built = target
    /\ phase' = "ready"
    /\ visible' = "complete"
    /\ writerCache' = writerVersion
    /\ cloneCache' = IF hasClone /\ sharedAux THEN cloneVersion ELSE cloneCache
    /\ UNCHANGED <<target, limit, built, writerVersion,
                    hasClone, cloneVersion, sharedAux,
                    mutated, savedCloneVersion, savedCloneCache, lastRead>>

CloneWriter ==
    /\ ~hasClone
    /\ hasClone' = TRUE
    /\ cloneVersion' = writerVersion
    /\ cloneCache' = writerCache
    /\ UNCHANGED <<target, limit, built, phase, visible,
                    writerVersion, writerCache, sharedAux,
                    mutated, savedCloneVersion, savedCloneCache, lastRead>>

ReadDirect ==
    /\ phase \in {"cold", "building", "refused"}
    /\ lastRead' = "complete"
    /\ UNCHANGED <<target, limit, built, phase, visible,
                    writerVersion, writerCache,
                    hasClone, cloneVersion, cloneCache, sharedAux,
                    mutated, savedCloneVersion, savedCloneCache>>

ReadPrepared ==
    /\ phase = "ready"
    /\ lastRead' = "complete"
    /\ UNCHANGED <<target, limit, built, phase, visible,
                    writerVersion, writerCache,
                    hasClone, cloneVersion, cloneCache, sharedAux,
                    mutated, savedCloneVersion, savedCloneCache>>

SuccessfulWriterMutation ==
    /\ hasClone
    /\ sharedAux
    /\ writerVersion = 0
    /\ writerVersion' = 1
    /\ writerCache' = NoCache
    /\ phase' = "cold"
    /\ visible' = "none"
    /\ built' = 0
    /\ sharedAux' = FALSE
    /\ mutated' = TRUE
    /\ savedCloneVersion' = cloneVersion
    /\ savedCloneCache' = cloneCache
    /\ UNCHANGED <<target, limit, hasClone, cloneVersion, cloneCache, lastRead>>

Next == StartBuild \/ RefuseTooLarge \/ BuildPrivateByte \/ PublishComplete
        \/ CloneWriter \/ ReadDirect \/ ReadPrepared \/ SuccessfulWriterMutation

Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ target \in 0..MaxBytes
    /\ limit \in 0..MaxBytes
    /\ built \in 0..target
    /\ phase \in Phases
    /\ visible \in VisibleImages
    /\ writerVersion \in 0..1
    /\ writerCache \in {0, 1, NoCache}
    /\ hasClone \in BOOLEAN
    /\ cloneVersion \in 0..1
    /\ cloneCache \in {0, 1, NoCache}
    /\ sharedAux \in BOOLEAN
    /\ mutated \in BOOLEAN
    /\ savedCloneVersion \in 0..1
    /\ savedCloneCache \in {0, 1, NoCache}
    /\ lastRead \in ReadImages

BuildingIsInvisible == phase = "building" => visible = "none"

ReadyImageIsComplete ==
    /\ (phase = "ready") <=> (visible = "complete")
    /\ visible # "partial"

RefusalOnlyWhenTooLarge == phase = "refused" => target > limit

WriterCacheMatchesIdentity ==
    writerCache = NoCache \/ writerCache = writerVersion

CloneCacheMatchesIdentity ==
    cloneCache = NoCache \/ cloneCache = cloneVersion

UntouchedCloneSurvivesWriterMutation ==
    mutated =>
      /\ ~sharedAux
      /\ cloneVersion = savedCloneVersion
      /\ cloneCache = savedCloneCache

EveryReadIsComplete == lastRead # "partial"

(***************************************************************************
Mutants. The proof harness requires each mutant to violate its named
invariant, calibrating the exact-cap, atomic-publication, and stale-cache
properties against concrete bad transitions.
***************************************************************************)
StrictCapRefusalMutant ==
    /\ phase = "cold"
    /\ target = limit
    /\ phase' = "refused"
    /\ UNCHANGED <<target, limit, built, visible,
                    writerVersion, writerCache,
                    hasClone, cloneVersion, cloneCache, sharedAux,
                    mutated, savedCloneVersion, savedCloneCache, lastRead>>

PartialPublicationMutant ==
    /\ phase = "building"
    /\ built < target
    /\ visible' = "partial"
    /\ UNCHANGED <<target, limit, built, phase,
                    writerVersion, writerCache,
                    hasClone, cloneVersion, cloneCache, sharedAux,
                    mutated, savedCloneVersion, savedCloneCache, lastRead>>

StaleCacheMutationMutant ==
    /\ hasClone
    /\ sharedAux
    /\ writerVersion = 0
    /\ writerCache = 0
    /\ writerVersion' = 1
    /\ writerCache' = 0
    /\ phase' = "cold"
    /\ visible' = "none"
    /\ built' = 0
    /\ sharedAux' = FALSE
    /\ mutated' = TRUE
    /\ savedCloneVersion' = cloneVersion
    /\ savedCloneCache' = cloneCache
    /\ UNCHANGED <<target, limit, hasClone, cloneVersion, cloneCache, lastRead>>

CapMutantNext == Next \/ StrictCapRefusalMutant
CapMutantSpec == Init /\ [][CapMutantNext]_vars

PartialMutantNext == Next \/ PartialPublicationMutant
PartialMutantSpec == Init /\ [][PartialMutantNext]_vars

StaleMutantNext == Next \/ StaleCacheMutationMutant
StaleMutantSpec == Init /\ [][StaleMutantNext]_vars

=============================================================================
