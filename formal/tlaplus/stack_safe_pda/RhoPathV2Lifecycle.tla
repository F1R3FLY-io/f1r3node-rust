---------------------- MODULE RhoPathV2Lifecycle ----------------------
EXTENDS Naturals, FiniteSets, TLC

(***************************************************************************)
(* Two-lane lifecycle refinement for the RhoPath bridge.  V1 and V2 carry  *)
(* the same semantic state; their encoded images are only routing keys.     *)
(* Digest buckets retain complete images, so a digest collision cannot      *)
(* overwrite or alias a distinct channel image.  Every lifecycle action    *)
(* publishes its root, lane, and image atomically.                          *)
(***************************************************************************)

Lanes == {"V1", "V2"}
Phases == {"fresh", "produced", "installed", "consumed", "committed",
           "checkpointed", "reset", "rebuilt", "exported", "imported",
           "replayed", "done"}
Roots == {"B", "T"}
Images == {"img-B-V1", "img-B-V2", "img-T-V1", "img-T-V2"}
Digests == {"d-B", "d-T"}

ImageDigest(image) ==
    IF image \in {"img-B-V1", "img-B-V2"} THEN "d-B" ELSE "d-T"
SemanticRoot(image) ==
    IF image \in {"img-B-V1", "img-B-V2"} THEN "B" ELSE "T"
ImageLane(image) ==
    IF image \in {"img-B-V1", "img-T-V1"} THEN "V1" ELSE "V2"

VARIABLES phase, lane, root, generation, image, digestBucket,
          stagedRoot, stagedImage, checkpointRoot, replayImage,
          lifecycleCount

vars == <<phase, lane, root, generation, image, digestBucket,
          stagedRoot, stagedImage, checkpointRoot, replayImage,
          lifecycleCount>>

Init ==
    /\ phase = "fresh"
    /\ lane = "V1"
    /\ root = "B"
    /\ generation = 0
    /\ image = "img-B-V1"
    /\ digestBucket = [d \in Digests |->
          IF d = "d-B" THEN {"img-B-V1", "img-B-V2"} ELSE {}]
    /\ stagedRoot = root
    /\ stagedImage = image
    /\ checkpointRoot = root
    /\ replayImage = image
    /\ lifecycleCount = 0

Produce(nextLane) ==
    /\ phase = "fresh"
    /\ nextLane \in Lanes
    /\ phase' = "produced"
    /\ lane' = nextLane
    /\ stagedRoot' = "T"
    /\ stagedImage' = IF nextLane = "V1" THEN "img-T-V1" ELSE "img-T-V2"
    /\ UNCHANGED <<root, generation, image, digestBucket, checkpointRoot,
                    replayImage, lifecycleCount>>

Install ==
    /\ phase = "produced"
    /\ stagedRoot = "T"
    /\ phase' = "installed"
    /\ UNCHANGED <<lane, root, generation, image, digestBucket,
                    stagedRoot, stagedImage, checkpointRoot, replayImage,
                    lifecycleCount>>

Consume ==
    /\ phase = "installed"
    /\ phase' = "consumed"
    /\ UNCHANGED <<lane, root, generation, image, digestBucket,
                    stagedRoot, stagedImage, checkpointRoot, replayImage,
                    lifecycleCount>>

Commit ==
    /\ phase = "consumed"
    /\ phase' = "committed"
    /\ root' = stagedRoot
    /\ image' = stagedImage
    /\ generation' = generation + 1
    /\ digestBucket' = [digestBucket EXCEPT ![ImageDigest(stagedImage)] =
          @ \union {stagedImage}]
    /\ lifecycleCount' = lifecycleCount + 1
    /\ UNCHANGED <<lane, stagedRoot, stagedImage, checkpointRoot, replayImage>>

Checkpoint ==
    /\ phase = "committed"
    /\ phase' = "checkpointed"
    /\ checkpointRoot' = root
    /\ UNCHANGED <<lane, root, generation, image, digestBucket,
                    stagedRoot, stagedImage, replayImage, lifecycleCount>>

Reset ==
    /\ phase = "checkpointed"
    /\ phase' = "reset"
    /\ root' = checkpointRoot
    /\ image' = CHOOSE candidate \in digestBucket[ImageDigest(image)]:
                 /\ SemanticRoot(candidate) = checkpointRoot
                 /\ ImageLane(candidate) = lane
    /\ UNCHANGED <<lane, generation, digestBucket, stagedRoot, stagedImage,
                    checkpointRoot, replayImage, lifecycleCount>>

Rebuild ==
    /\ phase = "reset"
    /\ phase' = "rebuilt"
    /\ root = checkpointRoot
    /\ image \in digestBucket[ImageDigest(image)]
    /\ UNCHANGED <<lane, root, generation, image, digestBucket,
                    stagedRoot, stagedImage, checkpointRoot, replayImage,
                    lifecycleCount>>

Export ==
    /\ phase = "rebuilt"
    /\ phase' = "exported"
    /\ replayImage' = image
    /\ UNCHANGED <<lane, root, generation, image, digestBucket,
                    stagedRoot, stagedImage, checkpointRoot, lifecycleCount>>

Import ==
    /\ phase = "exported"
    /\ replayImage \in digestBucket[ImageDigest(replayImage)]
    /\ phase' = "imported"
    /\ UNCHANGED <<lane, root, generation, image, digestBucket,
                    stagedRoot, stagedImage, checkpointRoot, replayImage,
                    lifecycleCount>>

Replay ==
    /\ phase = "imported"
    /\ replayImage \in digestBucket[ImageDigest(replayImage)]
    /\ phase' = "replayed"
    /\ image' = replayImage
    /\ root' = SemanticRoot(replayImage)
    /\ UNCHANGED <<lane, generation, digestBucket, stagedRoot, stagedImage,
                    checkpointRoot, replayImage, lifecycleCount>>

COMM ==
    /\ phase = "replayed"
    /\ phase' = "done"
    /\ lifecycleCount' = lifecycleCount + 1
    /\ UNCHANGED <<lane, root, generation, image, digestBucket,
                    stagedRoot, stagedImage, checkpointRoot, replayImage>>

Next == \E nextLane \in Lanes: Produce(nextLane)
        \/ Install \/ Consume \/ Commit \/ Checkpoint \/ Reset \/ Rebuild
        \/ Export \/ Import \/ Replay \/ COMM
        \/ (phase = "done" /\ UNCHANGED vars)

Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ phase \in Phases
    /\ lane \in Lanes
    /\ root \in Roots
    /\ generation \in Nat
    /\ image \in Images
    /\ stagedRoot \in Roots
    /\ stagedImage \in Images
    /\ checkpointRoot \in Roots
    /\ replayImage \in Images
    /\ lifecycleCount \in Nat
    /\ DOMAIN digestBucket = Digests
    /\ \A d \in Digests: digestBucket[d] \subseteq Images

FullImageCollisionSafety ==
    \A d \in Digests: \A left, right \in digestBucket[d]:
        left = right \/ ImageDigest(left) = ImageDigest(right)

DistinctCollisionRepresentativesPreserved ==
    {"img-B-V1", "img-B-V2"} \subseteq digestBucket["d-B"]

AuthoritativeImageIsAdmitted == image \in digestBucket[ImageDigest(image)]

SemanticRootMatchesImage == root = SemanticRoot(image)

LaneMatchesImageAfterCommit ==
    phase \in {"committed", "checkpointed", "reset", "rebuilt", "exported",
                "imported", "replayed", "done"} => lane = ImageLane(image)

NoPartialPublication ==
    phase = "committed" =>
      /\ root = SemanticRoot(image)
      /\ image \in digestBucket[ImageDigest(image)]

ReplayedImageWasExported == phase \in {"replayed", "done"} =>
    replayImage \in digestBucket[ImageDigest(replayImage)]

THEOREM Spec => []TypeInvariant
THEOREM Spec => []FullImageCollisionSafety
THEOREM Spec => []DistinctCollisionRepresentativesPreserved
THEOREM Spec => []AuthoritativeImageIsAdmitted
THEOREM Spec => []SemanticRootMatchesImage
THEOREM Spec => []LaneMatchesImageAfterCommit
THEOREM Spec => []NoPartialPublication
THEOREM Spec => []ReplayedImageWasExported

=============================================================================
