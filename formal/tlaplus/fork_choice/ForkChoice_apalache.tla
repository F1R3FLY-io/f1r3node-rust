-------------------------- MODULE ForkChoice_apalache --------------------------
(***************************************************************************)
(* Apalache SMT (symbolic) UNBOUNDED inductive-invariant wrapper for       *)
(* ForkChoice.tla.  Defense-in-depth BEYOND the bounded TLC runs.           *)
(*                                                                          *)
(* TLC (MC_ForkChoice.cfg) exhausts a FINITE horizon at MaxId=3, MaxScore=2. *)
(* Apalache proves the SAME determinism + heaviest-subtree invariants hold   *)
(* on ALL reachable states, with UNBOUNDED integer scores (no MaxScore cap   *)
(* at all: score : Int -> Int over all of the integers), via two symbolic    *)
(* checks that carry NO finite horizon:                                     *)
(*                                                                          *)
(*   BASE: apalache-mc check --init=Init  --inv=IndInv --length=0 --cinit=CInit *)
(*         -> every Init state satisfies IndInv.                             *)
(*   STEP: apalache-mc check --init=IndInv --inv=IndInv --length=1 --cinit=CInit *)
(*         -> from ANY IndInv state, one Next step preserves IndInv.         *)
(*                                                                          *)
(* If BOTH report "No error found", IndInv is INDUCTIVE, hence holds on every *)
(* reachable state -- strictly stronger than TLC's bounded exploration.      *)
(*                                                                          *)
(* WHY THIS IS A SEPARATE COPY (not EXTENDS): Apalache needs `@type`          *)
(* annotations on CONSTANTS/VARIABLES, which the TLC-facing base module does  *)
(* not carry.  This module is a type-annotated copy; the base ForkChoice.tla  *)
(* and its MC_*.cfg are left byte-for-byte intact so TLC keeps working.       *)
(*                                                                          *)
(* DELIBERATE DIFFERENCES FROM ForkChoice.tla (each a strict GENERALISATION): *)
(*  1. No MaxScore constant.  score has type Int -> Int and ranges over ALL   *)
(*     integers, not 0..MaxScore.  Determinism/heaviest-subtree are purely    *)
(*     ORDER-theoretic (they use only `<`, `=`, `<=` on scores), so proving   *)
(*     them over ℤ subsumes every 0..MaxScore instance for every MaxScore.    *)
(*     This is how the UNBOUNDED-score claim is realised: Apalache reasons     *)
(*     over native SMT integers instead of expanding a finite range.  (A       *)
(*     symbolic range `0..MaxScore` is an Apalache known-issue for            *)
(*     construction; `Int` is both admissible AND strictly more general.)     *)
(*  2. `tips \in SUBSET Ids` (== `tips \subseteq Ids`), the powerset-          *)
(*     membership form, so Apalache's assignment finder can use TypeOK as an   *)
(*     ASSIGNMENT when IndInv is the --init predicate of the STEP check.       *)
(*  3. MaxId is a concrete tip-arena bound (CInit sets MaxId=6, 2x TLC's 3).   *)
(*     Apalache encodes `SUBSET Ids` / `Cardinality` over a FINITE universe of *)
(*     ids, so the tip-COUNT stays bounded (an Apalache set-encoding limit),   *)
(*     while the SCORES are genuinely unbounded.  6 > 3 is strictly beyond TLC.*)
(***************************************************************************)
EXTENDS Integers, FiniteSets

CONSTANT
    \* @type: Int;
    MaxId,          \* finite tip-arena bound (Apalache set encoding; see note (3))
    \* @type: Bool;
    TotalTieBreak   \* TRUE = the code (total order); FALSE = the score-only fork bug

VARIABLE
    \* @type: Set(Int);
    tips,   \* the current set of ranked tip ids
    \* @type: Int -> Int;
    score   \* score[i] = cumulative supporting weight of tip i (UNBOUNDED)

vars == <<tips, score>>

Ids == 1..MaxId

\* "a precedes b": higher score first; on a score tie, lower id first -- but the
\* secondary key exists ONLY under TotalTieBreak (else equal-score tips are
\* mutually incomparable: the fork).
Prec(a, b) ==
    \/ score[b] < score[a]
    \/ (TotalTieBreak /\ score[a] = score[b] /\ a < b)

\* The maximal set = tips that nothing precedes. Determinism <=> singleton.
Maximal == { a \in tips : \A b \in tips : ~ Prec(b, a) }

Init ==
    /\ tips = {}
    /\ score = [i \in Ids |-> 0]

\* One evaluation: an arbitrary scored tip set (the estimator's output). Scores
\* are drawn from ALL of Int -- no upper cap -- so this havoc covers every
\* possible score assignment, not merely 0..MaxScore.
Step ==
    \E T  \in SUBSET Ids :
      \E sc \in [Ids -> Int] :
        /\ tips'  = T
        /\ score' = sc

Next == Step

Spec == Init /\ [][Next]_vars

------------------------------------------------------------------------------
\* NB: `tips \in SUBSET Ids` (== `tips \subseteq Ids`) and `score \in [Ids -> Int]`
\* are written in membership/assignment form so Apalache's assignment finder can
\* use TypeOK as the STEP --init predicate.  Semantically identical intent to the
\* base's `tips \subseteq Ids` / `score \in [Ids -> 0..MaxScore]`, generalised to
\* unbounded scores.
TypeOK ==
    /\ tips \in SUBSET Ids
    /\ score \in [Ids -> Int]

\* S1 / no-fork: the chosen main tip (argmax) is UNIQUE.
Inv_Deterministic == (tips = {}) \/ (Cardinality(Maximal) = 1)

\* GHOST heaviest-subtree: the chosen tip(s) have the maximum score among all tips.
Inv_HeaviestSubtree == \A a \in Maximal : \A b \in tips : score[b] <= score[a]

\* The inductive invariant.  TypeOK closes the state under Next; the two safety
\* facts are what we prove hold on ALL reachable states.  Because Next is a
\* memoryless havoc (each step re-picks the whole state), no extra strengthening
\* conjunct is needed -- IndInv is inductive as written (BASE + STEP both clean).
IndInv ==
    /\ TypeOK
    /\ Inv_Deterministic
    /\ Inv_HeaviestSubtree

\* Symbolic constant assignment (supplied via --cinit=CInit).
CInit ==
    /\ MaxId = 6
    /\ TotalTieBreak = TRUE
==============================================================================
