From Stdlib Require Import Lists.List Bool.Bool.
Import ListNotations.

Inductive Observation := Present | Missing | Fault.

Definition admitted_read (visible : bool) (row : Observation) : Observation :=
  if visible then row else Missing.

Fixpoint readiness (observations : list Observation) : option bool :=
  match observations with
  | [] => Some true
  | Fault :: _ => None
  | Present :: rest => readiness rest
  | Missing :: rest =>
      match readiness rest with None => None | Some _ => Some false end
  end.

Theorem present_requires_visibility_and_row :
  forall visible row,
    admitted_read visible row = Present <-> visible = true /\ row = Present.
Proof. intros [] []; simpl; intuition discriminate. Qed.

Theorem unpublished_row_is_missing :
  forall row, admitted_read false row = Missing.
Proof. reflexivity. Qed.

Theorem published_fault_is_not_missing : admitted_read true Fault = Fault.
Proof. reflexivity. Qed.

Theorem readiness_fault_exact :
  forall observations, readiness observations = None <-> In Fault observations.
Proof.
  induction observations as [|observation rest IH]; simpl.
  - split; [discriminate | contradiction].
  - destruct observation; simpl.
    + rewrite IH. intuition discriminate.
    + destruct (readiness rest) eqn:H; simpl.
      * split; [discriminate |]. intros [C|C]; [discriminate |].
        apply IH in C. congruence.
      * split; intros; [right; apply IH; reflexivity | reflexivity].
    + tauto.
Qed.

Theorem readiness_present_exact :
  forall observations,
    readiness observations = Some true <-> Forall (fun value => value = Present) observations.
Proof.
  induction observations as [|observation rest IH]; simpl.
  - split; intros; constructor.
  - destruct observation; simpl.
    + rewrite IH. split; [constructor; auto | intros H; inversion H; auto].
    + split.
      * destruct (readiness rest); discriminate.
      * intros H; inversion H; discriminate.
    + split; [discriminate | intros H; inversion H; discriminate].
Qed.

Theorem missing_does_not_hide_later_fault :
  forall before after,
    readiness (before ++ Missing :: Fault :: after) = None.
Proof.
  intros. apply readiness_fault_exact. apply in_or_app. right. simpl. auto.
Qed.

Theorem readiness_append :
  forall left right,
    readiness (left ++ right) =
    match readiness left, readiness right with
    | Some a, Some b => Some (andb a b)
    | _, _ => None
    end.
Proof.
  induction left as [|observation rest IH]; intros right; simpl.
  - destruct (readiness right); reflexivity.
  - destruct observation; simpl; auto.
    rewrite IH. destruct (readiness rest) as [[]|];
      destruct (readiness right) as [[]|]; reflexivity.
Qed.

Theorem readiness_all_source_groups :
  forall groups,
    readiness (concat groups) = Some true <->
    Forall (fun group => readiness group = Some true) groups.
Proof.
  induction groups as [|group rest IH]; simpl.
  - split; intros; constructor.
  - rewrite readiness_append. split.
    + destruct (readiness group) as [[]|] eqn:G;
        destruct (readiness (concat rest)) as [[]|] eqn:R;
        try discriminate.
      intros _. constructor; [exact G | apply IH; reflexivity].
    + intros H. inversion H; subst.
      rewrite H2, (proj2 IH H3). reflexivity.
Qed.

Print Assumptions present_requires_visibility_and_row.
Print Assumptions unpublished_row_is_missing.
Print Assumptions published_fault_is_not_missing.
Print Assumptions readiness_fault_exact.
Print Assumptions readiness_present_exact.
Print Assumptions missing_does_not_hide_later_fault.
Print Assumptions readiness_append.
Print Assumptions readiness_all_source_groups.
