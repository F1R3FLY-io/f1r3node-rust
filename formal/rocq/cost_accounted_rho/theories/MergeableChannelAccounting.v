From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia
  Sorting.Permutation ZArith.
Import ListNotations.
Open Scope Z_scope.

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import Settlement.

Inductive merge_type : Type :=
  | IntegerAdd
  | BitmaskOr.

Definition bitmask := list nat.

Definition same_bits (a b : bitmask) : Prop :=
  forall bit, In bit a <-> In bit b.

Definition bitmask_or (a b : bitmask) : bitmask :=
  a ++ b.

Definition bit_in (bit : nat) (mask : bitmask) : bool :=
  existsb (Nat.eqb bit) mask.

Definition bitmask_diff (prev endv : bitmask) : bitmask :=
  filter (fun bit => negb (bit_in bit prev)) endv.

Definition integer_add_diff (prev endv : Z) : Z :=
  endv - prev.

Definition integer_add_merge (prev diff : Z) : Z :=
  prev + diff.

Fixpoint bitmask_fold (values : list bitmask) : bitmask :=
  match values with
  | [] => []
  | value :: rest => bitmask_or value (bitmask_fold rest)
  end.

Inductive mergeable_payload : Type :=
  | NumericPayload : Z -> mergeable_payload
  | BitmaskPayload : bitmask -> mergeable_payload
  | NonNumericPayload : mergeable_payload.

Definition mergeable_payload_matches
  (ty : merge_type)
  (payload : mergeable_payload)
  : bool :=
  match ty, payload with
  | IntegerAdd, NumericPayload _ => true
  | BitmaskOr, BitmaskPayload _ => true
  | _, _ => false
  end.

Record typed_mergeable_channel := {
  mergeable_channel_id : nat;
  mergeable_channel_type : merge_type;
  mergeable_channel_payload : mergeable_payload
}.

Record typed_mergeable_diff := {
  mergeable_diff_channel_id : nat;
  mergeable_diff_type : merge_type;
  mergeable_diff_payload : mergeable_payload
}.

Definition typed_mergeable_delta
  (previous current : typed_mergeable_channel)
  : typed_mergeable_diff :=
  {|
    mergeable_diff_channel_id := mergeable_channel_id current;
    mergeable_diff_type := mergeable_channel_type current;
    mergeable_diff_payload :=
      match mergeable_channel_type current,
            mergeable_channel_payload previous,
            mergeable_channel_payload current with
      | IntegerAdd, NumericPayload prev, NumericPayload endv =>
          NumericPayload (integer_add_diff prev endv)
      | BitmaskOr, BitmaskPayload prev, BitmaskPayload endv =>
          BitmaskPayload (bitmask_diff prev endv)
      | _, _, _ => NonNumericPayload
      end
  |}.

Record mergeable_cost_boundary := {
  mergeable_boundary_settlement : fee_settlement;
  mergeable_boundary_user_system : system
}.

Record mergeable_accounting_state := {
  mergeable_accounting_boundary : mergeable_cost_boundary;
  mergeable_accounting_channels : list typed_mergeable_channel
}.

Definition apply_mergeable_accounting
  (state : mergeable_accounting_state)
  (channels : list typed_mergeable_channel)
  : mergeable_accounting_state :=
  {|
    mergeable_accounting_boundary := mergeable_accounting_boundary state;
    mergeable_accounting_channels := channels
  |}.

Lemma bit_in_true_iff : forall bit mask,
  bit_in bit mask = true <-> In bit mask.
Proof.
  intros bit mask.
  unfold bit_in.
  rewrite existsb_exists.
  split.
  - intros [candidate [Hin Heq]].
    apply Nat.eqb_eq in Heq.
    subst. exact Hin.
  - intros Hin.
    exists bit. split.
    + exact Hin.
    + apply Nat.eqb_refl.
Qed.

Theorem same_bits_bitmask_or_comm : forall a b,
  same_bits (bitmask_or a b) (bitmask_or b a).
Proof.
  intros a b bit.
  unfold same_bits, bitmask_or.
  repeat rewrite in_app_iff.
  tauto.
Qed.

Theorem same_bits_bitmask_or_assoc : forall a b c,
  same_bits (bitmask_or (bitmask_or a b) c)
            (bitmask_or a (bitmask_or b c)).
Proof.
  intros a b c bit.
  unfold same_bits, bitmask_or.
  repeat rewrite in_app_iff.
  tauto.
Qed.

Theorem same_bits_bitmask_or_idempotent : forall a,
  same_bits (bitmask_or a a) a.
Proof.
  intros a bit.
  unfold same_bits, bitmask_or.
  rewrite in_app_iff.
  tauto.
Qed.

Theorem bitmask_diff_merge_round_trip : forall previous current,
  same_bits
    (bitmask_or previous (bitmask_diff previous current))
    (bitmask_or previous current).
Proof.
  intros previous current bit.
  unfold same_bits, bitmask_or, bitmask_diff.
  repeat rewrite in_app_iff.
  split.
  - intros [Hprev | Hdiff].
    + left. exact Hprev.
    + right. apply filter_In in Hdiff. tauto.
  - intros [Hprev | Hcurrent].
    + left. exact Hprev.
    + destruct (bit_in bit previous) eqn:Hbit.
      * left. apply bit_in_true_iff. exact Hbit.
      * right. apply filter_In. split.
        -- exact Hcurrent.
        -- rewrite Hbit. reflexivity.
Qed.

Theorem integer_add_diff_merge_round_trip : forall previous current,
  integer_add_merge previous (integer_add_diff previous current) = current.
Proof.
  intros previous current.
  unfold integer_add_merge, integer_add_diff.
  lia.
Qed.

Theorem mergeable_channel_bitmask_fold_preserves_bits :
  forall values bit,
    In bit (bitmask_fold values) <->
    exists value, In value values /\ In bit value.
Proof.
  induction values as [| value rest IH]; intros bit; simpl.
  - split.
    + contradiction.
    + intros [value [Hin _]]. contradiction.
  - unfold bitmask_or.
    rewrite in_app_iff.
    rewrite IH.
    split.
    + intros [Hin | [found [Hfound Hbit]]].
      * exists value. split.
        -- left. reflexivity.
        -- exact Hin.
      * exists found. split.
        -- right. exact Hfound.
        -- exact Hbit.
    + intros [found [[Heq | Hrest] Hbit]].
      * subst. left. exact Hbit.
      * right. exists found. split; assumption.
Qed.

Theorem mergeable_channel_bitmask_fold_permutation :
  forall values values',
    Permutation values values' ->
    same_bits (bitmask_fold values) (bitmask_fold values').
Proof.
  intros values values' Hperm bit.
  unfold same_bits.
  repeat rewrite mergeable_channel_bitmask_fold_preserves_bits.
  split.
  - intros [value [Hin Hbit]].
    exists value. split.
    + eapply (Permutation_in (l:=values) (l':=values')); eassumption.
    + exact Hbit.
  - intros [value [Hin Hbit]].
    exists value. split.
    + eapply (Permutation_in (l:=values') (l':=values)).
      * exact (Permutation_sym Hperm).
      * exact Hin.
    + exact Hbit.
Qed.

Theorem mergeable_channel_delta_preserves_type :
  forall previous current,
    mergeable_diff_type (typed_mergeable_delta previous current) =
    mergeable_channel_type current.
Proof.
  reflexivity.
Qed.

Theorem non_numeric_channel_not_mergeable_payload_match :
  forall ty,
    mergeable_payload_matches ty NonNumericPayload = false.
Proof.
  intros ty. destruct ty; reflexivity.
Qed.

Theorem mergeable_channel_accounting_preserves_fee_settlement_inputs :
  forall state channels,
    let state' := apply_mergeable_accounting state channels in
    settlement_limit
      (mergeable_boundary_settlement
        (mergeable_accounting_boundary state')) =
      settlement_limit
        (mergeable_boundary_settlement
          (mergeable_accounting_boundary state)) /\
    settlement_price
      (mergeable_boundary_settlement
        (mergeable_accounting_boundary state')) =
      settlement_price
        (mergeable_boundary_settlement
          (mergeable_accounting_boundary state)) /\
    settlement_token_cost
      (mergeable_boundary_settlement
        (mergeable_accounting_boundary state')) =
      settlement_token_cost
        (mergeable_boundary_settlement
          (mergeable_accounting_boundary state)).
Proof.
  intros state channels.
  repeat split; reflexivity.
Qed.

Theorem mergeable_channel_accounting_preserves_user_budget :
  forall state channels,
    let state' := apply_mergeable_accounting state channels in
    system_token_count
      (mergeable_boundary_user_system
        (mergeable_accounting_boundary state')) =
    system_token_count
      (mergeable_boundary_user_system
        (mergeable_accounting_boundary state)).
Proof.
  reflexivity.
Qed.
