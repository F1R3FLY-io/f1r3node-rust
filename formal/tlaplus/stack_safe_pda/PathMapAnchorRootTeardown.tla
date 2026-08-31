-------------------- MODULE PathMapAnchorRootTeardown --------------------
EXTENDS Naturals, TLC

(***************************************************************************
Finite concurrency refinement of Arc<AnchorRoot<PathMap<V>>> ownership.

An immutable wrapper clone increments only one anchor Arc count.  Exactly one
raw PathMap header and one root-value image live in each active anchor.
Arc finalization linearizes the 1->0 transition and starts exactly one reverse-
prune PDA.  Arc::make_mut is the only operation that creates a second anchor;
it atomically moves one wrapper and installs one raw header/root image in the
fresh anchor.  TLC explores both orders of final drops after make_mut.
***************************************************************************)

Events == {"init", "clone-arc", "drop-nonlast", "drop-last-old",
            "drop-last-fresh", "make-mut", "abort-make-mut",
            "snapshot-race", "per-wrapper-header", "manual-family-drop",
            "unregistered-make-mut"}

VARIABLES oldArcs, freshArcs, cloneDone, makeMutDone,
          oldRaw, freshRaw, oldRoot, freshRoot,
          oldPda, freshPda, lastEvent

vars == <<oldArcs, freshArcs, cloneDone, makeMutDone,
          oldRaw, freshRaw, oldRoot, freshRoot,
          oldPda, freshPda, lastEvent>>

Init ==
    /\ oldArcs = 1
    /\ freshArcs = 0
    /\ cloneDone = FALSE
    /\ makeMutDone = FALSE
    /\ oldRaw = 1
    /\ freshRaw = 0
    /\ oldRoot = 1
    /\ freshRoot = 0
    /\ oldPda = 0
    /\ freshPda = 0
    /\ lastEvent = "init"

CloneArc ==
    /\ oldArcs = 1
    /\ ~cloneDone
    /\ oldArcs' = 2
    /\ cloneDone' = TRUE
    /\ lastEvent' = "clone-arc"
    /\ UNCHANGED <<freshArcs, makeMutDone, oldRaw, freshRaw,
                    oldRoot, freshRoot, oldPda, freshPda>>

DropOldNonlast ==
    /\ oldArcs = 2
    /\ oldArcs' = 1
    /\ lastEvent' = "drop-nonlast"
    /\ UNCHANGED <<freshArcs, cloneDone, makeMutDone, oldRaw, freshRaw,
                    oldRoot, freshRoot, oldPda, freshPda>>

DropOldLast ==
    /\ oldArcs = 1
    /\ oldPda = 0
    /\ oldArcs' = 0
    /\ oldRaw' = 0
    /\ oldRoot' = 0
    /\ oldPda' = 1
    /\ lastEvent' = "drop-last-old"
    /\ UNCHANGED <<freshArcs, cloneDone, makeMutDone, freshRaw,
                    freshRoot, freshPda>>

MakeMut ==
    /\ oldArcs = 2
    /\ freshArcs = 0
    /\ ~makeMutDone
    /\ oldArcs' = 1
    /\ freshArcs' = 1
    /\ makeMutDone' = TRUE
    /\ freshRaw' = 1
    /\ freshRoot' = 1
    /\ lastEvent' = "make-mut"
    /\ UNCHANGED <<cloneDone, oldRaw, oldRoot, oldPda, freshPda>>

AbortMakeMut ==
    /\ oldArcs = 2
    /\ freshArcs = 0
    /\ ~makeMutDone
    /\ lastEvent' = "abort-make-mut"
    /\ UNCHANGED <<oldArcs, freshArcs, cloneDone, makeMutDone,
                    oldRaw, freshRaw, oldRoot, freshRoot, oldPda, freshPda>>

DropFreshLast ==
    /\ freshArcs = 1
    /\ freshPda = 0
    /\ freshArcs' = 0
    /\ freshRaw' = 0
    /\ freshRoot' = 0
    /\ freshPda' = 1
    /\ lastEvent' = "drop-last-fresh"
    /\ UNCHANGED <<oldArcs, cloneDone, makeMutDone, oldRaw,
                    oldRoot, oldPda>>

Next == CloneArc \/ DropOldNonlast \/ DropOldLast \/ MakeMut \/
        AbortMakeMut \/ DropFreshLast
Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ oldArcs \in 0..2
    /\ freshArcs \in 0..1
    /\ cloneDone \in BOOLEAN
    /\ makeMutDone \in BOOLEAN
    /\ oldRaw \in 0..1
    /\ freshRaw \in 0..1
    /\ oldRoot \in 0..1
    /\ freshRoot \in 0..1
    /\ oldPda \in 0..1
    /\ freshPda \in 0..1
    /\ lastEvent \in Events

OneRawHeaderPerActiveAnchor ==
    /\ oldRaw = IF oldArcs > 0 THEN 1 ELSE 0
    /\ freshRaw = IF freshArcs > 0 THEN 1 ELSE 0

HeaderAndRootRegistrationAgree ==
    /\ oldRaw = oldRoot
    /\ freshRaw = freshRoot

DeadAnchorRequiresExactlyOnePda ==
    /\ (oldPda = 1) <=> (oldArcs = 0)
    /\ (freshPda = 1) <=> (makeMutDone /\ freshArcs = 0)

NonlastDropTouchesNoRawHeader ==
    lastEvent = "drop-nonlast" =>
      /\ oldArcs = 1
      /\ oldRaw = 1
      /\ oldRoot = 1
      /\ oldPda = 0

MakeMutCreatesOneRegisteredFreshAnchor ==
    makeMutDone =>
      /\ (freshArcs = 1 \/ freshPda = 1)
      /\ freshRaw = freshRoot

(***************************************************************************
Causal controls.
***************************************************************************)

SnapshotStrongCountRace ==
    /\ oldArcs = 2
    /\ oldArcs' = 0
    /\ oldRaw' = 0
    /\ oldRoot' = 0
    /\ oldPda' = 0
    /\ lastEvent' = "snapshot-race"
    /\ UNCHANGED <<freshArcs, cloneDone, makeMutDone, freshRaw,
                    freshRoot, freshPda>>
SnapshotMutantSpec == Init /\ [][Next \/ SnapshotStrongCountRace]_vars

PerWrapperRawHeaderClone ==
    /\ oldArcs = 1
    /\ ~cloneDone
    /\ oldArcs' = 2
    /\ oldRaw' = 2
    /\ oldRoot' = 2
    /\ cloneDone' = TRUE
    /\ lastEvent' = "per-wrapper-header"
    /\ UNCHANGED <<freshArcs, makeMutDone, freshRaw, freshRoot,
                    oldPda, freshPda>>
PerWrapperHeaderMutantSpec ==
    Init /\ [][Next \/ PerWrapperRawHeaderClone]_vars

ManualFamilyCounterDrop ==
    /\ oldArcs = 2
    /\ oldArcs' = 1
    /\ oldRaw' = 0
    /\ oldRoot' = 0
    /\ lastEvent' = "manual-family-drop"
    /\ UNCHANGED <<freshArcs, cloneDone, makeMutDone, freshRaw,
                    freshRoot, oldPda, freshPda>>
ManualFamilyMutantSpec ==
    Init /\ [][Next \/ ManualFamilyCounterDrop]_vars

UnregisteredMakeMut ==
    /\ oldArcs = 2
    /\ freshArcs = 0
    /\ ~makeMutDone
    /\ oldArcs' = 1
    /\ freshArcs' = 1
    /\ makeMutDone' = TRUE
    /\ freshRaw' = 1
    /\ freshRoot' = 0
    /\ lastEvent' = "unregistered-make-mut"
    /\ UNCHANGED <<cloneDone, oldRaw, oldRoot, oldPda, freshPda>>
UnregisteredMakeMutSpec ==
    Init /\ [][Next \/ UnregisteredMakeMut]_vars

=============================================================================
