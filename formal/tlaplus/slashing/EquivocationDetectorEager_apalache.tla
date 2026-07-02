------------------- MODULE EquivocationDetectorEager_apalache -------------------
(****************************************************************************)
(* Apalache SMT (symbolic) inductive-invariant wrapper for                  *)
(* EquivocationDetectorEager.tla.  Defense-in-depth BEYOND the bounded TLC   *)
(* run (MC_EquivocationDetectorEager.cfg).                                   *)
(*                                                                          *)
(* TLC explicitly enumerates the reachable states at 2v/2s/2b (exhaustive at *)
(* that bound, but only that bound).  Apalache proves the state-dependent    *)
(* SAFETY + liveness-as-safety invariants are INDUCTIVE via two symbolic     *)
(* checks that carry NO finite horizon:                                     *)
(*                                                                          *)
(*   BASE: apalache-mc check --init=Init  --inv=IndInv --length=0 --cinit=CInit *)
(*         -> every Init state satisfies IndInv.                             *)
(*   STEP: apalache-mc check --init=IndInv --inv=IndInv --length=1 --cinit=CInit *)
(*         -> from ANY IndInv state, one Next step preserves IndInv.         *)
(*                                                                          *)
(* If BOTH report "No error found", IndInv is INDUCTIVE, hence holds on every *)
(* reachable state (at these structural bounds) with NO horizon limit --      *)
(* complementary evidence to TLC's explicit enumeration, and it scales to     *)
(* structural bounds where TLC's state count would explode (the inductive     *)
(* check is O(1) in trajectory length).                                     *)
(*                                                                          *)
(* WHY A SEPARATE COPY (not EXTENDS): Apalache needs `@type` annotations on    *)
(* CONSTANTS/VARIABLES the TLC base module does not carry.  The base module    *)
(* and its MC_*.cfg are left byte-for-byte intact so TLC keeps working.        *)
(*                                                                          *)
(* DELIBERATE SCOPING vs EquivocationDetectorEager.tla:                       *)
(*  - Same STATE MACHINE: identical CONSTANTS/VARIABLES, Init, and the three   *)
(*    Next actions (SignAndDetect, MarkDetectableInView, DetectNeglected).     *)
(*  - IndInv = TypeOK + the FIVE state-dependent invariants that constrain the  *)
(*    reachable state: Inv_DetectionSound (T-1), Inv_TaxonomyCorrect (T-3),     *)
(*    Inv_NeglectedHasDetectableView, Inv_RecordHasWitness, Inv_LivenessAsSafety*)
(*    (T-2, the primary target).  These are exactly the invariants whose        *)
(*    inductiveness certifies the reachable-state safety of the detector.       *)
(*  - OMITTED (intentionally): the Canonical* and DetectorTraversal* invariants  *)
(*    and their helpers (BoundedChains, CanonicalIndex/Seq, TraversalAfter,      *)
(*    RECURSIVE ...).  Those are STATE-INDEPENDENT pure-math lemmas about helper *)
(*    operators (they quantify over ALL chains/graphs, never over the state      *)
(*    variables), so they are not part of a *state* inductive invariant; they    *)
(*    are validated by TLC (MC_EquivocationDetectorEager.cfg) and use RECURSIVE   *)
(*    operators Apalache does not support.  ReclassifySibling is likewise         *)
(*    omitted: the eager model does not include it in Next.                       *)
(*  - `equivocationRecords \in SUBSET (...)` is written in powerset-membership   *)
(*    form (== `\subseteq`) so Apalache's assignment finder can use TypeOK as     *)
(*    the STEP --init predicate.                                                 *)
(****************************************************************************)
EXTENDS Integers, FiniteSets

CONSTANT
    \* @type: Set(Int);
    Validators,
    \* @type: Int;
    MaxSeqNum,
    \* @type: Int;
    MaxBlocksPerSeq

VARIABLE
    \* @type: Int -> (Int -> Set(Int));
    blocks,                  \* DAG: per validator, per seq, set of distinct block IDs
    \* @type: <<Int, Int, Int>> -> Bool;
    detectableInView,        \* (v, s, b) -> Rust latest-message detectability
    \* @type: <<Int, Int, Int>> -> Str;
    detectedStatus,          \* (v, s, b) -> status
    \* @type: Set(<<Int, Int>>);
    equivocationRecords      \* set of (v, s-1) pairs

vars == <<blocks, detectableInView, detectedStatus, equivocationRecords>>

\* The (validator, seq, block) index space shared by the three per-triple functions.
Triples == Validators \X (1..MaxSeqNum) \X (1..MaxBlocksPerSeq)

Statuses == {"none", "valid", "admissible", "ignorable", "neglected"}

(****************************************************************************)
(* TypeOK                                                                    *)
(****************************************************************************)
TypeOK ==
    /\ blocks \in [Validators -> [1..MaxSeqNum -> SUBSET (1..MaxBlocksPerSeq)]]
    /\ detectableInView \in [Triples -> BOOLEAN]
    /\ detectedStatus \in [Triples -> Statuses]
    /\ equivocationRecords \in SUBSET (Validators \X (0..MaxSeqNum))

IsRealEquivocation(v, s) ==
    Cardinality(blocks[v][s]) >= 2

(****************************************************************************)
(* Init                                                                     *)
(****************************************************************************)
Init ==
    /\ blocks = [v \in Validators |-> [s \in 1..MaxSeqNum |-> {}]]
    /\ detectableInView = [t \in Triples |-> FALSE]
    /\ detectedStatus = [t \in Triples |-> "none"]
    /\ equivocationRecords = {}

(****************************************************************************)
(* SignAndDetect: atomically sign a block and (truly-eager) reclassify every *)
(* sibling at (v, s) in the same step.                                       *)
(****************************************************************************)
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
                [t \in Triples |->
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
(* Mark that a block's latest-message view detects a recorded equivocation. *)
(****************************************************************************)
MarkDetectableInView(v, s, b) ==
    /\ v \in Validators
    /\ s \in 1..MaxSeqNum
    /\ b \in blocks[v][s]
    /\ detectableInView[<<v, s, b>>] = FALSE
    /\ detectableInView' = [detectableInView EXCEPT ![<<v, s, b>>] = TRUE]
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

Spec == Init /\ [][Next]_vars

(****************************************************************************)
(* State-dependent invariants (the inductive-invariant conjuncts)           *)
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
        detectedStatus[<<v, s, b>>] \in Statuses

Inv_NeglectedHasDetectableView ==
    \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
        detectedStatus[<<v, s, b>>] = "neglected" =>
            detectableInView[<<v, s, b>>] = TRUE

\* Every record has a witness: <<v, base>> requires a real equivocation at base+1.
Inv_RecordHasWitness ==
    \A r \in equivocationRecords :
        LET v == r[1]
            base == r[2]
        IN  base + 1 \in 1..MaxSeqNum /\ IsRealEquivocation(v, base + 1)

\* T-2 (detection completeness, safety-fied): no real-equivocation block is stuck
\* at a non-detected status.
Inv_LivenessAsSafety ==
    \A v \in Validators, s \in 1..MaxSeqNum, b \in 1..MaxBlocksPerSeq :
        (b \in blocks[v][s] /\ IsRealEquivocation(v, s))
        => detectedStatus[<<v, s, b>>] \in {"admissible", "ignorable", "neglected"}

(****************************************************************************)
(* The inductive invariant.                                                 *)
(****************************************************************************)
IndInv ==
    /\ TypeOK
    /\ Inv_DetectionSound
    /\ Inv_TaxonomyCorrect
    /\ Inv_NeglectedHasDetectableView
    /\ Inv_RecordHasWitness
    /\ Inv_LivenessAsSafety

\* Symbolic constant assignment (supplied via --cinit=CInit).  These are the
\* finite set/function-arena bounds Apalache's encoding requires; there is no
\* unbounded-integer dimension in this model (every constant bounds a set or a
\* function domain).  Set to 3v/3s/3b -- STRICTLY BEYOND the gate's TLC eager cfg
\* (2v/2s/2b) on ALL THREE dimensions.  The inductive check is O(1) in trajectory
\* length, so it certifies a structural bound at which TLC's explicit enumeration
\* (3 validators) would blow up -- the horizon-free inductive proof scales where
\* explicit-state exploration does not.
CInit ==
    /\ Validators = {1, 2, 3}
    /\ MaxSeqNum = 3
    /\ MaxBlocksPerSeq = 3
================================================================================
