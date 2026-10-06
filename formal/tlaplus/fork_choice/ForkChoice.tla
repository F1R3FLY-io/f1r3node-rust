------------------------------- MODULE ForkChoice -------------------------------
(***************************************************************************)
(* Fork-choice DETERMINISM (S1 / no-fork) + heaviest-subtree selection.    *)
(*                                                                         *)
(* Models casper/src/rust/estimator.rs `rank_forkchoices` +                 *)
(* shared/src/rust/shared/list_ops.rs `sort_by_with_decreasing_order`: tips  *)
(* are ranked by SCORE descending, then block HASH ascending, and the head   *)
(* (argmax) is the chosen main tip. The single load-bearing safety property   *)
(* is DETERMINISM: every honest node must pick the SAME tip from the SAME     *)
(* scored tip set, independent of the HashSet/HashMap iteration order that    *)
(* produced it. That holds iff the tie-break is a TOTAL order.               *)
(*                                                                          *)
(* The `TotalTieBreak` constant is the knob distinguishing the real code       *)
(* from the fork it prevents:                                                *)
(*   TotalTieBreak = TRUE  -> the CODE: (score desc, hash asc) is total, so   *)
(*                            the argmax is UNIQUE -> deterministic (no fork). *)
(*   TotalTieBreak = FALSE -> the BUG: score-only, no secondary key, so       *)
(*                            equal-score tips are mutually incomparable and   *)
(*                            the choice among them leaks the iteration order  *)
(*                            (>1 maximal element) -> Inv_Deterministic fails. *)
(*                                                                          *)
(* Block ids stand in for distinct block hashes (a total order); scores are   *)
(* the cumulative validator-weight scores build_scores_map produces.         *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    MaxId,          \* bound on the number of tips (distinct hashes), e.g. 3
    MaxScore,       \* bound on a tip's score, e.g. 2
    TotalTieBreak   \* TRUE = the code (total order); FALSE = the score-only fork bug

VARIABLES
    tips,   \* the current set of ranked tip ids
    score   \* score[i] = cumulative supporting weight of tip i

vars == <<tips, score>>

Ids == 1..MaxId

\* "a precedes b" in the fork-choice ranking: higher score first; on a score tie,
\* lower hash (id) first -- but the secondary key exists ONLY under TotalTieBreak.
\* Without it, equal-score tips are mutually incomparable (the fork).
Prec(a, b) ==
    \/ score[b] < score[a]
    \/ (TotalTieBreak /\ score[a] = score[b] /\ a < b)

\* The maximal set = tips that nothing precedes = the fork-choice candidates for the
\* head. Determinism <=> this is a singleton.
Maximal == { a \in tips : \A b \in tips : ~ Prec(b, a) }

Init ==
    /\ tips = {}
    /\ score = [i \in Ids |-> 0]

\* One evaluation: an arbitrary scored tip set (the estimator's output before ranking).
Step ==
    \E T  \in SUBSET Ids :
      \E sc \in [Ids -> 0..MaxScore] :
        /\ tips'  = T
        /\ score' = sc

Next == Step

Spec == Init /\ [][Next]_vars

------------------------------------------------------------------------------
TypeOK ==
    /\ tips \subseteq Ids
    /\ score \in [Ids -> 0..MaxScore]

\* S1 / no-fork: the chosen main tip (argmax) is UNIQUE, so every node picks the same
\* tip regardless of iteration order. Holds under the total tie-break (the code);
\* under the score-only bug a score tie leaves >1 maximal element and TLC exhibits
\* the non-deterministic (forking) choice.
Inv_Deterministic == (tips = {}) \/ (Cardinality(Maximal) = 1)

\* GHOST heaviest-subtree: the chosen tip(s) have the maximum score among all tips.
Inv_HeaviestSubtree == \A a \in Maximal : \A b \in tips : score[b] <= score[a]
==============================================================================
