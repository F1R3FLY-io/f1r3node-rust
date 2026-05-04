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

    \* detectedStatus(b) ∈ {"valid", "admissible", "ignorable", "neglected"}
    \* The detector's classification.
    detectedStatus,

    \* Set of (validator, baseSeqNum) pairs for which an EquivocationRecord
    \* has been created.  In the abstract, we assume creation is atomic with
    \* detection; the ConcurrentTracker spec models the locking question.
    equivocationRecords

vars == <<blocks, requestedAsDependency, detectedStatus, equivocationRecords>>

(****************************************************************************)
(* TypeOK — state-shape invariant                                           *)
(****************************************************************************)
TypeOK ==
    /\ blocks \in [Validators -> [1..MaxSeqNum -> SUBSET (1..MaxBlocksPerSeq)]]
    /\ requestedAsDependency \in [Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) -> BOOLEAN]
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
    /\ UNCHANGED <<requestedAsDependency, detectedStatus, equivocationRecords>>

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
    /\ UNCHANGED <<blocks, detectedStatus, equivocationRecords>>

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
    /\ UNCHANGED <<blocks, requestedAsDependency>>

(****************************************************************************)
(* Action: a *later* block in the DAG carries an equivocation as one of its *)
(* justifications without slashing — yields "neglected".                    *)
(****************************************************************************)
DetectNeglected(v, s, b) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in blocks[v][s]
    /\ <<v, s - 1>> \in equivocationRecords
    /\ requestedAsDependency[<<v, s, b>>] = TRUE
    /\ detectedStatus[<<v, s, b>>] # "neglected"
    /\ detectedStatus' = [detectedStatus EXCEPT ![<<v, s, b>>] = "neglected"]
    /\ UNCHANGED <<blocks, requestedAsDependency, equivocationRecords>>

(****************************************************************************)
(* Next-state relation                                                      *)
(****************************************************************************)
Next ==
    \/ \E v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
            SignBlock(v, s, b)
    \/ \E v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
            MarkAsDependency(v, s, b)
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

(****************************************************************************)
(* Invariant: every record has a witness equivocation in the DAG.           *)
(****************************************************************************)
Inv_RecordHasWitness ==
    \A r \in equivocationRecords :
        LET v == r[1]
            base == r[2]
        IN  base + 1 \in 1..MaxSeqNum /\ IsRealEquivocation(v, base + 1)

============================================================================
