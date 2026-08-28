(*
  ChannelNetting.v proves the two-level merge algebra used by the node.

  A channel change is a pair of natural-number multiplicities. The first
  component counts added instances of one serialized datum and the second
  counts removed instances. Distinct Rholang executions compose by addition,
  because RSpace is a finite multiset rather than a set. Repeated observation
  of the same execution is removed one level above, in a map keyed by causal
  execution identity.

  Rust correspondence:

    combine_sum            ChannelChange::additive_join
    cancel                 ChannelChange::normalized
    merge_effect_maps      causal-effect map in compute_merged_state
    effect_map_conflict    inconsistent repeated CausalEffectId error

  The historical max-union operator remains a negative model. It is both
  non-associative when cancellation is inline and semantically wrong for two
  distinct executions that emit byte-identical data.
*)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import ZArith.
From Stdlib Require Import Lia.
Import ListNotations.

Definition cc := (nat * nat)%type.
Definition mk_add : cc := (1, 0).
Definition mk_rem : cc := (0, 1).
Definition empty_cc : cc := (0, 0).

Definition cancel (c : cc) : cc :=
  (fst c - Nat.min (fst c) (snd c),
   snd c - Nat.min (fst c) (snd c)).

Definition combine_sum (x y : cc) : cc :=
  (fst x + fst y, snd x + snd y).

Theorem combine_sum_comm : forall x y, combine_sum x y = combine_sum y x.
Proof.
  intros [xa xr] [ya yr]. unfold combine_sum; simpl.
  rewrite (Nat.add_comm xa ya), (Nat.add_comm xr yr). reflexivity.
Qed.

Theorem combine_sum_assoc :
  forall x y z, combine_sum x (combine_sum y z) = combine_sum (combine_sum x y) z.
Proof.
  intros [xa xr] [ya yr] [za zr]. unfold combine_sum; simpl.
  rewrite (Nat.add_assoc xa ya za), (Nat.add_assoc xr yr zr). reflexivity.
Qed.

Theorem combine_sum_id_l : forall x, combine_sum empty_cc x = x.
Proof. intros [xa xr]. unfold combine_sum, empty_cc; simpl. reflexivity. Qed.

Definition netting_fold (l : list cc) : cc :=
  fold_right combine_sum empty_cc l.

Theorem netting_fold_perm :
  forall l1 l2, Permutation l1 l2 -> netting_fold l1 = netting_fold l2.
Proof.
  intros l1 l2 H.
  induction H as [| x l1 l2 Hp IH | x y l | l1 l2 l3 Hp1 IH1 Hp2 IH2].
  - reflexivity.
  - simpl. rewrite IH. reflexivity.
  - simpl. rewrite combine_sum_assoc, combine_sum_assoc, (combine_sum_comm y x).
    reflexivity.
  - rewrite IH1, IH2. reflexivity.
Qed.

Definition net (c : cc) : Z :=
  (Z.of_nat (fst c) - Z.of_nat (snd c))%Z.

Theorem net_combine_sum :
  forall x y, net (combine_sum x y) = (net x + net y)%Z.
Proof.
  intros [xa xr] [ya yr]. unfold net, combine_sum; simpl.
  rewrite !Nat2Z.inj_add. lia.
Qed.

Theorem net_cancel : forall c, net (cancel c) = net c.
Proof.
  intros [a r]. unfold net, cancel; simpl.
  rewrite (Nat2Z.inj_sub a (Nat.min a r)) by apply Nat.le_min_l.
  rewrite (Nat2Z.inj_sub r (Nat.min a r)) by apply Nat.le_min_r.
  lia.
Qed.

Definition combine_max (x y : cc) : cc :=
  cancel (Nat.max (fst x) (fst y), Nat.max (snd x) (snd y)).

Theorem combine_not_assoc_exhibit :
  combine_max (combine_max mk_add mk_add) mk_rem = empty_cc
  /\ combine_max mk_add (combine_max mk_add mk_rem) = mk_add
  /\ empty_cc <> mk_add.
Proof.
  split; [vm_compute; reflexivity |].
  split; [vm_compute; reflexivity |].
  unfold empty_cc, mk_add. discriminate.
Qed.

Definition max_union (x y : cc) : cc :=
  (Nat.max (fst x) (fst y), Nat.max (snd x) (snd y)).

Theorem max_union_collapses_distinct_effects :
  max_union mk_add mk_add = mk_add
  /\ combine_sum mk_add mk_add = (2, 0)
  /\ mk_add <> (2, 0).
Proof. vm_compute. repeat split; discriminate. Qed.

Definition effect_map := nat -> option cc.

Definition compatible (left right : effect_map) : Prop :=
  forall id x y, left id = Some x -> right id = Some y -> x = y.

Definition merge_effect_maps (left right : effect_map) : effect_map :=
  fun id =>
    match left id with
    | Some change => Some change
    | None => right id
    end.

Definition effect_map_conflict (left right : effect_map) : Prop :=
  exists id x y,
    left id = Some x /\ right id = Some y /\ x <> y.

Theorem effect_map_merge_comm_pointwise :
  forall left right,
    compatible left right ->
    forall id, merge_effect_maps left right id = merge_effect_maps right left id.
Proof.
  intros left right H id.
  unfold merge_effect_maps.
  destruct (left id) as [x|] eqn:HL;
  destruct (right id) as [y|] eqn:HR; try reflexivity.
  specialize (H id x y HL HR). subst. reflexivity.
Qed.

Theorem effect_map_merge_assoc_pointwise :
  forall left middle right id,
    merge_effect_maps left (merge_effect_maps middle right) id =
    merge_effect_maps (merge_effect_maps left middle) right id.
Proof.
  intros left middle right id. unfold merge_effect_maps.
  destruct (left id); destruct (middle id); reflexivity.
Qed.

Theorem effect_map_merge_idem_pointwise :
  forall effects id, merge_effect_maps effects effects id = effects id.
Proof.
  intros effects id. unfold merge_effect_maps.
  destruct (effects id); reflexivity.
Qed.

Theorem incompatible_same_identity_is_rejected :
  forall left right id x y,
    left id = Some x -> right id = Some y -> x <> y ->
    effect_map_conflict left right.
Proof.
  intros left right id x y HL HR Hxy.
  exists id, x, y. repeat split; assumption.
Qed.

Definition contribution (effects : effect_map) (id : nat) : cc :=
  match effects id with
  | Some change => change
  | None => empty_cc
  end.

Definition effect_projection (ids : list nat) (effects : effect_map) : cc :=
  netting_fold (map (contribution effects) ids).

Theorem effect_projection_perm :
  forall ids1 ids2 effects,
    Permutation ids1 ids2 ->
    effect_projection ids1 effects = effect_projection ids2 effects.
Proof.
  intros ids1 ids2 effects H.
  unfold effect_projection.
  apply netting_fold_perm.
  apply Permutation_map. exact H.
Qed.

Definition two_equal_outputs (id : nat) : option cc :=
  if Nat.ltb id 2 then Some mk_add else None.

Theorem distinct_equal_effects_preserve_multiplicity :
  effect_projection [0; 1] two_equal_outputs = (2, 0).
Proof. vm_compute. reflexivity. Qed.

Theorem same_identity_projects_once :
  effect_projection [0] two_equal_outputs = mk_add.
Proof. vm_compute. reflexivity. Qed.

Theorem dependent_effects_telescope :
  cancel (netting_fold [mk_add; mk_rem]) = empty_cc.
Proof. vm_compute. reflexivity. Qed.

Theorem whole_block_replication_double_counts :
  combine_sum mk_add mk_add = (2, 0) /\ mk_add <> (2, 0).
Proof. vm_compute. split; [reflexivity | discriminate]. Qed.

Theorem channel_netting_exact_deterministic :
  (forall x y, combine_sum x y = combine_sum y x)
  /\ (forall x y z,
        combine_sum x (combine_sum y z) = combine_sum (combine_sum x y) z)
  /\ (forall x, combine_sum empty_cc x = x)
  /\ (forall l1 l2, Permutation l1 l2 -> netting_fold l1 = netting_fold l2)
  /\ (forall ids1 ids2 effects,
        Permutation ids1 ids2 ->
        effect_projection ids1 effects = effect_projection ids2 effects)
  /\ max_union mk_add mk_add = mk_add
  /\ combine_sum mk_add mk_add = (2, 0)
  /\ effect_projection [0; 1] two_equal_outputs = (2, 0)
  /\ effect_projection [0] two_equal_outputs = mk_add
  /\ (forall c, net (cancel c) = net c).
Proof.
  split; [exact combine_sum_comm |].
  split; [exact combine_sum_assoc |].
  split; [exact combine_sum_id_l |].
  split; [exact netting_fold_perm |].
  split; [exact effect_projection_perm |].
  split; [vm_compute; reflexivity |].
  split; [vm_compute; reflexivity |].
  split; [exact distinct_equal_effects_preserve_multiplicity |].
  split; [exact same_identity_projects_once |].
  exact net_cancel.
Qed.
