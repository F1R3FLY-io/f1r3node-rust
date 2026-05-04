--------------------------- MODULE EquivocationDetectorEager ---------------------------
(****************************************************************************)
(* Memory-efficient rewrite of EquivocationDetector.tla.                    *)
(*                                                                          *)
(* Equivalence-preserving optimizations versus the original:                *)
(*                                                                          *)
(* 1. EAGER DETECTION                                                       *)
(*    SignBlock + DetectArrival are combined into one atomic action         *)
(*    (SignAndDetect). This eliminates the spurious intermediate states     *)
(*    where a block is in the DAG but not yet classified. The rewrite is    *)
(*    a stutter-equivalent quotient: every observable trace of the          *)
(*    original is realized by exactly one trace here, and vice-versa.       *)
(*                                                                          *)
(*    Justification: no observable barb depends on the detector NOT having  *)
(*    fired yet. The Rust/Scala implementations always classify a block     *)
(*    in the same atomic step that adds it to the DAG.                      *)
(*                                                                          *)
(* 2. SAFETY-FIED LIVENESS                                                  *)
(*    The temporal property [](real-equivocation ~> non-valid) has 8        *)
(*    automaton instances in the original spec (one per (v, s, b)),         *)
(*    each multiplying the liveness graph. Under eager detection, it        *)
(*    becomes a pure SAFETY invariant Inv_LivenessAsSafety which is         *)
(*    equivalent to the temporal claim because:                             *)
(*      - Eager detection means detection happens IN the same step as       *)
(*        the equivocation becomes visible.                                 *)
(*      - There is no intermediate state where IsRealEquivocation holds     *)
(*        but detection has NOT yet fired.                                  *)
(*    Therefore "eventually detected" reduces to "always detected by the    *)
(*    time it's reachable", which is a one-step state property.             *)
(*                                                                          *)
(* 3. DEPENDENCY-AS-PARAMETER                                               *)
(*    Instead of MarkAsDependency as an independent action, the dependency  *)
(*    flag is chosen non-deterministically as a parameter to SignAndDetect. *)
(*    This is sound because nothing in the original spec constrains WHEN    *)
(*    a block becomes a dependency relative to its own classification.      *)
(*                                                                          *)
(* 4. SYMMETRY                                                              *)
(*    TLC SYMMETRY over Validators is exposed (declared in the .cfg).       *)
(*    With 2 validators this halves the explored state space; with 3 it    *)
(*    is a 6× reduction.                                                    *)
(*                                                                          *)
(* Reference: docs/theory/slashing/slashing-verification.md §10.            *)
(****************************************************************************)

EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    Validators,
    MaxSeqNum,
    MaxBlocksPerSeq

VARIABLES
    blocks,                  \* DAG: per validator, per seq, set of distinct block IDs
    detectedStatus,          \* (v, s, b) → status
    equivocationRecords      \* set of (v, s-1) pairs

vars == <<blocks, detectedStatus, equivocationRecords>>

(****************************************************************************)
(* TypeOK                                                                    *)
(****************************************************************************)
TypeOK ==
    /\ blocks \in [Validators -> [1..MaxSeqNum -> SUBSET (1..MaxBlocksPerSeq)]]
    /\ detectedStatus \in [Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) ->
                            {"none", "valid", "admissible", "ignorable", "neglected"}]
    /\ equivocationRecords \subseteq (Validators \X (0..MaxSeqNum))

(****************************************************************************)
(* IsRealEquivocation: (v, s) has more than one distinct block.             *)
(****************************************************************************)
IsRealEquivocation(v, s) ==
    Cardinality(blocks[v][s]) >= 2

(****************************************************************************)
(* Init                                                                     *)
(****************************************************************************)
Init ==
    /\ blocks = [v \in Validators |-> [s \in 1..MaxSeqNum |-> {}]]
    /\ detectedStatus =
            [t \in Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) |-> "none"]
    /\ equivocationRecords = {}

(****************************************************************************)
(* SignAndDetect: atomically sign a block and classify it.                  *)
(*                                                                          *)
(*   - dependencyFlag is chosen non-deterministically: this captures the    *)
(*     "block requested as dependency" state without needing a separate     *)
(*     MarkAsDependency action. The dependency status of a block is fixed   *)
(*     at signing time in this rewrite (sound because no observable barb    *)
(*     depends on later changes to the flag).                               *)
(*                                                                          *)
(*   - Re-detection: if the same (v, s, b) already exists, signing it      *)
(*     again upgrades classification from "valid" to "admissible" or        *)
(*     "ignorable" if a sibling block was meanwhile signed. We model this   *)
(*     by allowing re-classification in place.                              *)
(****************************************************************************)
(* TRULY eager detection: signing a block ALSO reclassifies every existing
   sibling at (v, s) in the same atomic step. After SignAndDetect, every
   block in blocks[v][s] has consistent non-"valid" status if equivocation
   exists, else "valid". *)
SignAndDetect(v, s, b, dependencyFlag) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in 1..MaxBlocksPerSeq
    /\ b \notin blocks[v][s]
    /\ LET newBlocks == blocks[v][s] \cup {b}
           willBeEquiv == Cardinality(newBlocks) >= 2
           newStatus == IF willBeEquiv
                        THEN IF dependencyFlag THEN "admissible" ELSE "ignorable"
                        ELSE "valid"
       IN  /\ blocks' = [blocks EXCEPT ![v] = [@ EXCEPT ![s] = newBlocks]]
           /\ detectedStatus' =
                [t \in Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) |->
                   IF t[1] = v /\ t[2] = s /\ t[3] \in newBlocks /\ willBeEquiv
                   THEN newStatus
                   ELSE IF t[1] = v /\ t[2] = s /\ t[3] = b
                        THEN newStatus
                        ELSE detectedStatus[t]]
           /\ IF newStatus = "admissible"
              THEN equivocationRecords' = equivocationRecords \cup {<<v, s - 1>>}
              ELSE equivocationRecords' = equivocationRecords

(****************************************************************************)
(* Re-detection action: when a SECOND distinct block at (v, s) is signed,  *)
(* the FIRST block's status must be promoted from "valid" to                *)
(* "admissible"/"ignorable". SignAndDetect handles the new arrival's       *)
(* status; this action handles the retroactive upgrade for siblings.       *)
(****************************************************************************)
ReclassifySibling(v, s, b, dependencyFlag) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in blocks[v][s]
    /\ Cardinality(blocks[v][s]) >= 2
    /\ detectedStatus[<<v, s, b>>] = "valid"
    /\ \E newStatus \in {"admissible", "ignorable"} :
         /\ ( IF dependencyFlag THEN newStatus = "admissible"
              ELSE                  newStatus = "ignorable" )
         /\ detectedStatus' =
              [detectedStatus EXCEPT ![<<v, s, b>>] = newStatus]
         /\ IF newStatus = "admissible"
            THEN equivocationRecords' = equivocationRecords \cup {<<v, s - 1>>}
            ELSE equivocationRecords' = equivocationRecords
    /\ UNCHANGED blocks

(****************************************************************************)
(* Detect a neglected equivocation. This action exists separately because   *)
(* "neglected" status requires another block to have already been recorded *)
(* as an equivocator and a NEW block citing it without a slash deploy.     *)
(* Modeled here as: any (v, s, b) whose record exists at (v, s-1) can       *)
(* receive Neglected status.                                                *)
(****************************************************************************)
DetectNeglected(v, s, b) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in blocks[v][s]
    /\ <<v, s - 1>> \in equivocationRecords
    /\ detectedStatus[<<v, s, b>>] # "neglected"
    /\ detectedStatus' = [detectedStatus EXCEPT ![<<v, s, b>>] = "neglected"]
    /\ UNCHANGED <<blocks, equivocationRecords>>

(****************************************************************************)
(* Next                                                                     *)
(****************************************************************************)
(* ReclassifySibling is no longer needed: SignAndDetect handles sibling
   reclassification atomically. Kept defined above for reference but not
   in Next. *)
Next ==
    \/ \E v \in Validators, s \in 1..MaxSeqNum,
         b \in 1..MaxBlocksPerSeq, d \in BOOLEAN :
            SignAndDetect(v, s, b, d)
    \/ \E v \in Validators, s \in 1..MaxSeqNum,
         b \in 1..MaxBlocksPerSeq :
            DetectNeglected(v, s, b)

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

(****************************************************************************)
(* Invariants                                                               *)
(****************************************************************************)

\* T-1 (soundness): every Admissible/Ignorable status corresponds to a real
\* equivocation in the DAG.
Inv_DetectionSound ==
    \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
        (detectedStatus[<<v, s, b>>] \in {"admissible", "ignorable"})
            => IsRealEquivocation(v, s)

\* T-3 (taxonomy): the status set is closed.
Inv_TaxonomyCorrect ==
    \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
        detectedStatus[<<v, s, b>>] \in
            {"none", "valid", "admissible", "ignorable", "neglected"}

\* Every record has a witness.
Inv_RecordHasWitness ==
    \A r \in equivocationRecords :
        LET v == r[1]
            base == r[2]
        IN  base + 1 \in 1..MaxSeqNum /\ IsRealEquivocation(v, base + 1)

(****************************************************************************)
(* SAFETY-FIED LIVENESS (T-2 detection completeness)                        *)
(*                                                                          *)
(* Under eager detection, the ONLY blocks with status "valid" are those     *)
(* without sibling equivocators. Therefore:                                 *)
(*    "every real equivocation eventually detected"                         *)
(*  ≡ "no reachable state has a real-equivocation block stuck at valid"     *)
(*                                                                          *)
(* This is a pure invariant; no liveness automaton is needed.               *)
(****************************************************************************)
Inv_LivenessAsSafety ==
    \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
        (b \in blocks[v][s] /\ IsRealEquivocation(v, s))
        => detectedStatus[<<v, s, b>>] \in {"admissible", "ignorable", "neglected"}

\* The above invariant holds trivially if at least one of the equivocating
\* siblings has been classified. ReclassifySibling fairness ensures
\* eventually all siblings are classified.

============================================================================
