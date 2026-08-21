From Stdlib Require Import Lia Lists.List Sorting.Permutation.

Import ListNotations.

Section AuthorityPresentation.

Context {atom : Type}.

Definition authority_cell := list atom.
Definition authority_presentation := list authority_cell.

Definition presentation_atoms
  (presented : authority_presentation)
  : list atom :=
  concat presented.

Definition exact_cover
  (demand : list atom)
  (presented : authority_presentation)
  : Prop :=
  Permutation (presentation_atoms presented) demand.

Theorem exact_cover_preserves_cardinality :
  forall demand presented,
    exact_cover demand presented ->
    length (presentation_atoms presented) = length demand.
Proof.
  intros demand presented Hcover.
  now apply Permutation_length.
Qed.

Theorem exact_cover_has_no_missing_or_extra_atom :
  forall demand presented candidate,
    exact_cover demand presented ->
    (In candidate demand <-> In candidate (presentation_atoms presented)).
Proof.
  intros demand presented candidate Hcover.
  split; intro Hin.
  - eapply Permutation_in.
    + exact (Permutation_sym Hcover).
    + exact Hin.
  - eapply Permutation_in.
    + exact Hcover.
    + exact Hin.
Qed.

Theorem exact_cover_regrouping_conserves_authority :
  forall demand left right,
    exact_cover demand left ->
    exact_cover demand right ->
    Permutation
      (presentation_atoms left)
      (presentation_atoms right).
Proof.
  intros demand left right Hleft Hright.
  eapply Permutation_trans.
  - exact Hleft.
  - exact (Permutation_sym Hright).
Qed.

Theorem intermediate_partition_is_exact :
  forall left_middle right_middle,
    exact_cover
      (left_middle ++ right_middle)
      [left_middle; right_middle].
Proof.
  intros left_middle right_middle.
  unfold exact_cover, presentation_atoms.
  simpl.
  rewrite app_nil_r.
  apply Permutation_refl.
Qed.

Theorem compound_cell_cannot_be_weakened :
  forall demand presented,
    exact_cover demand presented ->
    length (presentation_atoms presented) < length demand ->
    False.
Proof.
  intros demand presented Hcover Hweakened.
  pose proof (exact_cover_preserves_cardinality demand presented Hcover).
  lia.
Qed.

Definition event_authority_exact
  (declared debit : list atom)
  : Prop :=
  Permutation declared debit.

Theorem event_authority_exact_forbids_omission :
  forall declared debit candidate,
    event_authority_exact declared debit ->
    In candidate declared ->
    In candidate debit.
Proof.
  intros declared debit candidate Hexact Hin.
  eapply Permutation_in.
  - exact Hexact.
  - exact Hin.
Qed.

Theorem event_authority_exact_forbids_amplification :
  forall declared debit candidate,
    event_authority_exact declared debit ->
    In candidate debit ->
    In candidate declared.
Proof.
  intros declared debit candidate Hexact Hin.
  eapply Permutation_in.
  - exact (Permutation_sym Hexact).
  - exact Hin.
Qed.

Theorem event_authority_exact_preserves_multiplicity :
  forall declared debit,
    event_authority_exact declared debit ->
    length declared = length debit.
Proof.
  intros declared debit Hexact.
  now apply Permutation_length in Hexact.
Qed.

Definition authority_stack := list authority_cell.

Fixpoint pop_stacks
  (stacks : list authority_stack)
  : option (list authority_cell * list authority_stack) :=
  match stacks with
  | [] => Some ([], [])
  | [] :: _ => None
  | (head :: tail) :: rest =>
      match pop_stacks rest with
      | None => None
      | Some (heads, tails) => Some (head :: heads, tail :: tails)
      end
  end.

Fixpoint rebuild_stacks
  (heads : list authority_cell)
  (tails : list authority_stack)
  : option (list authority_stack) :=
  match heads, tails with
  | [], [] => Some []
  | head :: rest_heads, tail :: rest_tails =>
      match rebuild_stacks rest_heads rest_tails with
      | None => None
      | Some rest => Some ((head :: tail) :: rest)
      end
  | _, _ => None
  end.

Theorem pop_stacks_rebuilds_original :
  forall stacks heads tails,
    pop_stacks stacks = Some (heads, tails) ->
    rebuild_stacks heads tails = Some stacks.
Proof.
  induction stacks as [| stack rest IH]; intros heads tails Hpop.
  - simpl in Hpop.
    inversion Hpop.
    reflexivity.
  - destruct stack as [| head tail].
    + discriminate.
    + simpl in Hpop.
      destruct (pop_stacks rest) as [[rest_heads rest_tails] |] eqn:Hrest;
        try discriminate.
      inversion Hpop; subst heads tails.
      simpl.
      rewrite (IH rest_heads rest_tails eq_refl).
      reflexivity.
Qed.

Theorem pop_stacks_preserves_stack_count :
  forall stacks heads tails,
    pop_stacks stacks = Some (heads, tails) ->
    length heads = length stacks /\
    length tails = length stacks.
Proof.
  induction stacks as [| stack rest IH]; intros heads tails Hpop.
  - simpl in Hpop.
    inversion Hpop.
    auto.
  - destruct stack as [| head tail].
    + discriminate.
    + simpl in Hpop.
      destruct (pop_stacks rest) as [[rest_heads rest_tails] |] eqn:Hrest;
        try discriminate.
      inversion Hpop; subst heads tails.
      specialize (IH rest_heads rest_tails eq_refl) as [Hheads Htails].
      simpl.
      now rewrite Hheads, Htails.
Qed.

Theorem empty_stack_rejects_whole_event :
  forall prefix suffix,
    pop_stacks (prefix ++ [] :: suffix) = None.
Proof.
  induction prefix as [| stack rest IH]; intros suffix.
  - reflexivity.
  - destruct stack as [| head tail].
    + reflexivity.
    + simpl.
      rewrite IH.
      reflexivity.
Qed.

Theorem pop_stacks_is_replay_deterministic :
  forall stacks committed replayed,
    pop_stacks stacks = Some committed ->
    pop_stacks stacks = Some replayed ->
    committed = replayed.
Proof.
  intros stacks committed replayed Hcommitted Hreplayed.
  congruence.
Qed.

Definition consume_stacks_or_reject
  (stacks : list authority_stack)
  : list authority_stack * bool :=
  match pop_stacks stacks with
  | None => (stacks, false)
  | Some (_, tails) => (tails, true)
  end.

Theorem stack_consumption_is_atomic :
  forall stacks,
    consume_stacks_or_reject stacks = (stacks, false) \/
    exists heads tails,
      pop_stacks stacks = Some (heads, tails) /\
      consume_stacks_or_reject stacks = (tails, true) /\
      rebuild_stacks heads tails = Some stacks.
Proof.
  intros stacks.
  unfold consume_stacks_or_reject.
  destruct (pop_stacks stacks) as [[heads tails] |] eqn:Hpop.
  - right.
    exists heads, tails.
    repeat split; try reflexivity.
    now apply pop_stacks_rebuilds_original.
  - now left.
Qed.

End AuthorityPresentation.
