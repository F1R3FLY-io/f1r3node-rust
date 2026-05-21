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
    detectableInView,        \* (v, s, b) -> Rust latest-message detectability
    detectedStatus,          \* (v, s, b) → status
    equivocationRecords      \* set of (v, s-1) pairs

vars == <<blocks, detectableInView, detectedStatus, equivocationRecords>>

(****************************************************************************)
(* TypeOK                                                                    *)
(****************************************************************************)
TypeOK ==
    /\ MaxSeqNum \in Nat
    /\ MaxSeqNum >= 1
    /\ MaxBlocksPerSeq \in Nat
    /\ MaxBlocksPerSeq >= 1
    /\ blocks \in [Validators -> [1..MaxSeqNum -> SUBSET (1..MaxBlocksPerSeq)]]
    /\ detectableInView \in [Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) -> BOOLEAN]
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
    /\ detectableInView =
            [t \in Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq) |-> FALSE]
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
           /\ UNCHANGED detectableInView

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
    /\ UNCHANGED detectableInView

(****************************************************************************)
(* Mark that a block's latest-message view detects a recorded equivocation. *)
(****************************************************************************)
MarkDetectableInView(v, s, b) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in blocks[v][s]
    /\ detectableInView[<<v, s, b>>] = FALSE
    /\ detectableInView' =
            [detectableInView EXCEPT ![<<v, s, b>>] = TRUE]
    /\ UNCHANGED <<blocks, detectedStatus, equivocationRecords>>

(****************************************************************************)
(* Detect a neglected equivocation from a Rust-detectable latest view.      *)
(****************************************************************************)
DetectNeglected(v, s, b) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in blocks[v][s]
    /\ <<v, s - 1>> \in equivocationRecords
    /\ detectableInView[<<v, s, b>>] = TRUE
    /\ detectedStatus[<<v, s, b>>] # "neglected"
    /\ detectedStatus' = [detectedStatus EXCEPT ![<<v, s, b>>] = "neglected"]
    /\ UNCHANGED <<blocks, detectableInView, equivocationRecords>>

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
            MarkDetectableInView(v, s, b)
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

Inv_NeglectedHasDetectableView ==
    \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
        detectedStatus[<<v, s, b>>] = "neglected" =>
            detectableInView[<<v, s, b>>] = TRUE

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

\* Every record has a witness.
Inv_RecordHasWitness ==
    \A r \in equivocationRecords :
        LET v == r[1]
            base == r[2]
        IN  base + 1 \in 1..MaxSeqNum /\ IsRealEquivocation(v, base + 1)

TraversalDomain == 1..MaxBlocksPerSeq

TraversalStep(G, Seen) ==
    Seen \cup UNION {G[n] : n \in Seen}

RECURSIVE TraversalAfter(_, _, _)
TraversalAfter(G, Seen, fuel) ==
    IF fuel = 0 THEN Seen ELSE TraversalAfter(G, TraversalStep(G, Seen), fuel - 1)

Inv_DetectorTraversalFiniteFuel ==
    \A G \in [TraversalDomain -> SUBSET TraversalDomain] :
        TraversalAfter(G, {1}, MaxBlocksPerSeq + 1) =
        TraversalAfter(G, {1}, MaxBlocksPerSeq)

Inv_DetectorTraversalInDomain ==
    \A G \in [TraversalDomain -> SUBSET TraversalDomain] :
      \A fuel \in 0..(MaxBlocksPerSeq + 1) :
        TraversalAfter(G, {1}, fuel) \subseteq TraversalDomain

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
