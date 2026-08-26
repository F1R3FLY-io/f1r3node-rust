-------------------- MODULE RSpaceSplitIndexAuthority --------------------
EXTENDS FiniteSets, Naturals, TLC

(***************************************************************************
Finite-state publication model for the continuation/join split PathMap index.

Groups, canonical records, continuation identities, and aggregate counters
are distinct copy-on-write roots. A writer stages all four roots privately,
seals only the complete staged authority, and publishes one simultaneous root
tuple. Readers therefore observe the old epoch or the new epoch, never a
mixture. Verus owns content coherence inside an epoch; this model owns the
multi-root lifecycle.
***************************************************************************)

Roots == {"groups", "records", "identities", "counters"}
Phases == {"idle", "staging", "sealed", "committed"}
Epochs == 0..1

VARIABLES phase, visibleEpoch, stagedRoots, sealObserved, publications

vars == <<phase, visibleEpoch, stagedRoots, sealObserved, publications>>

Init ==
    /\ phase = "idle"
    /\ visibleEpoch = [root \in Roots |-> 0]
    /\ stagedRoots = {}
    /\ sealObserved = FALSE
    /\ publications = 0

BeginStaging ==
    /\ phase = "idle"
    /\ publications = 0
    /\ phase' = "staging"
    /\ UNCHANGED <<visibleEpoch, stagedRoots, sealObserved, publications>>

StageRoot(root) ==
    /\ phase = "staging"
    /\ root \in Roots \ stagedRoots
    /\ stagedRoots' = stagedRoots \union {root}
    /\ UNCHANGED <<phase, visibleEpoch, sealObserved, publications>>

StageAnyRoot == \E root \in Roots: StageRoot(root)

SealCompleteAuthority ==
    /\ phase = "staging"
    /\ stagedRoots = Roots
    /\ phase' = "sealed"
    /\ sealObserved' = TRUE
    /\ UNCHANGED <<visibleEpoch, stagedRoots, publications>>

PublishCompleteAuthority ==
    /\ phase = "sealed"
    /\ sealObserved
    /\ stagedRoots = Roots
    /\ phase' = "committed"
    /\ visibleEpoch' = [root \in Roots |-> 1]
    /\ publications' = publications + 1
    /\ UNCHANGED <<stagedRoots, sealObserved>>

Next == BeginStaging \/ StageAnyRoot \/ SealCompleteAuthority
        \/ PublishCompleteAuthority

Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ phase \in Phases
    /\ visibleEpoch \in [Roots -> Epochs]
    /\ stagedRoots \subseteq Roots
    /\ sealObserved \in BOOLEAN
    /\ publications \in 0..1

PrivateStagingIsInvisible ==
    phase \in {"idle", "staging", "sealed"} =>
      \A root \in Roots: visibleEpoch[root] = 0

SealRequiresCompleteStaging ==
    phase = "sealed" => stagedRoots = Roots

PublishedAuthorityIsComplete ==
    phase = "committed" => stagedRoots = Roots

VisibleRootsAreAtomic ==
    \A root \in Roots: visibleEpoch[root] = visibleEpoch["groups"]

PublicationRequiresSealedAuthority ==
    publications = 1 => /\ phase = "committed"
                        /\ sealObserved

PublicationIsExactlyOnce ==
    /\ publications = 0 =>
         \A root \in Roots: visibleEpoch[root] = 0
    /\ publications = 1 =>
         \A root \in Roots: visibleEpoch[root] = 1

CountersCoPublishWithCanonicalRecords ==
    visibleEpoch["counters"] = visibleEpoch["records"]

IdentityBindingsCoPublishWithGroups ==
    visibleEpoch["identities"] = visibleEpoch["groups"]

(***************************************************************************
Negative control: publish the canonical-record root without the other three
authority roots. The atomic-root invariant must exhibit the mixed epoch.
***************************************************************************)
PublishPartialRootsMutant ==
    /\ phase = "sealed"
    /\ phase' = "committed"
    /\ visibleEpoch' = [visibleEpoch EXCEPT !["records"] = 1]
    /\ publications' = publications + 1
    /\ UNCHANGED <<stagedRoots, sealObserved>>

PartialRootsMutantNext == Next \/ PublishPartialRootsMutant
PartialRootsMutantSpec == Init /\ [][PartialRootsMutantNext]_vars

(***************************************************************************
Negative control: mark a strict subset of roots sealed. The complete-staging
invariant must reject the authority before publication.
***************************************************************************)
SealIncompleteAuthorityMutant ==
    /\ phase = "staging"
    /\ stagedRoots # Roots
    /\ phase' = "sealed"
    /\ sealObserved' = TRUE
    /\ UNCHANGED <<visibleEpoch, stagedRoots, publications>>

IncompleteSealMutantNext == Next \/ SealIncompleteAuthorityMutant
IncompleteSealMutantSpec == Init /\ [][IncompleteSealMutantNext]_vars

(***************************************************************************
Negative control: atomically publish all roots directly from private staging.
Although the visible tuple is not mixed, no complete seal authorized it.
***************************************************************************)
PublishBeforeSealMutant ==
    /\ phase = "staging"
    /\ ~sealObserved
    /\ phase' = "committed"
    /\ visibleEpoch' = [root \in Roots |-> 1]
    /\ publications' = publications + 1
    /\ UNCHANGED <<stagedRoots, sealObserved>>

PublishBeforeSealMutantNext == Next \/ PublishBeforeSealMutant
PublishBeforeSealMutantSpec == Init /\ [][PublishBeforeSealMutantNext]_vars

=============================================================================
