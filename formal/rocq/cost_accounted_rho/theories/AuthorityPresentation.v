From Stdlib Require Import Lia Arith.PeanoNat Lists.List Sorting.Permutation.

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

Theorem exact_cover_preserves_each_atom :
  forall (eq_dec : forall x y : atom, {x = y} + {x <> y})
    demand presented candidate,
    exact_cover demand presented ->
    count_occ eq_dec (presentation_atoms presented) candidate =
    count_occ eq_dec demand candidate.
Proof.
  intros eq_dec demand presented candidate Hcover.
  now apply (proj1 (Permutation_count_occ eq_dec _ _) Hcover).
Qed.

Theorem exact_covers_compose :
  forall demands presentations,
    Forall2 exact_cover demands presentations ->
    exact_cover (concat demands) (concat presentations).
Proof.
  intros demands presentations Hcovers.
  induction Hcovers as [| demand presented demands presentations Hcover Hcovers IH].
  - apply Permutation_refl.
  - unfold exact_cover, presentation_atoms in *.
    simpl.
    rewrite concat_app.
    now apply Permutation_app.
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

Theorem pop_stacks_tails_are_ordered :
  forall stacks heads tails,
    pop_stacks stacks = Some (heads, tails) ->
    tails = map (@tl authority_cell) stacks.
Proof.
  induction stacks as [| stack rest IH]; intros heads tails Hpop.
  - inversion Hpop. reflexivity.
  - destruct stack as [| head tail]; try discriminate.
    simpl in Hpop.
    destruct (pop_stacks rest) as [[rest_heads rest_tails] |] eqn:Hrest;
      try discriminate.
    inversion Hpop; subst.
    simpl. now rewrite (IH rest_heads rest_tails eq_refl).
Qed.

Theorem pop_stacks_success_iff_nonempty :
  forall stacks,
    (exists heads tails, pop_stacks stacks = Some (heads, tails)) <->
    Forall (fun stack => 1 <= length stack) stacks.
Proof.
  induction stacks as [| stack rest IH].
  - split; intro H.
    + constructor.
    + exists [], []. reflexivity.
  - destruct stack as [| head tail].
    + split.
      * intros [heads [tails Hpop]]. discriminate.
      * intro H. inversion H. simpl in *. lia.
    + split.
      * intros [heads [tails Hpop]]. simpl in Hpop.
        destruct (pop_stacks rest) as [[rest_heads rest_tails] |] eqn:Hrest;
          try discriminate.
        constructor; [simpl; lia |].
        apply IH. eauto.
      * intro H. inversion H as [| ? ? Hhead Hrest]; subst.
        apply IH in Hrest. destruct Hrest as [heads [tails Hpop]].
        exists (head :: heads), (tail :: tails). simpl. now rewrite Hpop.
Qed.

Fixpoint pop_stack_rounds
  (rounds : nat)
  (stacks : list authority_stack)
  : option (list (list authority_cell) * list authority_stack) :=
  match rounds with
  | 0 => Some ([], stacks)
  | S remaining =>
      match pop_stacks stacks with
      | None => None
      | Some (heads, tails) =>
          match pop_stack_rounds remaining tails with
          | None => None
          | Some (history, final_stacks) =>
              Some (heads :: history, final_stacks)
          end
      end
  end.

Lemma skipn_after_tail :
  forall n (stack : authority_stack),
    skipn n (tl stack) = skipn (S n) stack.
Proof.
  intros n [| head tail]; simpl; [apply skipn_nil | reflexivity].
Qed.

Theorem stack_rounds_preserve_exact_suffix :
  forall rounds stacks history tails,
    pop_stack_rounds rounds stacks = Some (history, tails) ->
    tails = map (skipn rounds) stacks /\ length history = rounds.
Proof.
  induction rounds as [| rounds IH]; intros stacks history tails Hrun.
  - inversion Hrun; subst. split; [symmetry; apply map_id | reflexivity].
  - simpl in Hrun.
    destruct (pop_stacks stacks) as [[heads next] |] eqn:Hpop; try discriminate.
    destruct (pop_stack_rounds rounds next) as [[rest final_stacks] |] eqn:Hrest;
      try discriminate.
    inversion Hrun; subst.
    specialize (IH next rest tails Hrest) as [Htails Hlength].
    split; [| simpl; lia].
    rewrite Htails, (pop_stacks_tails_are_ordered stacks heads next Hpop), map_map.
    apply map_ext. intro stack. apply skipn_after_tail.
Qed.

Theorem stack_rounds_succeed_iff_capacity :
  forall rounds stacks,
    (exists history tails, pop_stack_rounds rounds stacks = Some (history, tails)) <->
    Forall (fun stack => rounds <= length stack) stacks.
Proof.
  induction rounds as [| rounds IH]; intro stacks.
  - split; intro H.
    + apply Forall_forall. intros. lia.
    + exists [], stacks. reflexivity.
  - split.
    + intros [history [tails Hrun]]. simpl in Hrun.
      destruct (pop_stacks stacks) as [[heads next] |] eqn:Hpop; try discriminate.
      destruct (pop_stack_rounds rounds next) as [[rest final_stacks] |] eqn:Hrest;
        try discriminate.
      assert (Hcapacity : Forall (fun stack => rounds <= length stack) next).
      { apply IH. eauto. }
      rewrite (pop_stacks_tails_are_ordered stacks heads next Hpop) in Hcapacity.
      rewrite Forall_map in Hcapacity.
      assert (Hnonempty : Forall (fun stack => 1 <= length stack) stacks).
      { apply pop_stacks_success_iff_nonempty. eauto. }
      apply Forall_forall. intros stack Hin.
      rewrite Forall_forall in Hcapacity, Hnonempty.
      specialize (Hcapacity stack Hin). specialize (Hnonempty stack Hin).
      destruct stack; simpl in *; lia.
    + intro Hcapacity.
      assert (Hnonempty : Forall (fun stack => 1 <= length stack) stacks).
      { rewrite Forall_forall in *.
        intros stack Hin. specialize (Hcapacity stack Hin). lia. }
      apply pop_stacks_success_iff_nonempty in Hnonempty.
      destruct Hnonempty as [heads [next Hpop]].
      assert (Hnext : Forall (fun stack => rounds <= length stack) next).
      { rewrite (pop_stacks_tails_are_ordered stacks heads next Hpop), Forall_map.
        rewrite Forall_forall in *.
        intros stack Hin. specialize (Hcapacity stack Hin).
        destruct stack; simpl in *; lia. }
      apply IH in Hnext. destruct Hnext as [history [tails Hrun]].
      exists (heads :: history), tails. simpl. now rewrite Hpop, Hrun.
Qed.

Theorem stack_rounds_compose :
  forall first second stacks first_history middle second_history final_stacks,
    pop_stack_rounds first stacks = Some (first_history, middle) ->
    pop_stack_rounds second middle = Some (second_history, final_stacks) ->
    pop_stack_rounds (first + second) stacks =
      Some (first_history ++ second_history, final_stacks).
Proof.
  induction first as [| first IH];
    intros second stacks first_history middle second_history final_stacks Hfirst Hsecond.
  - inversion Hfirst; subst. exact Hsecond.
  - simpl in Hfirst.
    destruct (pop_stacks stacks) as [[heads next] |] eqn:Hpop; try discriminate.
    destruct (pop_stack_rounds first next) as [[rest tails] |] eqn:Hrest;
      try discriminate.
    inversion Hfirst; subst.
    simpl. rewrite Hpop.
    now rewrite (IH second next rest middle second_history final_stacks Hrest Hsecond).
Qed.

Theorem repeated_cells_are_consumed_separately :
  forall rounds (cell : authority_cell),
    exists history,
      pop_stack_rounds rounds [repeat cell rounds] = Some (history, [[]]) /\
      length history = rounds.
Proof.
  intros rounds cell.
  assert (Hcapacity : Forall (fun stack => rounds <= length stack) [repeat cell rounds]).
  { constructor; [rewrite repeat_length; lia | constructor]. }
  apply stack_rounds_succeed_iff_capacity in Hcapacity.
  destruct Hcapacity as [history [tails Hrun]].
  pose proof (stack_rounds_preserve_exact_suffix rounds _ _ _ Hrun) as [Htails Hlength].
  simpl in Htails. rewrite skipn_all2 in Htails by (rewrite repeat_length; lia).
  subst tails. exists history. auto.
Qed.

Definition stack_inventory := nat -> option authority_stack.

Definition selected_stack_step
  (ids : list nat) (before after : stack_inventory) : Prop :=
  NoDup ids /\
  forall id,
    if in_dec Nat.eq_dec id ids then
      exists head tail,
        before id = Some (head :: tail) /\ after id = Some tail
    else after id = before id.

Inductive selected_stack_history :
  list (list nat) -> stack_inventory -> stack_inventory -> Prop :=
| SelectedHistoryNil : forall inventory,
    selected_stack_history [] inventory inventory
| SelectedHistoryCons : forall ids rest before middle after,
    selected_stack_step ids before middle ->
    selected_stack_history rest middle after ->
    selected_stack_history (ids :: rest) before after.

Definition selected_uses (id : nat) (events : list (list nat)) : nat :=
  count_occ Nat.eq_dec (concat events) id.

Lemma unique_selection_count :
  forall ids id, NoDup ids ->
    count_occ Nat.eq_dec ids id =
      if in_dec Nat.eq_dec id ids then 1 else 0.
Proof.
  intros ids id Hunique.
  destruct (in_dec Nat.eq_dec id ids) as [Hin | Hout].
  - exact (proj1 (NoDup_count_occ' Nat.eq_dec ids) Hunique id Hin).
  - now apply count_occ_not_In.
Qed.

Theorem selected_history_preserves_ordered_suffix :
  forall events before after,
    selected_stack_history events before after ->
    forall id original,
      before id = Some original ->
      after id = Some (skipn (selected_uses id events) original) /\
      selected_uses id events <= length original.
Proof.
  intros events before after Hhistory.
  induction Hhistory as [inventory | ids rest before middle after Hstep Hhistory IH];
    intros id original Hbefore.
  - split; [exact Hbefore | unfold selected_uses; simpl; lia].
  - destruct Hstep as [Hunique Hstep].
    specialize (Hstep id).
    unfold selected_uses. simpl. rewrite count_occ_app.
    rewrite (unique_selection_count ids id Hunique).
    destruct (in_dec Nat.eq_dec id ids) as [Hin | Hout].
    + destruct Hstep as [head [tail [Hhead Htail]]].
      rewrite Hbefore in Hhead. inversion Hhead; subst original.
      specialize (IH id tail Htail) as [Hsuffix Hbound].
      unfold selected_uses in *. simpl. split; [exact Hsuffix | lia].
    + rewrite Hbefore in Hstep.
      exact (IH id original Hstep).
Qed.

Theorem selected_history_cannot_create_stacks :
  forall events before after,
    selected_stack_history events before after ->
    forall id, before id = None -> after id = None.
Proof.
  intros events before after Hhistory.
  induction Hhistory as [inventory | ids rest before middle after Hstep Hhistory IH];
    intros id Hbefore.
  - exact Hbefore.
  - destruct Hstep as [Hunique Hstep]. specialize (Hstep id).
    destruct (in_dec Nat.eq_dec id ids).
    + destruct Hstep as [head [tail [Hhead Htail]]]. congruence.
    + apply IH. congruence.
Qed.

Theorem selected_history_reconstructs_each_stack :
  forall events before after,
    selected_stack_history events before after ->
    forall id original,
      before id = Some original ->
      exists consumed remaining,
        original = consumed ++ remaining /\
        length consumed = selected_uses id events /\
        after id = Some remaining.
Proof.
  intros events before after Hhistory id original Hbefore.
  pose proof (selected_history_preserves_ordered_suffix _ _ _ Hhistory id original Hbefore)
    as [Hsuffix Hbound].
  exists (firstn (selected_uses id events) original),
    (skipn (selected_uses id events) original).
  split; [symmetry; apply firstn_skipn |].
  split; [rewrite length_firstn, Nat.min_l; lia | exact Hsuffix].
Qed.

Theorem selected_history_preserves_unselected_stacks :
  forall events before after,
    selected_stack_history events before after ->
    forall id, ~ In id (concat events) -> after id = before id.
Proof.
  intros events before after Hhistory id Habsent.
  destruct (before id) as [original |] eqn:Hbefore.
  - pose proof (selected_history_preserves_ordered_suffix _ _ _ Hhistory id original Hbefore)
      as [Hsuffix Hbound].
    unfold selected_uses in Hsuffix.
    rewrite (proj1 (count_occ_not_In Nat.eq_dec _ _) Habsent) in Hsuffix.
    exact Hsuffix.
  - now apply (selected_history_cannot_create_stacks _ _ _ Hhistory).
Qed.

Theorem admitted_interleavings_preserve_stack_suffixes :
  forall first second before first_after second_after,
    selected_stack_history first before first_after ->
    selected_stack_history second before second_after ->
    Permutation (concat first) (concat second) ->
    forall id, first_after id = second_after id.
Proof.
  intros first second before first_after second_after Hfirst Hsecond Hperm id.
  destruct (before id) as [original |] eqn:Hbefore.
  - pose proof (selected_history_preserves_ordered_suffix _ _ _ Hfirst id original Hbefore)
      as [Hleft Hleft_bound].
    pose proof (selected_history_preserves_ordered_suffix _ _ _ Hsecond id original Hbefore)
      as [Hright Hright_bound].
    rewrite Hleft, Hright. unfold selected_uses.
    now rewrite (proj1 (Permutation_count_occ Nat.eq_dec _ _) Hperm id).
  - rewrite (selected_history_cannot_create_stacks _ _ _ Hfirst id Hbefore).
    now rewrite (selected_history_cannot_create_stacks _ _ _ Hsecond id Hbefore).
Qed.

Theorem selected_history_cannot_reuse_exhausted_cells :
  forall events before after id original,
    before id = Some original ->
    length original < selected_uses id events ->
    ~ selected_stack_history events before after.
Proof.
  intros events before after id original Hbefore Hover Hhistory.
  pose proof (selected_history_preserves_ordered_suffix _ _ _ Hhistory id original Hbefore).
  lia.
Qed.

End AuthorityPresentation.
