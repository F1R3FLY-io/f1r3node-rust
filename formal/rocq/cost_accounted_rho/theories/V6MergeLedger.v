(** * V6MergeLedger

    DR-120 (gap G9): the number-channel ledger of the v6 merge rule
    (ledger.rs).

    The merge folds IntegerAdd diffs with checked addition at three places,
    each in an order that the merge does not choose:
    - [compute_branch_derived] folds the user parts and the system parts of
      a branch's chains, then adds the two totals (dag_merger.rs:1408-1438);
    - the availability walk folds a branch's combined parts
      (dag_merger.rs:122-163);
    - [compute_merged_state] folds the combined parts of every kept chain
      (conflict_set_merger.rs:547-579).
    The apply step then writes base + total and requires the result to lie
    in [0, i64::MAX] (rholang_merging_logic.rs:116-136).

    The ledger bounds the sum of the positive diffs and the sum of the
    negative diffs of each list. Every partial sum of a list lies between
    those two sums, in every order, so no fold can overflow (C3). The
    pre-check bounds the user parts and the system parts separately (LOW-4).
    The final check bounds base + total (HIGH-1). The negative controls show
    what each weaker check misses, including the order dependence of dev's
    prefix fold (conflict_set_merger.rs:1180-1223). *)

From Stdlib Require Import ZArith Lists.List Lia Bool.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.
Open Scope Z_scope.

(** The range of an i64. *)
Definition MAX : Z := 9223372036854775807.
Definition MIN : Z := -9223372036854775808.

Definition pos_part (x : Z) : Z := Z.max x 0.
Definition neg_part (x : Z) : Z := Z.min x 0.

Fixpoint pos (l : list Z) : Z :=
  match l with [] => 0 | x :: rest => pos_part x + pos rest end.
Fixpoint neg (l : list Z) : Z :=
  match l with [] => 0 | x :: rest => neg_part x + neg rest end.
Fixpoint total (l : list Z) : Z :=
  match l with [] => 0 | x :: rest => x + total rest end.

(** The sign split of a list fits an i64. *)
Definition fits (l : list Z) : Prop := pos l <= MAX /\ MIN <= neg l.

(** A fold with checked addition: it fails when a partial sum leaves the
    i64 range ([combine_mergeable_value], merging_logic.rs:48-62). *)
Fixpoint checked_fold (acc : Z) (l : list Z) : option Z :=
  match l with
  | [] => Some acc
  | x :: rest =>
      if (MIN <=? acc + x) && (acc + x <=? MAX) then checked_fold (acc + x) rest else None
  end.

Lemma pos_nonneg : forall l, 0 <= pos l.
Proof. induction l as [| x rest IH]; simpl; unfold pos_part; lia. Qed.

Lemma neg_nonpos : forall l, neg l <= 0.
Proof. induction l as [| x rest IH]; simpl; unfold neg_part; lia. Qed.

Lemma total_between : forall l, neg l <= total l <= pos l.
Proof. induction l as [| x rest IH]; simpl; unfold pos_part, neg_part; lia. Qed.

Lemma checked_fold_within :
  forall l acc,
    MIN <= acc + neg l -> acc + pos l <= MAX ->
    checked_fold acc l = Some (acc + total l).
Proof.
  induction l as [| x rest IH]; intros acc Hlo Hhi; simpl in *.
  - f_equal. lia.
  - pose proof (pos_nonneg rest). pose proof (neg_nonpos rest).
    unfold pos_part, neg_part in *.
    assert (Hstep : (MIN <=? acc + x) && (acc + x <=? MAX) = true).
    { apply andb_true_intro. split; apply Z.leb_le; lia. }
    rewrite Hstep.
    rewrite IH by lia.
    f_equal. lia.
Qed.

Lemma pos_perm : forall l l', Permutation l l' -> pos l = pos l'.
Proof. intros l l' H. induction H; simpl; lia. Qed.

Lemma neg_perm : forall l l', Permutation l l' -> neg l = neg l'.
Proof. intros l l' H. induction H; simpl; lia. Qed.

Lemma total_perm : forall l l', Permutation l l' -> total l = total l'.
Proof. intros l l' H. induction H; simpl; lia. Qed.

(** C3: when the sign split fits, the checked fold succeeds in every order
    and gives the total. *)
Theorem sign_split_bounds_every_partial_sum :
  forall l l', Permutation l l' -> fits l -> checked_fold 0 l' = Some (total l).
Proof.
  intros l l' Hperm [Hpos Hneg].
  rewrite (total_perm l l' Hperm).
  rewrite (pos_perm l l' Hperm) in Hpos.
  rewrite (neg_perm l l' Hperm) in Hneg.
  rewrite checked_fold_within by lia.
  reflexivity.
Qed.

(** A chain's mergeable value has a user part and a system part. Its
    combined part, which the walk and compose fold, is their sum. *)
Definition users (l : list (Z * Z)) : list Z := map fst l.
Definition systems (l : list (Z * Z)) : list Z := map snd l.
Definition combined (l : list (Z * Z)) : list Z := map (fun p => fst p + snd p) l.

(** The pre-check of [ledger::branch_valid]. *)
Definition branch_valid (l : list (Z * Z)) : Prop :=
  fits (users l) /\ fits (systems l) /\
  pos (users l) + pos (systems l) <= MAX /\
  MIN <= neg (users l) + neg (systems l).

Lemma combined_split :
  forall l,
    pos (combined l) <= pos (users l) + pos (systems l) /\
    neg (users l) + neg (systems l) <= neg (combined l) /\
    total (combined l) = total (users l) + total (systems l).
Proof.
  induction l as [| [u s] rest IH]; simpl; unfold pos_part, neg_part in *; lia.
Qed.

(** LOW-4: the pre-check makes the user fold, the system fold, the sum of
    their totals and the fold of the combined parts succeed, in every order. *)
Theorem split_bounds_cover_user_system_folds :
  forall l,
    branch_valid l ->
    (forall l', Permutation (users l) l' -> checked_fold 0 l' = Some (total (users l))) /\
    (forall l', Permutation (systems l) l' -> checked_fold 0 l' = Some (total (systems l))) /\
    MIN <= total (users l) + total (systems l) <= MAX /\
    (forall l', Permutation (combined l) l' -> checked_fold 0 l' = Some (total (combined l))).
Proof.
  intros l [Husers [Hsystems [Hpos Hneg]]].
  pose proof (total_between (users l)).
  pose proof (total_between (systems l)).
  destruct (combined_split l) as [Hcpos [Hcneg Hctotal]].
  repeat split.
  - intros l' Hperm. exact (sign_split_bounds_every_partial_sum _ l' Hperm Husers).
  - intros l' Hperm. exact (sign_split_bounds_every_partial_sum _ l' Hperm Hsystems).
  - lia.
  - lia.
  - intros l' Hperm. apply sign_split_bounds_every_partial_sum; [exact Hperm |].
    unfold fits. lia.
Qed.

(** The apply step of the number override: it writes base + diff when the
    result lies in [0, i64::MAX], and fails otherwise. *)
Definition apply_number (base diff : Z) : option Z :=
  if (0 <=? base + diff) && (base + diff <=? MAX) then Some (base + diff) else None.

(** HIGH-1: when the kept diffs' split fits and the final balance lies in
    [0, i64::MAX], compose folds the diffs and applies them without error. *)
Theorem final_valid_implies_apply_ok :
  forall base l,
    fits l -> 0 <= base + total l <= MAX ->
    exists diff, checked_fold 0 l = Some diff /\ apply_number base diff = Some (base + total l).
Proof.
  intros base l Hfits Hfinal.
  exists (total l).
  split.
  - exact (sign_split_bounds_every_partial_sum l l (Permutation_refl l) Hfits).
  - unfold apply_number.
    assert (Hok : (0 <=? base + total l) && (base + total l <=? MAX) = true).
    { apply andb_true_intro. split; apply Z.leb_le; lia. }
    rewrite Hok. reflexivity.
Qed.

(** A channel entry with its merge type: true for IntegerAdd, false for
    BitmaskOr. Compose fails on two entries of one channel with different
    types (conflict_set_merger.rs:553-559). *)
Fixpoint find_type (c : nat) (seen : list (nat * bool)) : option bool :=
  match seen with
  | [] => None
  | (c', t) :: rest => if Nat.eqb c c' then Some t else find_type c rest
  end.

Fixpoint type_fold (seen : list (nat * bool)) (l : list (nat * bool)) : option (list (nat * bool)) :=
  match l with
  | [] => Some seen
  | (c, t) :: rest =>
      match find_type c seen with
      | Some t' => if Bool.eqb t t' then type_fold seen rest else None
      | None => type_fold ((c, t) :: seen) rest
      end
  end.

(** The entries agree: one merge type per channel. *)
Definition agree (l : list (nat * bool)) : Prop :=
  forall c t t', In (c, t) l -> In (c, t') l -> t = t'.

Lemma find_type_in : forall c seen t, find_type c seen = Some t -> In (c, t) seen.
Proof.
  intros c seen. induction seen as [| [c' t'] rest IH]; intros t H; simpl in H.
  - discriminate.
  - destruct (Nat.eqb c c') eqn:Hc.
    + apply Nat.eqb_eq in Hc. subst c'. injection H as H. subst t'. left. reflexivity.
    + right. exact (IH t H).
Qed.

Lemma type_fold_agree :
  forall l seen, agree (seen ++ l) -> type_fold seen l <> None.
Proof.
  induction l as [| [c t] rest IH]; intros seen Hagree; simpl.
  - discriminate.
  - destruct (find_type c seen) as [t' |] eqn:Hfind.
    + assert (Heq : t = t').
      { apply (Hagree c t t').
        - apply in_or_app. right. left. reflexivity.
        - apply in_or_app. left. exact (find_type_in c seen t' Hfind). }
      subst t'. rewrite Bool.eqb_reflx.
      apply IH. intros c0 t0 t1 H0 H1. apply (Hagree c0 t0 t1).
      * apply in_app_or in H0 as [H0 | H0]; apply in_or_app; [left | right; right]; exact H0.
      * apply in_app_or in H1 as [H1 | H1]; apply in_or_app; [left | right; right]; exact H1.
    + apply IH. intros c0 t0 t1 H0 H1. apply (Hagree c0 t0 t1).
      * destruct H0 as [H0 | H0].
        -- apply in_or_app. right. left. exact H0.
        -- apply in_app_or in H0 as [H0 | H0]; apply in_or_app; [left | right; right]; exact H0.
      * destruct H1 as [H1 | H1].
        -- apply in_or_app. right. left. exact H1.
        -- apply in_app_or in H1 as [H1 | H1]; apply in_or_app; [left | right; right]; exact H1.
Qed.

(** LOW-4: when every channel keeps one merge type, compose's fold over the
    kept entries never fails on a type mismatch. *)
Theorem cross_branch_mergetype_agreement :
  forall l, agree l -> type_fold [] l <> None.
Proof. intros l H. apply type_fold_agree. exact H. Qed.

(** Negative control (C3): a list whose net total fits can still overflow
    its fold. A check of net totals misses it. *)
Theorem nc_branch_net_totals_allow_chain_fold_overflow :
  MIN <= total [MAX; 1; -1] <= MAX /\ checked_fold 0 [MAX; 1; -1] = None.
Proof. vm_compute. split; [split; discriminate | reflexivity]. Qed.

(** Negative control (LOW-4): the combined parts fit, but the user fold of
    [compute_branch_derived] overflows. A bound on the combined parts alone
    misses it. *)
Theorem nc_combined_bound_misses_split_fold_overflow :
  let chains := [(MAX, -MAX); (1, 0)] in
  fits (combined chains) /\ checked_fold 0 (users chains) = None.
Proof. vm_compute. split; [split; discriminate | reflexivity]. Qed.

(** Dev's prefix fold (conflict_set_merger.rs:1180-1223): branches are taken
    in order, and a branch is rejected when the running balance plus its
    diff leaves [0, i64::MAX]. *)
Fixpoint prefix_accept (balance : Z) (l : list Z) : list Z :=
  match l with
  | [] => []
  | x :: rest =>
      if (0 <=? balance + x) && (balance + x <=? MAX)
      then x :: prefix_accept (balance + x) rest
      else prefix_accept balance rest
  end.

(** Negative control: the prefix fold depends on the order. With base 1, the
    diff -3 is rejected when it comes first and accepted when it comes
    after +3, although the whole set ends at 1. The ledger's final check
    reads only the total, so it accepts both orders. *)
Theorem nc_prefix_check_order_dependent :
  prefix_accept 1 [-3; 3] = [3] /\
  prefix_accept 1 [3; -3] = [3; -3] /\
  0 <= 1 + total [-3; 3] <= MAX.
Proof. vm_compute. split; [reflexivity | split; [reflexivity | split; discriminate]]. Qed.

(** Negative control (LOW-4): two branches that give one channel two merge
    types make compose fail. *)
Theorem nc_cross_branch_mergetype_mismatch_aborts :
  type_fold [] [(0%nat, true); (0%nat, false)] = None.
Proof. reflexivity. Qed.

(** ** The repair loop of the ordered pass (S6). *)

(** A sub-list of a list whose split fits also fits: removal only moves both
    sums toward zero. *)
Theorem removal_preserves_sign_split :
  forall kept removed, fits (kept ++ removed) -> fits kept.
Proof.
  intros kept removed [Hpos Hneg].
  assert (Hsplit : forall a b, pos (a ++ b) = pos a + pos b /\ neg (a ++ b) = neg a + neg b).
  { induction a as [| x a IH]; intros b; simpl; [lia |].
    destruct (IH b) as [H1 H2]. lia. }
  destruct (Hsplit kept removed) as [Hp Hn].
  pose proof (pos_nonneg removed). pose proof (neg_nonpos removed).
  unfold fits. lia.
Qed.

(** A contributor to a failing channel: a kept branch and its diff on the
    channel (any value, including zero). *)
Definition contributor : Type := (nat * Z)%type.

Definition channel_total (l : list contributor) : Z := total (map snd l).

Inductive failure : Type := Negative | Overflow.

Definition fails (base : Z) (l : list contributor) (f : failure) : Prop :=
  match f with
  | Negative => base + channel_total l < 0
  | Overflow => MAX < base + channel_total l
  end.

(** The victim of a failure: for Negative, a contributor with a negative
    diff, or else any contributor (HIGH-1); for Overflow, a contributor with
    a positive diff. The list is in K order, so [last] is the last by K. *)
Definition victim (f : failure) (l : list contributor) : option contributor :=
  match f with
  | Negative =>
      match rev (filter (fun c => snd c <? 0) l) with
      | v :: _ => Some v
      | [] => match rev l with v :: _ => Some v | [] => None end
      end
  | Overflow =>
      match rev (filter (fun c => 0 <? snd c) l) with
      | v :: _ => Some v
      | [] => None
      end
  end.

Lemma total_nonpos_without_positive :
  forall l : list contributor, filter (fun c => 0 <? snd c) l = [] -> channel_total l <= 0.
Proof.
  unfold channel_total.
  induction l as [| [b d] rest IH]; intros H; simpl in *.
  - lia.
  - destruct (0 <? d) eqn:Hd; [discriminate |].
    apply Z.ltb_ge in Hd. specialize (IH H). lia.
Qed.

Lemma rev_head_in : forall (A : Type) (l : list A) v rest, rev l = v :: rest -> In v l.
Proof.
  intros A l v rest H. apply in_rev. rewrite H. left. reflexivity.
Qed.

Lemma rev_nil_inv : forall (A : Type) (l : list A), rev l = [] -> l = [].
Proof.
  intros A l H. rewrite <- (rev_involutive l). rewrite H. reflexivity.
Qed.

(** L4b: a failing channel with a contributor always has a victim among its
    contributors, so every repair round drops a branch and the loop ends. A
    channel without contributors never enters the ledger, so it cannot
    fail. *)
Theorem repair_drop_progresses_or_shrinks_contributors :
  forall base l f,
    base <= MAX -> l <> [] -> fails base l f ->
    exists v, victim f l = Some v /\ In v l.
Proof.
  intros base l f Hbase Hne Hfails.
  destruct f; simpl in *.
  - destruct (rev (filter (fun c => snd c <? 0) l)) as [| v rest] eqn:Hneg.
    + destruct (rev l) as [| v rest'] eqn:Hall.
      * exfalso. apply Hne. rewrite <- (rev_involutive l). rewrite Hall. reflexivity.
      * exists v. split; [reflexivity | exact (rev_head_in _ l v rest' Hall)].
    + exists v. split; [reflexivity |].
      apply (rev_head_in _ _ v rest) in Hneg. apply filter_In in Hneg as [Hin _]. exact Hin.
  - destruct (rev (filter (fun c => 0 <? snd c) l)) as [| v rest] eqn:Hpos.
    + exfalso.
      assert (Hnil : filter (fun c => 0 <? snd c) l = []) by exact (rev_nil_inv _ _ Hpos).
      pose proof (total_nonpos_without_positive l Hnil). lia.
    + exists v. split; [reflexivity |].
      apply (rev_head_in _ _ v rest) in Hpos. apply filter_In in Hpos as [Hin _]. exact Hin.
Qed.

(** A repair that drops only contributors of the failure's sign. *)
Definition sign_only_victim (f : failure) (l : list contributor) : option contributor :=
  match f with
  | Negative => match rev (filter (fun c => snd c <? 0) l) with v :: _ => Some v | [] => None end
  | Overflow => match rev (filter (fun c => 0 <? snd c) l) with v :: _ => Some v | [] => None end
  end.

(** Negative control (HIGH-1): with a negative base and only positive
    contributors, the channel fails Negative and a sign-only repair finds no
    victim, so its loop stalls. *)
Theorem nc_sign_only_drop_stalls_on_negative_base :
  let l := [(1%nat, 1); (2%nat, 2)] in
  fails (-5) l Negative /\ sign_only_victim Negative l = None /\ victim Negative l <> None.
Proof. vm_compute. split; [reflexivity | split; [reflexivity | discriminate]]. Qed.
