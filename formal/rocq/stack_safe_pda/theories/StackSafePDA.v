From Stdlib Require Import List Arith Bool Lia FunctionalExtensionality.

Import ListNotations.
Set Implicit Arguments.

Section Machine.

Context {Label Value : Type}.
Variable algebra : Label -> list Value -> Value.

Inductive tree : Type :=
| Node : Label -> forest -> tree
with forest : Type :=
| FNil : forest
| FCons : tree -> forest -> forest.

Fixpoint fold_tree (subject : tree) : Value :=
  match subject with
  | Node label children => algebra label (fold_forest children)
  end
with fold_forest (subjects : forest) : list Value :=
  match subjects with
  | FNil => []
  | FCons subject rest => fold_tree subject :: fold_forest rest
  end.

Fixpoint forest_arity (subjects : forest) : nat :=
  match subjects with
  | FNil => 0
  | FCons _ rest => S (forest_arity rest)
  end.

Inductive instruction : Type :=
| Reduce : Label -> nat -> instruction.

Fixpoint compile_tree (subject : tree) : list instruction :=
  match subject with
  | Node label children =>
      compile_forest children ++ [Reduce label (forest_arity children)]
  end
with compile_forest (subjects : forest) : list instruction :=
  match subjects with
  | FNil => []
  | FCons subject rest => compile_tree subject ++ compile_forest rest
  end.

Fixpoint run (program : list instruction) (values : list Value)
  : option (list Value) :=
  match program with
  | [] => Some values
  | Reduce label arity :: rest =>
      if arity <=? length values then
        run rest
          (algebra label (rev (firstn arity values)) :: skipn arity values)
      else None
  end.

Scheme tree_induction := Induction for tree Sort Prop
with forest_induction := Induction for forest Sort Prop.
Combined Scheme tree_forest_induction from tree_induction, forest_induction.

Lemma fold_forest_length :
  forall subjects, length (fold_forest subjects) = forest_arity subjects.
Proof.
  fix induction 1.
  destruct subjects; simpl; auto.
Qed.

Lemma firstn_length_app :
  forall (values suffix : list Value),
    firstn (length values) (values ++ suffix) = values.
Proof.
  induction values; intros suffix; simpl; auto.
  now rewrite IHvalues.
Qed.

Lemma skipn_length_app :
  forall (values suffix : list Value),
    skipn (length values) (values ++ suffix) = suffix.
Proof.
  induction values; intros suffix; simpl; auto.
Qed.

Theorem compile_run_equivalence :
  (forall subject program values,
      run (compile_tree subject ++ program) values =
      run program (fold_tree subject :: values)) /\
  (forall subjects program values,
      run (compile_forest subjects ++ program) values =
      run program (rev (fold_forest subjects) ++ values)).
Proof.
  apply tree_forest_induction.
  - intros label children children_equivalence program values.
    simpl compile_tree.
    rewrite <- app_assoc.
    rewrite children_equivalence.
    simpl run.
    set (completed := rev (fold_forest children)).
    assert (arity_length : forest_arity children = length completed).
    { unfold completed. rewrite length_rev, fold_forest_length. reflexivity. }
    rewrite arity_length.
    assert (enough : (length completed <=? length (completed ++ values)) = true).
    { apply Nat.leb_le. rewrite length_app. lia. }
    rewrite enough, firstn_length_app, skipn_length_app.
    unfold completed. rewrite rev_involutive. reflexivity.
  - intros program values. reflexivity.
  - intros subject subject_equivalence rest rest_equivalence program values.
    simpl compile_forest.
    rewrite <- app_assoc.
    rewrite subject_equivalence.
    rewrite rest_equivalence.
    simpl fold_forest.
    change
      (run program (rev (fold_forest rest) ++ fold_tree subject :: values) =
       run program ((rev (fold_forest rest) ++ [fold_tree subject]) ++ values)).
    rewrite <- app_assoc.
    reflexivity.
Qed.

Corollary pda_fold_equivalent_to_recursive_fold :
  forall subject,
    run (compile_tree subject) [] = Some [fold_tree subject].
Proof.
  intro subject.
  pose proof (proj1 compile_run_equivalence subject [] []) as equivalence.
  now rewrite app_nil_r in equivalence.
Qed.

Fixpoint tree_nodes (subject : tree) : nat :=
  match subject with
  | Node _ children => S (forest_nodes children)
  end
with forest_nodes (subjects : forest) : nat :=
  match subjects with
  | FNil => 0
  | FCons subject rest => tree_nodes subject + forest_nodes rest
  end.

Theorem compiled_program_has_one_instruction_per_node :
  (forall subject, length (compile_tree subject) = tree_nodes subject) /\
  (forall subjects, length (compile_forest subjects) = forest_nodes subjects).
Proof.
  apply tree_forest_induction; intros; simpl.
  - rewrite length_app, H. simpl. lia.
  - reflexivity.
  - rewrite length_app, H, H0. reflexivity.
Qed.

End Machine.

Section Reconstruction.

Context {Label : Type}.

Fixpoint forest_of_list (subjects : list (@tree Label))
  : @forest Label :=
  match subjects with
  | [] => FNil
  | subject :: rest => FCons subject (forest_of_list rest)
  end.

Fixpoint forest_to_list (subjects : @forest Label) : list (@tree Label) :=
  match subjects with
  | FNil => []
  | FCons subject rest => subject :: forest_to_list rest
  end.

Lemma forest_list_round_trip :
  forall subjects, forest_of_list (forest_to_list subjects) = subjects.
Proof.
  induction subjects; simpl; congruence.
Qed.

Definition rebuild (label : Label) (children : list (@tree Label))
  : @tree Label := Node label (forest_of_list children).

Theorem recursive_rebuild_is_identity :
  (forall subject, @fold_tree Label (@tree Label) rebuild subject = subject) /\
  (forall subjects,
      @fold_forest Label (@tree Label) rebuild subjects =
      forest_to_list subjects).
Proof.
  apply tree_forest_induction; intros; simpl.
  - rewrite H. unfold rebuild. rewrite forest_list_round_trip. reflexivity.
  - reflexivity.
  - rewrite H, H0. reflexivity.
Qed.

Corollary decoder_machine_equivalent_to_recursive_rebuild :
  forall subject,
    @run Label (@tree Label) rebuild
      (@compile_tree Label subject) [] =
    Some [subject].
Proof.
  intro subject.
  rewrite (@pda_fold_equivalent_to_recursive_fold
    Label (@tree Label) rebuild subject).
  rewrite (proj1 recursive_rebuild_is_identity subject).
  reflexivity.
Qed.

End Reconstruction.

Section Drop.

Context {Label : Type}.

Definition discard (_ : Label) (_ : list unit) : unit := tt.

Corollary drop_machine_reaches_one_completion :
  forall subject,
    @run Label unit discard (@compile_tree Label subject) [] = Some [tt].
Proof.
  intro subject.
  rewrite (@pda_fold_equivalent_to_recursive_fold Label unit discard subject).
  destruct subject. reflexivity.
Qed.

End Drop.
