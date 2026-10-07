From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool Lia.
From CostAccountedRho Require Import LexicographicMinimax.
Import ListNotations.

Record family_priority_key := {
  family_rank_sequence : list nat;
  family_tie_sequence : list nat
}.

Definition family_priority_le left right :=
  funding_lex_le (family_rank_sequence left) (family_rank_sequence right) /\
  (family_rank_sequence left = family_rank_sequence right ->
    funding_lex_le (family_tie_sequence right) (family_tie_sequence left)).

Definition family_priority_leb left right :=
  funding_lex_leb (family_rank_sequence left) (family_rank_sequence right) &&
  if list_eq_dec Nat.eq_dec (family_rank_sequence left) (family_rank_sequence right)
  then funding_lex_leb (family_tie_sequence right) (family_tie_sequence left)
  else true.

Theorem family_priority_check_exact : forall left right,
  family_priority_leb left right = true <-> family_priority_le left right.
Proof.
  intros left right. unfold family_priority_leb, family_priority_le.
  rewrite andb_true_iff, funding_lex_check_exact.
  destruct (list_eq_dec Nat.eq_dec (family_rank_sequence left) (family_rank_sequence right));
    [rewrite funding_lex_check_exact|]; tauto.
Qed.

Theorem family_priority_reflexive : forall key, family_priority_le key key.
Proof. intros. split; [|intro]; apply funding_lex_reflexive. Qed.

Theorem family_priority_transitive : forall left middle right,
  family_priority_le left middle -> family_priority_le middle right -> family_priority_le left right.
Proof.
  intros left middle right [rank_lm tie_lm] [rank_mr tie_mr]. split.
  - eapply funding_lex_transitive; eauto.
  - intro same_lr.
    assert (same_lm : family_rank_sequence left = family_rank_sequence middle).
    { apply funding_lex_antisymmetric; [exact rank_lm|now rewrite same_lr]. }
    assert (same_mr : family_rank_sequence middle = family_rank_sequence right) by congruence.
    eapply funding_lex_transitive; [apply tie_mr; exact same_mr|apply tie_lm; exact same_lm].
Qed.

Theorem family_priority_total : forall left right,
  family_priority_le left right \/ family_priority_le right left.
Proof.
  intros left right. destruct (list_eq_dec Nat.eq_dec (family_rank_sequence left) (family_rank_sequence right)) as [same|different].
  - destruct (funding_lex_total (family_tie_sequence right) (family_tie_sequence left)) as [lr|rl].
    + left. split; [rewrite same; apply funding_lex_reflexive|auto].
    + right. split; [rewrite same; apply funding_lex_reflexive|auto].
  - destruct (funding_lex_total (family_rank_sequence left) (family_rank_sequence right)) as [lr|rl].
    + left. split; [exact lr|intros same; contradiction].
    + right. split; [exact rl|intros same; congruence].
Qed.

Theorem family_priority_antisymmetric : forall left right,
  family_priority_le left right -> family_priority_le right left -> left = right.
Proof.
  intros [lr lt] [rr rt] [ranks tie_lr] [reverse tie_rl]. simpl in *.
  assert (same : lr = rr) by (apply funding_lex_antisymmetric; assumption).
  subst rr. specialize (tie_lr eq_refl). specialize (tie_rl eq_refl).
  assert (lt = rt) by (apply funding_lex_antisymmetric; assumption). now subst rt.
Qed.

Definition canonical_family_resource_key rows cursor :=
  {| family_rank_sequence := concat (map burden_rank rows);
     family_tie_sequence := concat (map (fun row => skipn cursor row ++ firstn cursor row) rows) |}.

Example later_family_rank_precedes_earlier_outcome_tie :
  family_priority_le
    (canonical_family_resource_key [[0; 1]; [1; 1]] 0)
    (canonical_family_resource_key [[1; 0]; [2; 0]] 0) /\
  ~ family_priority_le
    (canonical_family_resource_key [[1; 0]; [2; 0]] 0)
    (canonical_family_resource_key [[0; 1]; [1; 1]] 0).
Proof.
  split; [apply family_priority_check_exact; reflexivity|].
  intro wrong. apply family_priority_check_exact in wrong. discriminate wrong.
Qed.

Example canonical_family_priority_resolves_the_exposure_counterexample :
  family_priority_le
    (canonical_family_resource_key [[0; 1; 1; 0; 0]; [0; 1; 1; 1; 0]] 0)
    (canonical_family_resource_key [[0; 0; 1; 1; 0]; [1; 0; 1; 1; 0]] 0).
Proof. apply family_priority_check_exact. reflexivity. Qed.

Print Assumptions family_priority_check_exact.
Print Assumptions family_priority_transitive.
Print Assumptions family_priority_total.
Print Assumptions family_priority_antisymmetric.
Print Assumptions later_family_rank_precedes_earlier_outcome_tie.
Print Assumptions canonical_family_priority_resolves_the_exposure_counterexample.
