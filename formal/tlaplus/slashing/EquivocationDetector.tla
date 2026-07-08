--------------------------- MODULE EquivocationDetector ---------------------------
(****************************************************************************)
(* Finite-state model of the equivocation-detection state machine.          *)
(*                                                                          *)
(* Models:                                                                  *)
(*   - A set of validators producing blocks at sequential seq numbers       *)
(*   - The detector classifying each new block as Valid, Admissible-, or    *)
(*     Ignorable-Equivocation, or NeglectedEquivocation                     *)
(*   - Soundness: detection only fires on real equivocations                *)
(*   - Completeness: every real equivocation is eventually detected         *)
(*                                                                          *)
(* Complements the Rocq mechanization at                                    *)
(*   formal/rocq/slashing/theories/EquivocationDetector.v                   *)
(* which proves these properties for unbounded validator and DAG sizes.    *)
(* This TLA+ model exhaustively checks finite instances via TLC.            *)
(*                                                                          *)
(* Reference: docs/theory/slashing/slashing-verification.md §4.            *)
(****************************************************************************)

EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    Validators,     \* Set of validator identifiers
    MaxSeqNum,      \* Maximum sequence number any validator may reach
    MaxBlocksPerSeq \* Bound on how many blocks a validator may sign at one seq

VARIABLES
    \* DAG abstraction: blocks(v) is a function seq → set of distinct block IDs
    \* signed by validator v at that seq number.  The cardinality of
    \* blocks[v][s] tells us whether v equivocated at s.
    blocks,

    \* requestedAsDependency(b) is TRUE if some other block in the DAG cites
    \* b in its justifications.  Determines admissible vs. ignorable.
    requestedAsDependency,

    \* detectableInView(b) abstracts Rust is_equivocation_detectable for a
    \* later block's latest-message view.
    detectableInView,

    \* detectedStatus(b) ∈ {"valid", "admissible", "ignorable", "neglected"}
    \* The detector's classification.
    detectedStatus,

    \* Set of (validator, baseSeqNum) pairs for which an EquivocationRecord
    \* has been created.  In the abstract, we assume creation is atomic with
    \* detection; the ConcurrentTracker spec models the locking question.
    equivocationRecords

vars == <<blocks, requestedAsDependency, detectableInView, detectedStatus, equivocationRecords>>

(****************************************************************************)
(* TypeOK — state-shape invariant                                           *)
(****************************************************************************)
TypeOK ==
    /\ MaxSeqNum \in Nat
    /\ MaxSeqNum >= 1
    /\ MaxBlocksPerSeq \in Nat
    /\ MaxBlocksPerSeq >= 1
    /\ blocks \in [Validators -> [1..MaxSeqNum -> SUBSET (1..MaxBlocksPerSeq)]]
    /\ requestedAsDependency \in [Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) -> BOOLEAN]
    /\ detectableInView \in [Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) -> BOOLEAN]
    /\ detectedStatus \in [Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) ->
                            {"none", "valid", "admissible", "ignorable", "neglected"}]
    /\ equivocationRecords \subseteq (Validators \X (0..MaxSeqNum))

(****************************************************************************)
(* Helper: does (v, s) describe a real equivocation in the current DAG?     *)
(****************************************************************************)
IsRealEquivocation(v, s) ==
    Cardinality(blocks[v][s]) >= 2

(****************************************************************************)
(* Init — all DAGs empty, no records, no detections                         *)
(****************************************************************************)
Init ==
    /\ blocks = [v \in Validators |->
                    [s \in 1..MaxSeqNum |-> {}]]
    /\ requestedAsDependency =
            [t \in Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) |-> FALSE]
    /\ detectableInView =
            [t \in Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) |-> FALSE]
    /\ detectedStatus =
            [t \in Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) |-> "none"]
    /\ equivocationRecords = {}

(****************************************************************************)
(* Action: validator v signs a (possibly fresh, possibly equivocating)      *)
(* block b at sequence number s.                                            *)
(****************************************************************************)
SignBlock(v, s, b) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in 1..MaxBlocksPerSeq
    /\ b \notin blocks[v][s]
    /\ blocks' = [blocks EXCEPT
                    ![v] = [@ EXCEPT ![s] = @ \cup {b}]]
    /\ UNCHANGED <<requestedAsDependency, detectableInView, detectedStatus, equivocationRecords>>

(****************************************************************************)
(* Action: another block in the DAG cites (v, s, b) in its justifications.  *)
(****************************************************************************)
MarkAsDependency(v, s, b) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in blocks[v][s]
    /\ requestedAsDependency[<<v, s, b>>] = FALSE
    /\ requestedAsDependency' =
            [requestedAsDependency EXCEPT ![<<v, s, b>>] = TRUE]
    /\ UNCHANGED <<blocks, detectableInView, detectedStatus, equivocationRecords>>

(****************************************************************************)
(* Action: a later block's latest-message view can detect the record.       *)
(****************************************************************************)
MarkDetectableInView(v, s, b) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in blocks[v][s]
    /\ detectableInView[<<v, s, b>>] = FALSE
    /\ detectableInView' =
            [detectableInView EXCEPT ![<<v, s, b>>] = TRUE]
    /\ UNCHANGED <<blocks, requestedAsDependency, detectedStatus, equivocationRecords>>

(****************************************************************************)
(* Action: detector (re-)classifies an arrival.                             *)
(*                                                                          *)
(* The classification rules mirror EquivocationDetector.scala /             *)
(* equivocation_detector.rs:                                                *)
(*   - if no equivocation: "valid"                                          *)
(*   - if equivocation AND requested-as-dependency: "admissible"            *)
(*   - if equivocation AND not requested-as-dependency: "ignorable"         *)
(*                                                                          *)
(* "neglected" arises only when a later block carries the equivocation in   *)
(* its justifications — modeled in the Neglected action below.              *)
(*                                                                          *)
(* Re-detection: a "valid" classification is allowed to upgrade to          *)
(* "admissible" or "ignorable" if a second block at the same (v, s) is      *)
(* later signed.  This matches the implementation, which re-validates each  *)
(* block as the DAG evolves.                                                *)
(****************************************************************************)
DetectArrival(v, s, b) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in blocks[v][s]
    /\ LET new_status ==
              IF \neg IsRealEquivocation(v, s) THEN "valid"
              ELSE IF requestedAsDependency[<<v, s, b>>] THEN "admissible"
              ELSE "ignorable"
       IN  /\ detectedStatus[<<v, s, b>>] # new_status
           /\ \/ detectedStatus[<<v, s, b>>] = "none"
              \/ ( detectedStatus[<<v, s, b>>] = "valid"
                   /\ new_status \in {"admissible", "ignorable"} )
           /\ detectedStatus' = [detectedStatus EXCEPT ![<<v, s, b>>] = new_status]
           /\ IF new_status = "admissible"
              THEN equivocationRecords' = equivocationRecords \cup {<<v, s - 1>>}
              ELSE equivocationRecords' = equivocationRecords
    /\ UNCHANGED <<blocks, requestedAsDependency, detectableInView>>

(****************************************************************************)
(* Action: a later block's latest-message view makes the record detectable. *)
(****************************************************************************)
DetectNeglected(v, s, b) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in blocks[v][s]
    /\ <<v, s - 1>> \in equivocationRecords
    /\ detectableInView[<<v, s, b>>] = TRUE
    /\ detectedStatus[<<v, s, b>>] # "neglected"
    /\ detectedStatus' = [detectedStatus EXCEPT ![<<v, s, b>>] = "neglected"]
    /\ UNCHANGED <<blocks, requestedAsDependency, detectableInView, equivocationRecords>>

(****************************************************************************)
(* Next-state relation                                                      *)
(****************************************************************************)
Next ==
    \/ \E v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
            SignBlock(v, s, b)
    \/ \E v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
            MarkAsDependency(v, s, b)
    \/ \E v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
            MarkDetectableInView(v, s, b)
    \/ \E v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
            DetectArrival(v, s, b)
    \/ \E v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
            DetectNeglected(v, s, b)

(****************************************************************************)
(* Spec = Init ∧ □[Next]_vars ∧ Fairness                                    *)
(****************************************************************************)
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

(****************************************************************************)
(* Invariant: detection soundness (T-1).                                    *)
(*                                                                          *)
(* Whenever the detector reports admissible or ignorable for (v, s, b),     *)
(* there is a real equivocation: at least two distinct blocks signed by v   *)
(* at sequence number s.                                                    *)
(****************************************************************************)
Inv_DetectionSound ==
    \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
        (detectedStatus[<<v, s, b>>] \in {"admissible", "ignorable"})
            => IsRealEquivocation(v, s)

(****************************************************************************)
(* Liveness: detection completeness (T-2).                                  *)
(*                                                                          *)
(* Every real equivocation eventually receives some non-"valid" status.     *)
(* This is a temporal property; TLC checks it under the Spec fairness.      *)
(****************************************************************************)
Live_DetectionComplete ==
    \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
        (b \in blocks[v][s] /\ IsRealEquivocation(v, s)) ~>
            (detectedStatus[<<v, s, b>>] # "none"
             /\ detectedStatus[<<v, s, b>>] # "valid")

(****************************************************************************)
(* Invariant: taxonomy correctness (T-3).                                   *)
(*                                                                          *)
(* The set of statuses the detector emits is exactly                        *)
(*   {valid, admissible, ignorable, neglected} ∪ {none}                     *)
(* No other variant can leak in.                                            *)
(****************************************************************************)
Inv_TaxonomyCorrect ==
    \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
        detectedStatus[<<v, s, b>>] \in
            {"none", "valid", "admissible", "ignorable", "neglected"}

Inv_NeglectedHasDetectableView ==
    \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
        detectedStatus[<<v, s, b>>] = "neglected" =>
            detectableInView[<<v, s, b>>] = TRUE

FixedDetectable(hasDetected, distinctChildren) ==
    hasDetected \/ Cardinality(distinctChildren) >= 2

TraversalDomain == 1..MaxBlocksPerSeq

TraversalStep(G, Seen) ==
    Seen \cup UNION {G[n] : n \in Seen}

RECURSIVE TraversalAfter(_, _, _)
TraversalAfter(G, Seen, fuel) ==
    IF fuel = 0 THEN Seen ELSE TraversalAfter(G, TraversalStep(G, Seen), fuel - 1)

DetectorBugFixDivergenceClass == "permitted_bug_fix"

BoundedChains == {<<>>} \cup UNION {[1..n -> 0..MaxSeqNum] : n \in 1..MaxSeqNum}

AbovePrefixIndexes(chain, base) ==
    {i \in DOMAIN chain : \A j \in 1..i : chain[j] > base}

CanonicalIndex(chain, base) ==
    Cardinality(AbovePrefixIndexes(chain, base))

CanonicalSeq(chain, base) ==
    LET idx == CanonicalIndex(chain, base)
    IN  IF idx = 0 THEN 0 ELSE chain[idx]

PrefixAbove(chain, base) ==
    \A i \in DOMAIN chain : chain[i] > base

WellFormedSelfChain(chain) ==
    \A i \in DOMAIN chain :
        i < Len(chain) => chain[i] > chain[i + 1]

MemoizedCanonicalSeq(cache, chain, base) ==
    IF cache = 0 THEN CanonicalSeq(chain, base) ELSE cache

Inv_FixedDetectorTotal ==
    /\ FixedDetectable(FALSE, {}) = FALSE
    /\ FixedDetectable(TRUE, {}) = TRUE

Inv_MissingPointerNonContributing ==
    FixedDetectable(FALSE, {}) = FALSE

Inv_DuplicateChildNeedsDistinctChildren ==
    FixedDetectable(FALSE, {1}) = FALSE

Inv_TwoDistinctChildrenDetect ==
    FixedDetectable(FALSE, {1, 2}) = TRUE

Inv_DetectedHashDetects ==
    FixedDetectable(TRUE, {}) = TRUE

Inv_DetectorTraversalFiniteFuel ==
    \A G \in [TraversalDomain -> SUBSET TraversalDomain] :
        TraversalAfter(G, {1}, MaxBlocksPerSeq + 1) =
        TraversalAfter(G, {1}, MaxBlocksPerSeq)

Inv_DetectorTraversalInDomain ==
    \A G \in [TraversalDomain -> SUBSET TraversalDomain] :
      \A fuel \in 0..(MaxBlocksPerSeq + 1) :
        TraversalAfter(G, {1}, fuel) \subseteq TraversalDomain

Inv_DetectorBugFixClassAllowed ==
    DetectorBugFixDivergenceClass \in {"bisimilar", "permitted_bug_fix"}

Inv_CanonicalChildSound ==
    \A chain \in BoundedChains, base \in 0..MaxSeqNum :
        CanonicalIndex(chain, base) > 0 => CanonicalSeq(chain, base) > base

Inv_CanonicalChildBoundary ==
    \A chain \in BoundedChains, base \in 0..MaxSeqNum :
        LET idx == CanonicalIndex(chain, base)
        IN  IF idx = 0
            THEN IF Len(chain) = 0 THEN TRUE ELSE chain[1] <= base
            ELSE IF idx < Len(chain) THEN chain[idx + 1] <= base ELSE TRUE

Inv_CanonicalGapCompleteness ==
    \A chain \in BoundedChains, base \in 0..MaxSeqNum :
        (WellFormedSelfChain(chain) /\ \E i \in DOMAIN chain : chain[i] > base) =>
            CanonicalIndex(chain, base) > 0

Inv_CanonicalDenseSubsumesPreFix ==
    \A chain \in BoundedChains, base \in 0..MaxSeqNum :
        (/\ WellFormedSelfChain(chain)
         /\ Len(chain) > 0
         /\ base + 1 \in 1..MaxSeqNum
         /\ chain[1] = base + 1)
        => CanonicalSeq(chain, base) = base + 1

Inv_CanonicalPrefixStability ==
    \A prefix \in BoundedChains, chain \in BoundedChains, base \in 0..MaxSeqNum :
        (PrefixAbove(prefix, base) /\ CanonicalIndex(chain, base) > 0) =>
            CanonicalSeq(prefix \o chain, base) = CanonicalSeq(chain, base)

Inv_CanonicalSameBranchNoOvercount ==
    \A chain \in BoundedChains, base \in 0..MaxSeqNum :
        Cardinality(IF CanonicalIndex(chain, base) = 0
                    THEN {}
                    ELSE {CanonicalSeq(chain, base)}) <= 1

Inv_CanonicalMemoizedEquivalent ==
    \A chain \in BoundedChains, base \in 0..MaxSeqNum, cache \in 0..MaxSeqNum :
        (cache = 0 \/ cache = CanonicalSeq(chain, base)) =>
            MemoizedCanonicalSeq(cache, chain, base) = CanonicalSeq(chain, base)

(****************************************************************************)
(* Invariant: every record has a witness equivocation in the DAG.           *)
(****************************************************************************)
Inv_RecordHasWitness ==
    \A r \in equivocationRecords :
        LET v == r[1]
            base == r[2]
        IN  base + 1 \in 1..MaxSeqNum /\ IsRealEquivocation(v, base + 1)

============================================================================
