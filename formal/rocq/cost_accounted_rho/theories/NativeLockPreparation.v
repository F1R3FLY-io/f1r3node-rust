From Stdlib Require Import Lists.List Bool.Bool Arith.PeanoNat Lia.
Import ListNotations.

Definition lock_indices (stripes : nat) (keys : list nat) : list nat :=
  filter (fun index => existsb (Nat.eqb index) (map (fun key => key mod stripes) keys))
    (seq 0 stripes).

Theorem lock_indices_exact : forall stripes keys index,
  In index (lock_indices stripes keys) <->
  index < stripes /\ exists key, In key keys /\ index = key mod stripes.
Proof.
  intros. unfold lock_indices. rewrite filter_In, in_seq, existsb_exists.
  split.
  - intros [[_ bounded] [value [present equal]]]. apply Nat.eqb_eq in equal.
    apply in_map_iff in present. destruct present as [key [same present]].
    split; [exact bounded|]. exists key. split; [exact present|]. congruence.
  - intros [bounded [key [present equal]]]. split; [lia|].
    exists (key mod stripes). split.
    + apply in_map_iff. exists key. auto.
    + now apply Nat.eqb_eq.
Qed.

Theorem lock_indices_no_duplicates : forall stripes keys,
  NoDup (lock_indices stripes keys).
Proof. intros. apply NoDup_filter, seq_NoDup. Qed.

Theorem lock_indices_preserve_key_coverage : forall stripes keys key,
  0 < stripes -> In key keys -> In (key mod stripes) (lock_indices stripes keys).
Proof.
  intros. apply lock_indices_exact. split; [apply Nat.mod_upper_bound; lia|].
  exists key. auto.
Qed.

Theorem lock_indices_bounded_by_stripes : forall stripes keys,
  length (lock_indices stripes keys) <= stripes.
Proof.
  intros. unfold lock_indices. eapply Nat.le_trans; [apply filter_length_le|].
  rewrite length_seq. lia.
Qed.

Theorem lock_indices_ignore_permutation : forall stripes left right,
  (forall key, In key left <-> In key right) ->
  (forall index, In index (lock_indices stripes left) <-> In index (lock_indices stripes right)).
Proof.
  intros stripes left right same index. rewrite !lock_indices_exact.
  split; intros [bounded [key [present equal]]]; split; auto;
    exists key; split; auto; apply same; assumption.
Qed.

Fixpoint reserve_lock_work (charges : list nat) (used limit : nat) : option nat :=
  match charges with
  | [] => Some used
  | charge :: rest =>
      if used + charge <=? limit then reserve_lock_work rest (used + charge) limit else None
  end.

Theorem lock_reservation_exact : forall charges used limit final,
  reserve_lock_work charges used limit = Some final ->
  final = used + fold_right Nat.add 0 charges.
Proof.
  induction charges as [|charge rest IH]; intros used limit final checked; simpl in *.
  - inversion checked. lia.
  - destruct (used + charge <=? limit); [|discriminate].
    specialize (IH _ _ _ checked). lia.
Qed.

Theorem lock_reservation_bounded : forall charges used limit final,
  used <= limit -> reserve_lock_work charges used limit = Some final -> final <= limit.
Proof.
  induction charges as [|charge rest IH]; intros used limit final bounded checked; simpl in *.
  - inversion checked. lia.
  - destruct (used + charge <=? limit) eqn:fits; [|discriminate].
    apply Nat.leb_le in fits. eapply IH; eauto.
Qed.

Definition prepare_lock_set stripes keys charges used limit :=
  match reserve_lock_work charges used limit with
  | Some final => Some (lock_indices stripes keys, final)
  | None => None
  end.

Theorem rejected_lock_preparation_has_no_lock_set : forall stripes keys charges used limit,
  reserve_lock_work charges used limit = None ->
  prepare_lock_set stripes keys charges used limit = None.
Proof. intros. unfold prepare_lock_set. now rewrite H. Qed.

Theorem prepared_lock_set_retains_coverage_and_budget : forall stripes keys charges used limit indices final,
  used <= limit -> prepare_lock_set stripes keys charges used limit = Some (indices, final) ->
  indices = lock_indices stripes keys /\ NoDup indices /\ final <= limit /\
  final = used + fold_right Nat.add 0 charges.
Proof.
  intros. unfold prepare_lock_set in H0.
  destruct (reserve_lock_work charges used limit) as [total|] eqn:checked; [|discriminate].
  inversion H0; subst. repeat split; auto using lock_indices_no_duplicates.
  - eapply lock_reservation_bounded; eauto.
  - eapply lock_reservation_exact; eauto.
Qed.

Theorem parallel_lock_reservations_share_one_ceiling : forall first second used limit middle final,
  used <= limit -> reserve_lock_work first used limit = Some middle ->
  reserve_lock_work second middle limit = Some final ->
  used + fold_right Nat.add 0 first + fold_right Nat.add 0 second = final /\ final <= limit.
Proof.
  intros. pose proof (lock_reservation_exact _ _ _ _ H0).
  pose proof (lock_reservation_exact _ _ _ _ H1).
  pose proof (lock_reservation_bounded _ _ _ _ H H0).
  split; [lia|]. eapply lock_reservation_bounded; eauto.
Qed.

Definition reserve_read_only {Execution : Type} (execution : Execution) charges used limit :=
  match reserve_lock_work charges used limit with
  | Some final => Some (execution, final)
  | None => None
  end.

Theorem read_only_reservation_preserves_execution : forall (Execution : Type)
    (execution after : Execution) charges used limit final,
  reserve_read_only execution charges used limit = Some (after, final) -> after = execution.
Proof.
  intros. unfold reserve_read_only in H.
  destruct (reserve_lock_work charges used limit); inversion H. reflexivity.
Qed.

Theorem read_only_reservation_uses_its_own_ceiling : forall (Execution : Type)
    (execution after : Execution) charges used limit final,
  used <= limit -> reserve_read_only execution charges used limit = Some (after, final) ->
  final <= limit.
Proof.
  intros. unfold reserve_read_only in H0.
  destruct (reserve_lock_work charges used limit) eqn:checked; [|discriminate].
  inversion H0; subst. eapply lock_reservation_bounded; eauto.
Qed.
