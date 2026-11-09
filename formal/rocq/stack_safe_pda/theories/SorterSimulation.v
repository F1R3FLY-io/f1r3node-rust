From Stdlib Require Import List Arith Lia Relations Relation_Operators.
From StackSafePDA Require Import StackSafePDA.
Import ListNotations.
Set Implicit Arguments.

Section Sorter.
Context {Label Value : Type}.
Variable algebra : Label -> list Value -> Value.

Inductive work :=
| Visit : @tree Label -> work
| Combine : Label -> nat -> work.

Fixpoint visits (children : @forest Label) : list work :=
  match children with
  | FNil => []
  | FCons child rest => Visit child :: visits rest
  end.

Fixpoint erase (pending : list work) : list (@instruction Label) :=
  match pending with
  | [] => []
  | Visit subject :: rest => compile_tree subject ++ erase rest
  | Combine label arity :: rest => Reduce label arity :: erase rest
  end.

Fixpoint weight (pending : list work) : nat :=
  match pending with
  | [] => 0
  | Visit subject :: rest => 2 * tree_nodes subject + weight rest
  | Combine _ _ :: rest => S (weight rest)
  end.

Lemma erase_visits : forall children rest,
  erase (visits children ++ rest) = compile_forest children ++ erase rest.
Proof.
  induction children; intros; simpl; auto.
  rewrite IHchildren, app_assoc. reflexivity.
Qed.

Lemma weight_visits : forall children rest,
  weight (visits children ++ rest) = 2 * forest_nodes children + weight rest.
Proof.
  induction children; intros; simpl; auto. rewrite IHchildren. lia.
Qed.

Definition stack := list Value.
Definition lazy_state := (list work * stack)%type.
Definition reference_state := (list (@instruction Label) * stack)%type.

Inductive lazy_step : lazy_state -> option (@instruction Label) -> lazy_state -> Prop :=
| Expand : forall label children rest values,
    lazy_step (Visit (Node label children) :: rest, values) None
      (visits children ++ Combine label (forest_arity children) :: rest, values)
| Complete : forall label arity rest values,
    arity <= length values ->
    lazy_step (Combine label arity :: rest, values) (Some (Reduce label arity))
      (rest, algebra label (rev (firstn arity values)) :: skipn arity values).

Inductive reference_step : reference_state -> @instruction Label -> reference_state -> Prop :=
| ReduceStep : forall label arity rest values,
    arity <= length values ->
    reference_step (Reduce label arity :: rest, values) (Reduce label arity)
      (rest, algebra label (rev (firstn arity values)) :: skipn arity values).

Definition related (left : lazy_state) (right : reference_state) : Prop :=
  right = (erase (fst left), snd left).

Definition silent (left right : lazy_state) := lazy_step left None right.
Definition silent_steps := clos_refl_trans_1n lazy_state silent.

Lemma expansion_erasure : forall label children rest,
  erase (Visit (Node label children) :: rest) =
  erase (visits children ++ Combine label (forest_arity children) :: rest).
Proof.
  intros. rewrite erase_visits. simpl. rewrite <- app_assoc. reflexivity.
Qed.

Theorem lazy_step_matches_reference : forall left event next right,
  related left right -> lazy_step left event next ->
  match event with
  | None => related next right
  | Some instruction => exists next_right,
      reference_step right instruction next_right /\ related next next_right
  end.
Proof.
  intros left event next right Hrel Hstep.
  unfold related in *. subst right. destruct Hstep; simpl.
  - f_equal. apply expansion_erasure.
  - eexists. split; [constructor; assumption | reflexivity].
Qed.

Lemma expose_instruction : forall pending instruction program values,
  erase pending = instruction :: program ->
  exists rest,
    silent_steps (pending, values)
      (Combine (match instruction with Reduce label _ => label end)
               (match instruction with Reduce _ arity => arity end) :: rest, values) /\
    erase rest = program.
Proof.
  intros pending.
  remember (weight pending) as size eqn:Hsize.
  revert pending Hsize.
  induction size using lt_wf_ind.
  intros pending Hsize instruction program values Herase.
  destruct pending as [|item rest]; [discriminate|].
  destruct item as [[label children]|label arity].
  - assert (Hweight : weight (visits children ++ Combine label (forest_arity children) :: rest) < size).
    { subst size. rewrite weight_visits. simpl. lia. }
    assert (Herase' : erase (visits children ++ Combine label (forest_arity children) :: rest) = instruction :: program).
    { rewrite <- expansion_erasure. exact Herase. }
    destruct (H _ Hweight _ eq_refl instruction program values Herase') as [tail [Hsteps Htail]].
    exists tail. split; [|exact Htail].
    eapply rt1n_trans; [constructor | exact Hsteps].
  - simpl in Herase. inversion Herase; subst.
    exists rest. split; [constructor | reflexivity].
Qed.

Theorem reference_step_matches_lazy : forall left right instruction next_right,
  related left right -> reference_step right instruction next_right ->
  exists exposed next,
    silent_steps left exposed /\
    lazy_step exposed (Some instruction) next /\ related next next_right.
Proof.
  intros [pending values] right instruction next_right Hrel Hstep.
  unfold related in Hrel. simpl in Hrel. subst right.
  inversion Hstep; subst.
  destruct (expose_instruction pending values (eq_sym H))
    as [tail [Hsteps Htail]].
  exists (Combine label arity :: tail, values).
  eexists. split; [exact Hsteps|]. split.
  - constructor. assumption.
  - unfold related. simpl. now rewrite Htail.
Qed.

Lemma empty_erasure : forall pending, erase pending = [] -> pending = [].
Proof.
  intros [|item rest]; auto. destruct item as [[label children]|label arity]; simpl.
  - intro H. apply app_eq_nil in H. destruct H as [H _].
    apply app_eq_nil in H. destruct H as [_ H]. discriminate.
  - discriminate.
Qed.

Theorem related_final_values : forall pending values program,
  related (pending, values) (program, values) ->
  (pending = [] <-> program = []).
Proof.
  intros pending values program H. unfold related in H. simpl in H.
  inversion H; subst. split; [intro Hnil; subst; reflexivity|apply empty_erasure].
Qed.

Theorem every_lazy_step_decreases_weight : forall left event right,
  lazy_step left event right -> weight (fst right) < weight (fst left).
Proof.
  intros left event right H. destruct H; simpl.
  - rewrite weight_visits. simpl. lia.
  - lia.
Qed.

Definition result (state : lazy_state) :=
  @run Label Value algebra (erase (fst state)) (snd state).

Theorem lazy_step_preserves_result : forall left event right,
  lazy_step left event right -> result left = result right.
Proof.
  intros left event right H. destruct H; unfold result; simpl.
  - rewrite <- app_assoc, erase_visits. reflexivity.
  - assert (Hle : (arity <=? length values) = true) by (apply Nat.leb_le; assumption).
    now rewrite Hle.
Qed.

Theorem sorter_initial_result : forall subject,
  result ([Visit subject], []) = Some [fold_tree algebra subject].
Proof.
  intros. unfold result. simpl. rewrite app_nil_r.
  apply pda_fold_equivalent_to_recursive_fold.
Qed.

Theorem sorter_terminates : forall pending values output,
  result (pending, values) = Some output ->
  exists final,
    clos_refl_trans_1n lazy_state (fun a b => exists event, lazy_step a event b)
      (pending, values) ([], final) /\ final = output.
Proof.
  intros pending. remember (weight pending) as size eqn:Hsize.
  revert pending Hsize. induction size using lt_wf_ind.
  intros pending Hsize values output Hresult.
  destruct pending as [|item rest].
  - unfold result in Hresult. simpl in Hresult. inversion Hresult; subst.
    exists output. split; [constructor|reflexivity].
  - assert (Hnext : exists next event, lazy_step (item :: rest, values) event next).
    { destruct item as [[label children]|label arity].
      - eexists. eexists. constructor.
      - unfold result in Hresult. simpl in Hresult.
        destruct (arity <=? length values) eqn:Hle; [|discriminate].
        eexists. eexists. constructor. now apply Nat.leb_le. }
    destruct Hnext as [[next_work next_values] [event Hstep]].
    assert (Hweight : weight next_work < size).
    { subst size. exact (every_lazy_step_decreases_weight Hstep). }
    assert (Hresult' : result (next_work, next_values) = Some output).
    { rewrite <- (lazy_step_preserves_result Hstep). exact Hresult. }
    destruct (H _ Hweight _ eq_refl _ _ Hresult') as [final [Hsteps Hfinal]].
    exists final. split; [|exact Hfinal].
    eapply rt1n_trans; [exists event; exact Hstep|exact Hsteps].
Qed.

Corollary sorter_returns_recursive_result : forall subject,
  clos_refl_trans_1n lazy_state (fun a b => exists event, lazy_step a event b)
    ([Visit subject], []) ([], [fold_tree algebra subject]).
Proof.
  intro subject.
  destruct (sorter_terminates [Visit subject] [] (sorter_initial_result subject)) as [final [Hsteps Hfinal]].
  now subst final.
Qed.

End Sorter.

Print Assumptions lazy_step_matches_reference.
Print Assumptions reference_step_matches_lazy.
Print Assumptions related_final_values.
Print Assumptions every_lazy_step_decreases_weight.
Print Assumptions sorter_initial_result.
Print Assumptions sorter_terminates.
Print Assumptions sorter_returns_recursive_result.

Definition order_observer (label : nat) (children : list (list nat)) :=
  concat children ++ [label].

Definition sibling_witness : @tree nat :=
  Node 0 (FCons (Node 1 FNil) (FCons (Node 2 FNil) FNil)).

Example reversed_sibling_schedule_changes_result :
  run order_observer [Reduce 2 0; Reduce 1 0; Reduce 0 2] [] <>
  Some [fold_tree order_observer sibling_witness].
Proof. vm_compute. discriminate. Qed.

Fixpoint run_without_pop_reversal (program : list (@instruction nat))
  (values : list (list nat)) : option (list (list nat)) :=
  match program with
  | [] => Some values
  | Reduce label arity :: rest =>
      if arity <=? length values then
        run_without_pop_reversal rest
          (order_observer label (firstn arity values) :: skipn arity values)
      else None
  end.

Example unreversed_values_change_result :
  run_without_pop_reversal (compile_tree sibling_witness) [] <>
  Some [fold_tree order_observer sibling_witness].
Proof. vm_compute. discriminate. Qed.

Example missing_combine_changes_result :
  run order_observer [Reduce 1 0; Reduce 2 0] [] <>
  Some [fold_tree order_observer sibling_witness].
Proof. vm_compute. discriminate. Qed.
