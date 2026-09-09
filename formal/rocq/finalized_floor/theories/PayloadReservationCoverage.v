From Stdlib Require Import Arith Bool Lia Lists.List.
Import ListNotations.

Record payload_slot := {
  payload_size : nat;
  reservation_held : bool;
  payload_owners : nat
}.

Definition slot_covered (slot : payload_slot) : Prop :=
  payload_owners slot > 0 -> reservation_held slot = true.

Definition slot_reserved_bytes (slot : payload_slot) : nat :=
  if reservation_held slot then payload_size slot else 0.

Definition slot_payload_bytes (slot : payload_slot) : nat :=
  if payload_owners slot =? 0 then 0 else payload_size slot.

Definition release_slot (slot : payload_slot) : payload_slot :=
  if payload_owners slot =? 0
  then {| payload_size := payload_size slot;
          reservation_held := false;
          payload_owners := 0 |}
  else slot.

Definition move_payload (slot : payload_slot) (owners : nat) : payload_slot :=
  if reservation_held slot
  then {| payload_size := payload_size slot;
          reservation_held := true;
          payload_owners := owners |}
  else slot.

Definition reserve_slot (slot : payload_slot) : payload_slot :=
  {| payload_size := payload_size slot;
     reservation_held := true;
     payload_owners := payload_owners slot |}.

Fixpoint replace_slot (index : nat) (change : payload_slot -> payload_slot)
  (slots : list payload_slot) : list payload_slot :=
  match index, slots with
  | _, [] => []
  | 0, first :: rest => change first :: rest
  | S index', first :: rest => first :: replace_slot index' change rest
  end.

Definition total_reserved (slots : list payload_slot) : nat :=
  fold_right (fun slot total => slot_reserved_bytes slot + total) 0 slots.

Definition total_payload (slots : list payload_slot) : nat :=
  fold_right (fun slot total => slot_payload_bytes slot + total) 0 slots.

Definition covered (slots : list payload_slot) : Prop := Forall slot_covered slots.
Definition safe (capacity : nat) (slots : list payload_slot) : Prop :=
  covered slots /\ total_reserved slots <= capacity.

Inductive ownership_action := Reserve | Move (owners : nat) | Release.

Definition local_change (action : ownership_action) : payload_slot -> payload_slot :=
  match action with
  | Reserve => reserve_slot
  | Move owners => fun slot => move_payload slot owners
  | Release => release_slot
  end.

Definition step (capacity index : nat) (action : ownership_action)
  (slots : list payload_slot) : list payload_slot :=
  let candidate := replace_slot index (local_change action) slots in
  if total_reserved candidate <=? capacity then candidate else slots.

Fixpoint run (capacity : nat) (operations : list (nat * ownership_action))
  (slots : list payload_slot) : list payload_slot :=
  match operations with
  | [] => slots
  | (index, action) :: rest => run capacity rest (step capacity index action slots)
  end.

Theorem covered_slot_bounds_logical_payload : forall slot,
  slot_covered slot -> slot_payload_bytes slot <= slot_reserved_bytes slot.
Proof.
  intros [bytes held owners] H. unfold slot_covered in H.
  unfold slot_payload_bytes, slot_reserved_bytes. simpl in *.
  destruct (owners =? 0) eqn:E.
  - lia.
  - apply Nat.eqb_neq in E. assert (held = true) by (apply H; lia).
    subst held. lia.
Qed.

Theorem release_preserves_coverage : forall slot,
  slot_covered slot -> slot_covered (release_slot slot).
Proof.
  intros [bytes held owners] H. unfold release_slot. simpl.
  destruct (owners =? 0); [unfold slot_covered; simpl; lia|exact H].
Qed.

Theorem release_cannot_remove_live_payload_reservation : forall slot,
  payload_owners slot > 0 -> release_slot slot = slot.
Proof.
  intros slot H. unfold release_slot.
  destruct (payload_owners slot =? 0) eqn:E; [apply Nat.eqb_eq in E; lia|reflexivity].
Qed.

Theorem release_after_last_owner_returns_all_reserved_bytes : forall slot,
  payload_owners slot = 0 -> slot_reserved_bytes (release_slot slot) = 0.
Proof.
  intros slot H. unfold release_slot. rewrite H. reflexivity.
Qed.

Theorem release_is_idempotent : forall slot,
  release_slot (release_slot slot) = release_slot slot.
Proof.
  intros [bytes held owners]. unfold release_slot. simpl.
  destruct (owners =? 0) eqn:E; simpl; rewrite ?E; reflexivity.
Qed.

Theorem release_never_increases_reserved_bytes : forall slot,
  slot_reserved_bytes (release_slot slot) <= slot_reserved_bytes slot.
Proof.
  intros [bytes held owners]. unfold release_slot. simpl.
  destruct (owners =? 0); unfold slot_reserved_bytes; simpl; lia.
Qed.

Theorem move_preserves_coverage : forall slot owners,
  slot_covered slot -> slot_covered (move_payload slot owners).
Proof.
  intros [bytes held old_owners] owners H. unfold move_payload. simpl.
  destruct held; [unfold slot_covered; simpl; auto|exact H].
Qed.

Theorem local_change_preserves_coverage : forall action slot,
  slot_covered slot -> slot_covered (local_change action slot).
Proof.
  intros action slot H. destruct action; simpl.
  - unfold reserve_slot, slot_covered. simpl. auto.
  - now apply move_preserves_coverage.
  - now apply release_preserves_coverage.
Qed.

Theorem replacement_preserves_all_coverage : forall slots index action,
  covered slots -> covered (replace_slot index (local_change action) slots).
Proof.
  induction slots as [|slot rest IH]; intros index action H; destruct index; simpl.
  - constructor.
  - constructor.
  - inversion H; subst. constructor; [now apply local_change_preserves_coverage|assumption].
  - inversion H; subst. constructor; [assumption|now apply IH].
Qed.

Theorem step_preserves_safe : forall capacity slots index action,
  safe capacity slots -> safe capacity (step capacity index action slots).
Proof.
  intros capacity slots index action [Hcovered Hcapacity]. unfold step.
  destruct (total_reserved (replace_slot index (local_change action) slots) <=? capacity) eqn:E.
  - split; [now apply replacement_preserves_all_coverage|now apply Nat.leb_le in E].
  - split; assumption.
Qed.

Theorem every_finite_ownership_schedule_is_safe : forall operations capacity slots,
  safe capacity slots -> safe capacity (run capacity operations slots).
Proof.
  induction operations as [|[index action] rest IH]; intros capacity slots H; simpl.
  - exact H.
  - apply IH. now apply step_preserves_safe.
Qed.

Theorem coverage_bounds_all_logical_payloads : forall slots,
  covered slots -> total_payload slots <= total_reserved slots.
Proof.
  induction slots as [|slot rest IH]; intros H; [reflexivity|].
  inversion H as [|first tail Hslot Htail]; subst.
  specialize (IH Htail).
  pose proof (covered_slot_bounds_logical_payload slot Hslot).
  unfold total_payload, total_reserved in *. simpl in *. lia.
Qed.

Theorem ownership_schedule_respects_the_logical_payload_ceiling :
  forall operations capacity slots,
  safe capacity slots -> total_payload (run capacity operations slots) <= capacity.
Proof.
  intros operations capacity slots H.
  apply (every_finite_ownership_schedule_is_safe operations) in H.
  destruct H as [Hcovered Hcapacity].
  apply coverage_bounds_all_logical_payloads in Hcovered. lia.
Qed.

Print Assumptions covered_slot_bounds_logical_payload.
Print Assumptions release_preserves_coverage.
Print Assumptions release_cannot_remove_live_payload_reservation.
Print Assumptions release_after_last_owner_returns_all_reserved_bytes.
Print Assumptions release_is_idempotent.
Print Assumptions release_never_increases_reserved_bytes.
Print Assumptions move_preserves_coverage.
Print Assumptions local_change_preserves_coverage.
Print Assumptions replacement_preserves_all_coverage.
Print Assumptions step_preserves_safe.
Print Assumptions every_finite_ownership_schedule_is_safe.
Print Assumptions coverage_bounds_all_logical_payloads.
Print Assumptions ownership_schedule_respects_the_logical_payload_ceiling.
