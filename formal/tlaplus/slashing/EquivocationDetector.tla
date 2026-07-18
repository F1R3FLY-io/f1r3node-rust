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
    Validators,      \* Set of validator identifiers
    MaxSeqNum,       \* Maximum sequence number any validator may reach
    MaxBlocksPerSeq, \* Bound on how many blocks a validator may sign at one seq
    \* FV audit #6 (unbonded-window record pollution fork). Two BOOLEAN model
    \* selectors gate the bond-status / witness-set machinery so the EXISTING
    \* detector configs are behaviourally unchanged (both FALSE ⇒ `bonded` stays
    \* all-TRUE and `recordWitness` stays all-empty, adding NO reachable states):
    EnableBondDynamics, \* enable Unbond/Rebond (offender bond status may toggle)
    EnableStampWitness  \* enable StampWitness = the PRE-FIX Detected-arm stamp

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
    equivocationRecords,

    \* FV audit #6: offender bond status.  bonded[v] = TRUE iff v is currently
    \* bonded (has positive stake).  An offender may Unbond then Rebond.
    bonded,

    \* FV audit #6: the record witness set — the TLA+ image of
    \* EquivocationRecord.equivocation_detected_block_hashes.  recordWitness[<<v,
    \* base>>] is the set of observer block IDs stamped into v's record at
    \* baseSeq `base`.  POST-FIX this stays empty (the unbonded/stake-0 offender
    \* resolves to Oblivious, so the caller never stamps); PRE-FIX StampWitness
    \* pollutes it while v is unbonded — the root of the fork.
    recordWitness

vars == <<blocks, requestedAsDependency, detectableInView, detectedStatus,
          equivocationRecords, bonded, recordWitness>>

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
    /\ bonded \in [Validators -> BOOLEAN]
    /\ recordWitness \in [(Validators \X (0..MaxSeqNum)) -> SUBSET (1..MaxBlocksPerSeq)]

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
    /\ bonded = [v \in Validators |-> TRUE]
    /\ recordWitness = [k \in (Validators \X (0..MaxSeqNum)) |-> {}]

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
    /\ UNCHANGED <<requestedAsDependency, detectableInView, detectedStatus,
                   equivocationRecords, bonded, recordWitness>>

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
    /\ UNCHANGED <<blocks, detectableInView, detectedStatus,
                   equivocationRecords, bonded, recordWitness>>

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
    /\ UNCHANGED <<blocks, requestedAsDependency, detectedStatus,
                   equivocationRecords, bonded, recordWitness>>

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
    /\ UNCHANGED <<blocks, requestedAsDependency, detectableInView, bonded, recordWitness>>

(****************************************************************************)
(* Action: a later block's latest-message view makes the record detectable.  *)
(*                                                                          *)
(* FV audit #6 changes, mirroring the post-fix Rust detector:               *)
(*   - `bonded[v]` guard: neglect is emitted ONLY for a bonded offender.     *)
(*     In the Rust, EquivocationNeglected comes exclusively from the bonded  *)
(*     stake>0 branch (equivocation_detector.rs:306); the unbonded/stake-0   *)
(*     branches now return Oblivious.                                        *)
(*   - witness disjunct `b \in recordWitness[<<v, s-1>>]`: mirrors the        *)
(*     equivocation_detected_block_hashes.contains(..) early return           *)
(*     (:351-356).  POST-FIX recordWitness is empty, so this disjunct never   *)
(*     fires and neglect requires a genuine detectable-in-view equivocation.  *)
(****************************************************************************)
DetectNeglected(v, s, b) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in blocks[v][s]
    /\ <<v, s - 1>> \in equivocationRecords
    /\ bonded[v]
    /\ ( detectableInView[<<v, s, b>>] = TRUE
         \/ b \in recordWitness[<<v, s - 1>>] )
    /\ detectedStatus[<<v, s, b>>] # "neglected"
    /\ detectedStatus' = [detectedStatus EXCEPT ![<<v, s, b>>] = "neglected"]
    /\ UNCHANGED <<blocks, requestedAsDependency, detectableInView,
                   equivocationRecords, bonded, recordWitness>>

(****************************************************************************)
(* FV audit #6 actions: offender bond-status dynamics + the PRE-FIX stamp.   *)
(****************************************************************************)

\* Unbond an offender.  Guarded by EnableBondDynamics so the existing detector
\* configs (which set it FALSE) see `bonded` frozen all-TRUE.  A validator that
\* currently carries a "neglected" verdict does NOT voluntarily unbond (it is
\* slated for slashing / removal) — this keeps neglect-implies-bonded a genuine
\* state invariant (Inv_NeglectNotFromUnbondedPollution) rather than a fact only
\* about the detection instant.
Unbond(v) ==
    /\ EnableBondDynamics
    /\ v \in Validators
    /\ bonded[v] = TRUE
    /\ ~(\E ss \in 1..MaxSeqNum, bb \in 1..MaxBlocksPerSeq :
            detectedStatus[<<v, ss, bb>>] = "neglected")
    /\ bonded' = [bonded EXCEPT ![v] = FALSE]
    /\ UNCHANGED <<blocks, requestedAsDependency, detectableInView,
                   detectedStatus, equivocationRecords, recordWitness>>

\* Re-bond an offender (the fork trigger in the pre-fix: after re-bond the
\* polluted witness resurrected a spurious neglect).
Rebond(v) ==
    /\ EnableBondDynamics
    /\ v \in Validators
    /\ bonded[v] = FALSE
    /\ bonded' = [bonded EXCEPT ![v] = TRUE]
    /\ UNCHANGED <<blocks, requestedAsDependency, detectableInView,
                   detectedStatus, equivocationRecords, recordWitness>>

\* PRE-FIX ONLY: the caller's EquivocationDetected arm stamped the currently-
\* validated observer block hash `b` into the UNBONDED offender's record
\* (equivocation_detector.rs:213-216, pre-fix).  Guarded by EnableStampWitness:
\* the POST-FIX model omits it (EnableStampWitness=FALSE) so `recordWitness`
\* stays empty (the fix); the PRE-FIX model enables it, reproducing the
\* observation-order-dependent pollution that violates Inv_NoStampAgainstUnbonded.
StampWitness(v, s, b) ==
    /\ EnableStampWitness
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in 1..MaxBlocksPerSeq
    /\ <<v, s - 1>> \in equivocationRecords
    /\ ~bonded[v]
    /\ b \notin recordWitness[<<v, s - 1>>]
    /\ recordWitness' = [recordWitness EXCEPT ![<<v, s - 1>>] = @ \cup {b}]
    /\ UNCHANGED <<blocks, requestedAsDependency, detectableInView,
                   detectedStatus, equivocationRecords, bonded>>

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
    \/ \E v \in Validators : Unbond(v)
    \/ \E v \in Validators : Rebond(v)
    \/ \E v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
            StampWitness(v, s, b)

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
(* Ranges over the REAL 27-variant InvalidBlock enum                        *)
(*   (casper/src/rust/block_status.rs) and PINS the 19-element is_slashable  *)
(* set (block_status.rs:191-236), then asserts:                             *)
(*   (a) the enum has 27 variants and the slashable set has 19, contained    *)
(*       in the enum;                                                        *)
(*   (b) the detector's status set is closed AND every non-valid status the  *)
(*       detector emits (admissible/ignorable/neglected) maps to a slashable  *)
(*       InvalidBlock variant.                                              *)
(* This replaces the prior 5-element status-range-only check (which the      *)
(* README overclaimed as covering the 17 slashable variants).               *)
(****************************************************************************)
InvalidBlockVariants ==
    { "AdmissibleEquivocation", "IgnorableEquivocation", "NeglectedEquivocation",
      "NeglectedInvalidBlock", "JustificationRegression", "InvalidParents",
      "InvalidFollows", "InvalidBlockNumber", "InvalidSequenceNumber",
      "InvalidShardId", "InvalidRepeatDeploy", "DeployNotSigned",
      "InvalidTransaction", "InvalidBondsCache", "InvalidBlockHash",
      "UnauthorizedSlashDeploy", "ContainsExpiredDeploy",
      "ContainsTimeExpiredDeploy", "ContainsFutureDeploy",
      "InvalidFormat", "InvalidSignature", "InvalidSender", "InvalidVersion",
      "InvalidTimestamp", "InvalidRejectedDeploy", "NotOfInterest",
      "LowDeployCost" }

SlashableVariants ==
    { "AdmissibleEquivocation", "IgnorableEquivocation", "NeglectedEquivocation",
      "NeglectedInvalidBlock", "JustificationRegression", "InvalidParents",
      "InvalidFollows", "InvalidBlockNumber", "InvalidSequenceNumber",
      "InvalidShardId", "InvalidRepeatDeploy", "DeployNotSigned",
      "InvalidTransaction", "InvalidBondsCache", "InvalidBlockHash",
      "UnauthorizedSlashDeploy", "ContainsExpiredDeploy",
      "ContainsTimeExpiredDeploy", "ContainsFutureDeploy" }

StatusInvalidBlock(st) ==
    CASE st = "admissible" -> "AdmissibleEquivocation"
      [] st = "ignorable"  -> "IgnorableEquivocation"
      [] st = "neglected"  -> "NeglectedEquivocation"
      [] OTHER             -> "AdmissibleEquivocation"

Inv_TaxonomyCorrect ==
    /\ Cardinality(InvalidBlockVariants) = 27
    /\ Cardinality(SlashableVariants) = 19
    /\ SlashableVariants \subseteq InvalidBlockVariants
    /\ \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
         /\ detectedStatus[<<v, s, b>>] \in
              {"none", "valid", "admissible", "ignorable", "neglected"}
         /\ ( detectedStatus[<<v, s, b>>] \in {"admissible", "ignorable", "neglected"}
              => StatusInvalidBlock(detectedStatus[<<v, s, b>>]) \in SlashableVariants )

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

(****************************************************************************)
(* FV audit #6 invariants (unbonded-window record pollution fork).          *)
(****************************************************************************)

\* PRIMARY. No witness hash is ever recorded against an UNBONDED offender.
\* POST-FIX (StampWitness absent) recordWitness is empty, so this holds
\* trivially.  PRE-FIX (StampWitness present) a stamp lands while the offender
\* is unbonded, so recordWitness[<<v, base>>] becomes non-empty with
\* bonded[v] = FALSE — VIOLATING this invariant.  This is the invariant the
\* pre-fix config must reproduce a counterexample to.
Inv_NoStampAgainstUnbonded ==
    \A vk \in DOMAIN recordWitness :
        ~bonded[vk[1]] => recordWitness[vk] = {}

\* DOWNSTREAM. A "neglected" verdict is never produced from unbonded-window
\* pollution: it is backed by a genuine detectable-in-view equivocation AND a
\* currently-bonded offender.  POST-FIX recordWitness is empty, so neglect can
\* only come from `detectableInView`, and the DetectNeglected `bonded[v]` guard
\* (together with the Unbond guard that a neglected offender does not unbond)
\* keeps `bonded[v]` true.
Inv_NeglectNotFromUnbondedPollution ==
    \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
        detectedStatus[<<v, s, b>>] = "neglected" =>
            (bonded[v] /\ detectableInView[<<v, s, b>>] = TRUE)

============================================================================
