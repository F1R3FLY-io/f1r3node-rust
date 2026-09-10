From Stdlib Require Import Arith Bool Lia Lists.List.

Import ListNotations.

Fixpoint first_ready (lanes : list bool) : option nat :=
  match lanes with
  | [] => None
  | true :: _ => Some 0
  | false :: rest => option_map S (first_ready rest)
  end.

Theorem first_ready_selects_a_ready_lane :
  forall lanes selected,
    first_ready lanes = Some selected ->
    nth_error lanes selected = Some true.
Proof.
  induction lanes as [|ready rest IH]; intros selected Hselected; simpl in *.
  - discriminate.
  - destruct ready.
    + inversion Hselected. reflexivity.
    + destruct (first_ready rest) as [index|] eqn:Hrest;
        inversion Hselected; subst.
      simpl. now apply IH.
Qed.

Theorem first_ready_never_skips_an_earlier_ready_lane :
  forall lanes selected target,
    first_ready lanes = Some selected ->
    nth_error lanes target = Some true ->
    selected <= target.
Proof.
  induction lanes as [|ready rest IH]; intros selected target Hselected Htarget.
  - discriminate.
  - simpl in Hselected. destruct ready.
    + inversion Hselected. lia.
    + destruct target as [|target]; simpl in Htarget; try discriminate.
      destruct (first_ready rest) as [index|] eqn:Hrest;
        inversion Hselected; subst.
      specialize (IH index target eq_refl Htarget). lia.
Qed.

Theorem first_ready_none_has_no_ready_lane :
  forall lanes,
    first_ready lanes = None <-> Forall (fun ready => ready = false) lanes.
Proof.
  induction lanes as [|ready rest IH]; simpl.
  - split; auto.
  - destruct ready.
    + split; intro H; [discriminate|inversion H; discriminate].
    + destruct (first_ready rest) as [index|] eqn:Hrest; simpl.
      * split; intro H; [discriminate|].
        inversion H as [|head tail Hhead Htail]; subst.
        apply IH in Htail. discriminate.
      * split; intro H.
        -- constructor; [reflexivity|]. apply IH. reflexivity.
        -- reflexivity.
Qed.

Definition cyclic_rank (count start lane : nat) : nat :=
  if start <=? lane then lane - start else count - start + lane.

Definition next_lane (count selected : nat) : nat :=
  if S selected <? count then S selected else 0.

Theorem cyclic_rank_is_bounded :
  forall count start lane,
    start < count -> lane < count -> cyclic_rank count start lane < count.
Proof.
  intros count start lane Hstart Hlane. unfold cyclic_rank.
  destruct (start <=? lane) eqn:Horder.
  - apply Nat.leb_le in Horder. lia.
  - apply Nat.leb_gt in Horder. lia.
Qed.

Theorem next_lane_is_bounded :
  forall count selected,
    selected < count -> next_lane count selected < count.
Proof.
  intros count selected Hselected. unfold next_lane.
  destruct (S selected <? count) eqn:Hnext.
  - now apply Nat.ltb_lt in Hnext.
  - lia.
Qed.

Theorem earlier_service_reduces_target_rank :
  forall count start selected target,
    start < count -> selected < count -> target < count ->
    cyclic_rank count start selected < cyclic_rank count start target ->
    cyclic_rank count (next_lane count selected) target =
      cyclic_rank count start target - S (cyclic_rank count start selected).
Proof.
  intros count start selected target Hstart Hselected Htarget Hearlier.
  unfold cyclic_rank in *.
  destruct (start <=? selected) eqn:Hss;
    destruct (start <=? target) eqn:Hst;
    unfold next_lane;
    destruct (S selected <? count) eqn:Hnext;
    destruct (S selected <=? target) eqn:Hnew;
    simpl.
  all: repeat match goal with
       | H : (?a <=? ?b) = true |- _ => apply Nat.leb_le in H
       | H : (?a <=? ?b) = false |- _ => apply Nat.leb_gt in H
       | H : (?a <? ?b) = true |- _ => apply Nat.ltb_lt in H
       | H : (?a <? ?b) = false |- _ => apply Nat.ltb_ge in H
       end; lia.
Qed.

Theorem serving_target_resets_its_rank_to_the_last_position :
  forall count selected,
    selected < count ->
    cyclic_rank count (next_lane count selected) selected = count - 1.
Proof.
  intros count selected Hselected. unfold next_lane.
  destruct (S selected <? count) eqn:Hnext; unfold cyclic_rank.
  - apply Nat.ltb_lt in Hnext.
    destruct (S selected <=? selected) eqn:Horder.
    + apply Nat.leb_le in Horder. lia.
    + lia.
  - apply Nat.ltb_ge in Hnext. simpl. lia.
Qed.

Definition service_invariant (count debt rank : nat) : Prop :=
  debt + rank < count.

Theorem service_preserves_bounded_bypass :
  forall count debt start selected target,
    start < count -> selected < count -> target < count ->
    service_invariant count debt (cyclic_rank count start target) ->
    cyclic_rank count start selected < cyclic_rank count start target ->
    service_invariant count (S debt)
      (cyclic_rank count (next_lane count selected) target).
Proof.
  intros count debt start selected target Hstart Hselected Htarget Hinv Hearlier.
  rewrite (earlier_service_reduces_target_rank count start selected target
    Hstart Hselected Htarget Hearlier).
  unfold service_invariant in *. lia.
Qed.

Theorem target_service_preserves_bounded_bypass :
  forall count target,
    target < count ->
    service_invariant count 0 (cyclic_rank count (next_lane count target) target).
Proof.
  intros count target Htarget.
  rewrite serving_target_resets_its_rank_to_the_last_position by assumption.
  unfold service_invariant. lia.
Qed.

Fixpoint bypass_ready_target (selections : list nat) (rank : nat) : option nat :=
  match selections with
  | [] => Some rank
  | selected :: rest =>
      if selected <? rank
      then bypass_ready_target rest (rank - S selected)
      else None
  end.

Theorem continuous_readiness_bounds_all_finite_bypass_sequences :
  forall selections initial final,
    bypass_ready_target selections initial = Some final ->
    length selections + final <= initial.
Proof.
  induction selections as [|selected rest IH]; intros initial final Hrun; simpl in *.
  - inversion Hrun. lia.
  - destruct (selected <? initial) eqn:Hbefore; try discriminate.
    apply Nat.ltb_lt in Hbefore. apply IH in Hrun. lia.
Qed.

Theorem a_ready_lane_cannot_be_bypassed_for_one_full_rotation :
  forall count selections initial final,
    initial < count ->
    bypass_ready_target selections initial = Some final ->
    length selections < count.
Proof.
  intros count selections initial final Hinitial Hrun.
  apply continuous_readiness_bounds_all_finite_bypass_sequences in Hrun. lia.
Qed.

Print Assumptions first_ready_selects_a_ready_lane.
Print Assumptions first_ready_never_skips_an_earlier_ready_lane.
Print Assumptions first_ready_none_has_no_ready_lane.
Print Assumptions cyclic_rank_is_bounded.
Print Assumptions next_lane_is_bounded.
Print Assumptions earlier_service_reduces_target_rank.
Print Assumptions serving_target_resets_its_rank_to_the_last_position.
Print Assumptions service_preserves_bounded_bypass.
Print Assumptions target_service_preserves_bounded_bypass.
Print Assumptions continuous_readiness_bounds_all_finite_bypass_sequences.
Print Assumptions a_ready_lane_cannot_be_bypassed_for_one_full_rotation.
