(** * V6MergeOrderedPass

    DR-120 (gap G9): the ordered pass, the repair loop and the fast path of
    the v6 merge rule (ordered.rs, fast.rs, compose.rs).

    The ordered pass replaces dev's rejection-option search at its single
    call site (dag_merger.rs:1818). Its steps are:
    - S5 ([walk]): walk the candidates in the order K once. Keep a candidate
      only when it conflicts with no kept candidate, mixes no folded and
      plain change with them, and keeps every sign split in range.
    - S6 ([repair]): repeat until nothing changes: lineage closure, the
      conflict re-check after a candidate shrinks, the overfill dry run, and
      the final balances. Each repair round drops a whole candidate.
    S1 to S4 (late chains, branches, the pre-check and dev's availability
    walk, and dev's conflict map) are dev's code. The model takes their
    results as inputs: the candidates, and the chains [rej0] that S1 to S3
    rejected.

    The model is generic. Chains are natural numbers. A candidate is a list
    of chains. The relations of the merge are Section variables, and each
    property that the proofs use is a Section hypothesis that a cited theorem
    or code fact supplies:
    - [conflict] is dev's conflict map ([compute_conflict_map]), a symmetric
      relation on candidates. It is not monotone: removing a chain can create
      a conflict (MEDIUM-1, [nc_partial_lineage_creates_conflict]).
    - [ledger_ok] is the sign-split and merge-type check of ledger.rs. A sub-
      collection of a valid chain set is valid
      (V6MergeLedger.removal_preserves_sign_split and
      V6MergeLedger.cross_branch_mergetype_agreement).
    - [overfill] is the dry run of compose's guard (compose.rs
      [first_overfill]). An overfilled channel has an adder (the base holds at
      most one value), and the dry run reads the candidates in canonical
      order, so a permutation of the candidates gives the same result.
    - [balance] is [PurseLedger::first_failure]. A failing channel has a
      contributor of the right sign
      (V6MergeLedger.repair_drop_progresses_or_shrinks_contributors), and the
      result depends only on the set of chains (BTreeMap sums).
    - [anc] is strict DAG ancestry of blocks, which is transitive.
    - [resort] is [sort_by_k]. It permutes its input and puts every pinned
      candidate first ([sort_by_k_perm], [sort_by_k_pinned_first] below,
      from V6MergeOrder).

    Main results (v4 section 6):
    - [survivors_pairwise_conflict_free], [lineage_recheck_restores_conflict_freedom],
      [survivors_free_of_base_and_pinned_conflicts], [survivors_no_folded_mixing],
      [overfill_check_matches_guard], [survivors_no_overfill],
      [survivor_ids_exactly_once], [repair_loop_terminates_valid],
      [pinned_kept_unless_forced] (L8), [first_unpinned_in_k_not_lost_to_later]
      (L9), [rejection_has_witness], [chain_level_p5_empties_settled_partition],
      [stamped_walk_matches_slow_path] and [fast_path_equals_ordered_pass] (L5).
    - The negative controls show what each check prevents. *)

From Stdlib Require Import ZArith Lists.List Lia Bool Arith.
From Stdlib Require Import Sorting.Permutation Sorting.Sorted.
From CostAccountedRho Require Import V6MergeOrder V6MergeLedger.
Import ListNotations.
Local Open Scope nat_scope.

(** ** Witnesses *)

(** Why the ordered pass rejected a candidate or a chain (ordered.rs
    [Witness]). The model records the kept set at the time of a rejection,
    so the theorems can state what the rejection depended on. *)
Inductive witness : Type :=
| WConflict (k : list nat)
| WMixing (kept : list (list nat))
| WLedger (kept : list (list nat))
| WStale (r : nat)
| WLineageConflict (k : list nat)
| WOverfill (ch : nat) (kept : list (list nat))
| WBalance (ch : nat) (f : failure) (kept : list (list nat)).

(** ** List lemmas *)

Definition meets (l1 l2 : list nat) : bool := existsb (fun ch => existsb (Nat.eqb ch) l2) l1.

Definition nonempty (c : list nat) : bool := match c with [] => false | _ => true end.

Definition drop (v : list nat) (l : list (list nat)) : list (list nat) :=
  remove (list_eq_dec Nat.eq_dec) v l.

(** The last element of [l] that satisfies [p]. *)
Fixpoint last_sat (p : list nat -> bool) (l : list (list nat)) : option (list nat) :=
  match l with
  | [] => None
  | a :: rest =>
      match last_sat p rest with
      | Some v => Some v
      | None => if p a then Some a else None
      end
  end.

Definition pairwise (R : list nat -> list nat -> Prop) (l : list (list nat)) : Prop :=
  forall a b, In a l -> In b l -> a <> b -> R a b.

Definition antitone (R : list nat -> list nat -> Prop) : Prop :=
  forall a b a' b', R a b -> incl a' a -> incl b' b -> R a' b'.

Definition filter_closed (U : list nat -> Prop) : Prop :=
  forall c (q : nat -> bool), U c -> U (filter q c).

Definition all_nonempty (l : list (list nat)) : Prop := forall c, In c l -> c <> [].

Lemma meets_true : forall l1 l2, meets l1 l2 = true <-> exists ch, In ch l1 /\ In ch l2.
Proof.
  intros l1 l2. unfold meets. rewrite existsb_exists. split.
  - intros [ch [H1 H2]]. rewrite existsb_exists in H2. destruct H2 as [ch' [H2 Heq]].
    apply Nat.eqb_eq in Heq. subst ch'. exists ch. split; assumption.
  - intros [ch [H1 H2]]. exists ch. split; [exact H1 |].
    rewrite existsb_exists. exists ch. split; [exact H2 | apply Nat.eqb_refl].
Qed.

Lemma meets_sym : forall l1 l2, meets l1 l2 = meets l2 l1.
Proof.
  intros l1 l2. destruct (meets l1 l2) eqn:H1; destruct (meets l2 l1) eqn:H2; try reflexivity.
  - apply meets_true in H1. destruct H1 as [ch [Ha Hb]].
    assert (H : meets l2 l1 = true) by (apply meets_true; exists ch; split; assumption).
    congruence.
  - apply meets_true in H2. destruct H2 as [ch [Ha Hb]].
    assert (H : meets l1 l2 = true) by (apply meets_true; exists ch; split; assumption).
    congruence.
Qed.

Lemma meets_incl : forall l1 l2 l1' l2',
  meets l1 l2 = false -> incl l1' l1 -> incl l2' l2 -> meets l1' l2' = false.
Proof.
  intros l1 l2 l1' l2' H H1 H2. destruct (meets l1' l2') eqn:H'; [| reflexivity].
  apply meets_true in H'. destruct H' as [ch [Ha Hb]].
  assert (Hm : meets l1 l2 = true)
    by (apply meets_true; exists ch; split; [apply H1 | apply H2]; assumption).
  congruence.
Qed.

Lemma flat_map_incl : forall (g : nat -> list nat) a a', incl a' a -> incl (flat_map g a') (flat_map g a).
Proof.
  intros g a a' H x Hx. apply in_flat_map in Hx as [y [Hy Hxy]].
  apply in_flat_map. exists y. split; [apply H; exact Hy | exact Hxy].
Qed.

Lemma existsb_false : forall (A : Type) (f : A -> bool) l,
  existsb f l = false -> forall x, In x l -> f x = false.
Proof.
  intros A f l H x Hx. destruct (f x) eqn:Hf; [| reflexivity].
  assert (He : existsb f l = true) by (apply existsb_exists; exists x; split; assumption).
  congruence.
Qed.

Lemma existsb_false_intro : forall (A : Type) (f : A -> bool) l,
  (forall x, In x l -> f x = false) -> existsb f l = false.
Proof.
  intros A f l H. destruct (existsb f l) eqn:He; [| reflexivity].
  apply existsb_exists in He as [x [Hx Hfx]]. rewrite (H x Hx) in Hfx. discriminate.
Qed.

Lemma find_none_intro : forall (A : Type) (f : A -> bool) l,
  (forall x, In x l -> f x = false) -> find f l = None.
Proof.
  intros A f l H. induction l as [| y l IH]; simpl; [reflexivity |].
  rewrite (H y (or_introl eq_refl)). apply IH. intros x Hx. apply H. right. exact Hx.
Qed.

Lemma filter_all_false : forall (A : Type) (f : A -> bool) l,
  (forall x, In x l -> f x = false) -> filter f l = [].
Proof.
  intros A f l H. induction l as [| x l IH]; simpl; [reflexivity |].
  rewrite (H x (or_introl eq_refl)). apply IH. intros y Hy. apply H. right. exact Hy.
Qed.

Lemma StronglySorted_impl : forall (A : Type) (R S : A -> A -> Prop),
  (forall a b, R a b -> S a b) -> forall l, StronglySorted R l -> StronglySorted S l.
Proof.
  intros A R S Himp l H. induction H as [| x l H IH Hx]; constructor; [exact IH |].
  eapply Forall_impl; [| exact Hx]. exact (Himp x).
Qed.

Lemma filter_id_all : forall (A : Type) (f : A -> bool) l,
  (forall x, In x l -> f x = true) -> filter f l = l.
Proof.
  intros A f l H. induction l as [| x l IH]; simpl; [reflexivity |].
  rewrite (H x (or_introl eq_refl)). f_equal. apply IH. intros y Hy. apply H. right. exact Hy.
Qed.

Lemma nodup_app_disjoint : forall (A : Type) (l1 l2 : list A),
  NoDup (l1 ++ l2) -> forall x, In x l1 -> In x l2 -> False.
Proof.
  intros A l1 l2 H x H1 H2. induction l1 as [| y l1 IH]; [contradiction |].
  simpl in H. inversion H as [| y' l' Hnotin Hnd]; subst.
  destruct H1 as [Heq | H1].
  - subst y. apply Hnotin. apply in_or_app. right. exact H2.
  - exact (IH Hnd H1).
Qed.

Lemma nodup_remove_middle : forall (A : Type) (l1 m l2 : list A),
  NoDup (l1 ++ m ++ l2) -> NoDup (l1 ++ l2).
Proof.
  intros A l1 m. induction m as [| y m IH]; intros l2 H; simpl in H; [exact H |].
  apply IH. exact (NoDup_remove_1 l1 (m ++ l2) y H).
Qed.

Lemma concat_map_filter : forall (q : nat -> bool) l,
  concat (map (filter q) l) = filter q (concat l).
Proof.
  intros q l. induction l as [| c l IH]; simpl; [reflexivity |]. rewrite filter_app, IH. reflexivity.
Qed.

Lemma concat_filter_nonempty : forall l, concat (filter nonempty l) = concat l.
Proof.
  induction l as [| c l IH]; simpl; [reflexivity |].
  destruct c as [| n c]; simpl; [exact IH | rewrite IH; reflexivity].
Qed.

Lemma perm_concat : forall (l l' : list (list nat)), Permutation l l' -> Permutation (concat l) (concat l').
Proof.
  intros l l' H. induction H; simpl.
  - reflexivity.
  - apply Permutation_app_head. exact IHPermutation.
  - rewrite !app_assoc. apply Permutation_app_tail. apply Permutation_app_comm.
  - transitivity (concat l'); assumption.
Qed.

Lemma in_drop : forall c v l, In c (drop v l) <-> In c l /\ c <> v.
Proof.
  intros c v l. unfold drop. split.
  - apply in_remove.
  - intros [Hin Hne]. apply in_in_remove; assumption.
Qed.

Lemma incl_concat_drop : forall v l, incl (concat (drop v l)) (concat l).
Proof.
  intros v l x Hx. apply in_concat in Hx as [c [Hc Hxc]]. apply in_drop in Hc as [Hc _].
  apply in_concat. exists c. split; assumption.
Qed.

Lemma concat_drop_le : forall v l, length (concat (drop v l)) <= length (concat l).
Proof.
  intros v l. unfold drop. induction l as [| a rest IH]; simpl; [lia |].
  destruct (list_eq_dec Nat.eq_dec v a); simpl; rewrite ?length_app; lia.
Qed.

Lemma length_concat_drop : forall v l, In v l -> v <> [] ->
  length (concat (drop v l)) < length (concat l).
Proof.
  intros v l. induction l as [| a rest IH]; intros Hin Hne; [contradiction |].
  unfold drop in *. simpl. destruct (list_eq_dec Nat.eq_dec v a) as [Heq | Hneq].
  - subst a. rewrite length_app. pose proof (concat_drop_le v rest) as Hle. unfold drop in Hle.
    destruct v as [| n v]; [contradiction | simpl; lia].
  - simpl. rewrite !length_app. destruct Hin as [Heq | Hin]; [congruence |].
    specialize (IH Hin Hne). lia.
Qed.

Lemma nodup_concat_drop : forall v l, NoDup (concat l) -> NoDup (concat (drop v l)).
Proof.
  intros v l. unfold drop. induction l as [| a rest IH]; intros Hnd; simpl; [constructor |].
  simpl in Hnd. destruct (list_eq_dec Nat.eq_dec v a).
  - apply IH. exact (NoDup_app_remove_l _ _ Hnd).
  - simpl. apply NoDup_app.
    + exact (NoDup_app_remove_r _ _ Hnd).
    + apply IH. exact (NoDup_app_remove_l _ _ Hnd).
    + intros x Hx Hx'. apply (nodup_app_disjoint _ _ _ Hnd x Hx).
      apply (incl_concat_drop v rest). exact Hx'.
Qed.

Lemma concat_drop_split : forall v l x, In v l -> In x (concat l) ->
  In x (concat (drop v l)) \/ In x v.
Proof.
  intros v l x Hv Hx. apply in_concat in Hx as [c [Hc Hxc]].
  destruct (list_eq_dec Nat.eq_dec c v) as [-> | Hne]; [right; exact Hxc |].
  left. apply in_concat. exists c. split; [apply in_drop; split; assumption | exact Hxc].
Qed.

Lemma nodup_concat_drop_disjoint : forall v l x, NoDup (concat l) -> In v l -> In x v ->
  ~ In x (concat (drop v l)).
Proof.
  intros v l x Hnd. unfold drop. induction l as [| a rest IH]; intros Hv Hx; [contradiction |].
  simpl in Hnd |- *. destruct (list_eq_dec Nat.eq_dec v a) as [Heq | Hneq].
  - subst a. intros Hx'.
    apply (nodup_app_disjoint _ _ _ Hnd x Hx).
    apply (incl_concat_drop v rest). exact Hx'.
  - destruct Hv as [Heq | Hv]; [congruence |].
    simpl. intros Hx'. apply in_app_or in Hx' as [Hxa | Hxr].
    + apply (nodup_app_disjoint _ _ _ Hnd x Hxa). apply in_concat. exists v. split; assumption.
    + exact (IH (NoDup_app_remove_l _ _ Hnd) Hv Hx Hxr).
Qed.

Lemma nodup_outer : forall l, NoDup (concat l) -> all_nonempty l -> NoDup l.
Proof.
  induction l as [| c l IH]; intros Hnd Hne; constructor.
  - intros Hc. destruct c as [| z c']; [exact (Hne [] (or_introl eq_refl) eq_refl) |].
    apply (nodup_app_disjoint _ (z :: c') (concat l) Hnd z); [left; reflexivity |].
    apply in_concat. exists (z :: c'). split; [exact Hc | left; reflexivity].
  - apply IH; [exact (NoDup_app_remove_l _ _ Hnd) | intros d Hd; apply Hne; right; exact Hd].
Qed.

Lemma nodup_concat_split : forall (A B : list (list nat)) c x,
  NoDup (concat (A ++ c :: B)) -> In x c -> ~ In x (concat A) /\ ~ In x (concat B).
Proof.
  intros A B c x Hnd Hx. rewrite concat_app in Hnd. simpl in Hnd. split.
  - intros HA. apply (nodup_app_disjoint _ _ _ Hnd x HA). apply in_or_app. left. exact Hx.
  - intros HB. pose proof (NoDup_app_remove_l _ _ Hnd) as Hnd'.
    exact (nodup_app_disjoint _ _ _ Hnd' x Hx HB).
Qed.

Lemma nodup_split_unique : forall (l : list (list nat)) p1 s1 p2 s2 u,
  NoDup l -> l = p1 ++ u :: s1 -> l = p2 ++ u :: s2 -> p1 = p2.
Proof.
  intros l p1. revert l. induction p1 as [| a p1 IH]; intros l s1 p2 s2 u Hnd H1 H2.
  - destruct p2 as [| b p2]; [reflexivity |]. exfalso. subst l. simpl in H2.
    injection H2 as Hub Hs1. subst b. inversion Hnd as [| u' l' Hnotin _]; subst.
    apply Hnotin. apply in_or_app. right. left. reflexivity.
  - destruct p2 as [| b p2].
    + exfalso. subst l. simpl in H2. injection H2 as Hau Hs2. subst a.
      inversion Hnd as [| u' l' Hnotin _]; subst.
      apply Hnotin. apply in_or_app. right. left. reflexivity.
    + subst l. simpl in H2. injection H2 as Hab Hrest. subst b. f_equal.
      inversion Hnd as [| a' l' _ Hnd']; subst.
      exact (IH (p1 ++ u :: s1) s1 p2 s2 u Hnd' eq_refl Hrest).
Qed.

Lemma last_sat_some : forall p l v, last_sat p l = Some v -> In v l /\ p v = true.
Proof.
  intros p l. induction l as [| a rest IH]; intros v H; simpl in H; [discriminate |].
  destruct (last_sat p rest) as [v' |] eqn:Hr.
  - injection H as <-. destruct (IH v' eq_refl) as [Hin Hp]. split; [right; exact Hin | exact Hp].
  - destruct (p a) eqn:Hpa; [| discriminate]. injection H as <-.
    split; [left; reflexivity | exact Hpa].
Qed.

Lemma last_sat_none : forall p l, last_sat p l = None -> forall c, In c l -> p c = false.
Proof.
  intros p l. induction l as [| a rest IH]; intros H c Hc; simpl in H; [contradiction |].
  destruct (last_sat p rest) eqn:Hr; [discriminate |].
  destruct Hc as [Heq | Hc].
  - subst c. destruct (p a) eqn:Hpa; [discriminate H | reflexivity].
  - exact (IH eq_refl c Hc).
Qed.

Lemma pairwise_incl : forall R l l', pairwise R l -> incl l' l -> pairwise R l'.
Proof.
  unfold pairwise. intros R l l' H Hi a b Ha Hb Hab.
  apply H; [apply Hi; exact Ha | apply Hi; exact Hb | exact Hab].
Qed.

Lemma pairwise_shrink : forall R (f : list nat -> list nat) l,
  antitone R -> (forall c, incl (f c) c) -> pairwise R l -> pairwise R (map f l).
Proof.
  unfold pairwise. intros R f l Hant Hf H a' b' Ha Hb Hab.
  apply in_map_iff in Ha as [a [Ha' Ha]]. apply in_map_iff in Hb as [b [Hb' Hb]].
  subst a' b'. apply (Hant a b); [| apply Hf | apply Hf].
  apply H; [exact Ha | exact Hb |]. intros Heq. apply Hab. rewrite Heq. reflexivity.
Qed.

(** ** The pass *)

Section Pass.

Variable pinned : nat -> bool.
Variable conflict : list nat -> list nat -> bool.
Variable folded plain : nat -> list nat.
Variable ledger_ok : list nat -> bool.
Variable overfill : list (list nat) -> option nat.
Variable adds : nat -> nat -> bool.
Variable balance : list nat -> option (nat * failure).
Variable contrib : nat -> nat -> option Z.
Variable src : nat -> nat.
Variable anc : nat -> nat -> bool.
Variable base_conflicting settled_conflicting : list nat.
Variable resort : list (list nat) -> list (list nat).

(** A candidate is pinned when it holds a settled chain (order.rs). *)
Definition pinned_cand (c : list nat) : bool := existsb pinned c.

(** [pfirst a b]: K allows [a] before [b], as far as the pinned flag decides. *)
Definition pfirst (a b : list nat) : Prop := pinned_cand a = true \/ pinned_cand b = false.

Hypothesis conflict_sym : forall a b, conflict a b = conflict b a.
Hypothesis ledger_ok_nil : ledger_ok [] = true.
Hypothesis ledger_ok_incl :
  forall l l', ledger_ok l = true -> incl l' l -> NoDup l' -> ledger_ok l' = true.
Hypothesis overfill_has_adder :
  forall K ch, overfill K = Some ch -> exists c x, In c K /\ In x c /\ adds x ch = true.
Hypothesis overfill_perm : forall K K', Permutation K K' -> overfill K = overfill K'.
Hypothesis balance_has_contributor :
  forall l ch f, balance l = Some (ch, f) ->
    exists x d, In x l /\ contrib x ch = Some d /\ (f = Overflow -> (0 < d)%Z).
Hypothesis balance_perm : forall l l', Permutation l l' -> balance l = balance l'.
Hypothesis anc_trans : forall a b c, anc a b = true -> anc b c = true -> anc a c = true.
Hypothesis resort_perm : forall l, Permutation (resort l) l.
Hypothesis resort_pinned_first : forall l, StronglySorted pfirst (resort l).

(** C1: the claims of ledger.rs. A candidate mixes when it has a plain
    change on a channel where a kept candidate has a folded change, or the
    reverse (claims.rs [FoldedClaims::mixes]). *)
Definition cross_mix (a b : list nat) : bool :=
  meets (flat_map folded a) (flat_map plain b) || meets (flat_map plain a) (flat_map folded b).
Definition mixes (kept : list (list nat)) (c : list nat) : bool := existsb (cross_mix c) kept.
Definition try_add (kept : list (list nat)) (c : list nat) : bool := ledger_ok (concat kept ++ c).

(** S5: one walk in K order with the three monotone checks. *)
Fixpoint walk (cands kept : list (list nat)) (log : list (list nat * witness))
  : list (list nat) * list (list nat * witness) :=
  match cands with
  | [] => (kept, log)
  | c :: rest =>
      match find (conflict c) kept with
      | Some k => walk rest kept (log ++ [(c, WConflict k)])
      | None =>
          if mixes kept c then walk rest kept (log ++ [(c, WMixing kept)])
          else if try_add kept c then walk rest (kept ++ [c]) log
          else walk rest kept (log ++ [(c, WLedger kept)])
      end
  end.

(** S6.1: dev's stale-diff lineage rule (dag_merger.rs:1855-1898), per
    chain. A kept chain whose source block strictly descends from the source
    block of a rejected chain, of a base-conflicting chain or of a settled-
    conflicting chain is rejected, unless it is settled. *)
Definition rejected_blocks (rej : list nat) : list nat :=
  map src (rej ++ base_conflicting ++ settled_conflicting).
Definition stale (rb : list nat) (x : nat) : bool :=
  negb (pinned x) && existsb (fun r => anc r (src x)) rb.
Definition stale_witness (rb : list nat) (x : nat) : nat :=
  match find (fun r => anc r (src x)) rb with Some r => r | None => 0 end.
Definition keep_fresh (rb : list nat) (c : list nat) : list nat :=
  filter (fun x => negb (stale rb x)) c.
Definition fresh_kept (kept : list (list nat)) (rej : list nat) : list (list nat) :=
  filter nonempty (map (keep_fresh (rejected_blocks rej)) kept).
Definition stale_chains (kept : list (list nat)) (rej : list nat) : list nat :=
  filter (stale (rejected_blocks rej)) (concat kept).
Definition stale_log (kept : list (list nat)) (rej : list nat) : list (list nat * witness) :=
  map (fun x => ([x], WStale (stale_witness (rejected_blocks rej) x))) (stale_chains kept rej).

(** S6.2: the first conflicting pair in K order; the later endpoint loses. *)
Fixpoint first_conflict (l : list (list nat)) : option (list nat * list nat) :=
  match l with
  | [] => None
  | a :: rest =>
      match find (conflict a) rest with
      | Some b => Some (a, b)
      | None => first_conflict rest
      end
  end.

(** [last_by_k] of order.rs: the last candidate by K that satisfies [p],
    preferring unpinned candidates. *)
Definition last_by_k (p : list nat -> bool) (l : list (list nat)) : option (list nat) :=
  match last_sat (fun c => negb (pinned_cand c) && p c) l with
  | Some v => Some v
  | None => last_sat (fun c => pinned_cand c && p c) l
  end.

Definition adds_on (ch : nat) (c : list nat) : bool := existsb (fun x => adds x ch) c.
Definition contributes (ch : nat) (sign : Z -> bool) (c : list nat) : bool :=
  existsb (fun x => match contrib x ch with Some d => sign d | None => false end) c.
Definition is_negative (d : Z) : bool := (d <? 0)%Z.
Definition is_positive (d : Z) : bool := (0 <? d)%Z.
Definition any_sign (d : Z) : bool := true.

(** S6.4 with HIGH-1: a negative failure drops the last negative
    contributor, or else the last contributor of any sign; an overflow drops
    the last positive contributor. *)
Definition balance_victim (ch : nat) (f : failure) (l : list (list nat)) : option (list nat) :=
  match f with
  | Negative =>
      match last_by_k (contributes ch is_negative) l with
      | Some v => Some v
      | None => last_by_k (contributes ch any_sign) l
      end
  | Overflow => last_by_k (contributes ch is_positive) l
  end.

(** S6: the repair loop. [fuel] bounds the recursion; [repair_spec] shows
    that one more than the number of kept chains always suffices. *)
Fixpoint repair (fuel : nat) (recheck : bool) (kept : list (list nat)) (rej : list nat)
  (log : list (list nat * witness))
  : option (list (list nat) * list nat * list (list nat * witness)) :=
  match fuel with
  | O => None
  | S fuel' =>
      let sc := stale_chains kept rej in
      let kept2 := resort (fresh_kept kept rej) in
      let rej1 := rej ++ sc in
      let log1 := log ++ stale_log kept rej in
      match (if recheck || nonempty sc then first_conflict kept2 else None) with
      | Some (earlier, v) =>
          repair fuel' true (drop v kept2) (rej1 ++ v) (log1 ++ [(v, WLineageConflict earlier)])
      | None =>
          match overfill kept2 with
          | Some ch =>
              match last_by_k (adds_on ch) kept2 with
              | Some v =>
                  repair fuel' false (drop v kept2) (rej1 ++ v) (log1 ++ [(v, WOverfill ch kept2)])
              | None => None
              end
          | None =>
              match balance (concat kept2) with
              | None => Some (kept2, rej1, log1)
              | Some (ch, f) =>
                  match balance_victim ch f kept2 with
                  | Some v =>
                      repair fuel' false (drop v kept2) (rej1 ++ v)
                        (log1 ++ [(v, WBalance ch f kept2)])
                  | None => None
                  end
              end
          end
      end
  end.

(** The ordered pass: S5 over the candidates sorted by K, then S6. *)
Definition ordered_pass (fuel : nat) (cands : list (list nat)) (rej0 : list nat) :=
  match walk (resort cands) [] [] with
  | (kept, log) => repair fuel false kept (rej0 ++ concat (map fst log)) log
  end.

(** Variants for the negative controls. *)
Fixpoint walk_unclaimed (cands kept : list (list nat)) : list (list nat) :=
  match cands with
  | [] => kept
  | c :: rest =>
      if mixes kept c then walk_unclaimed rest kept
      else if try_add kept c then walk_unclaimed rest (kept ++ [c])
      else walk_unclaimed rest kept
  end.

Fixpoint repair_norecheck (fuel : nat) (kept : list (list nat)) (rej : list nat)
  : option (list (list nat)) :=
  match fuel with
  | O => None
  | S fuel' =>
      let kept2 := resort (fresh_kept kept rej) in
      let rej1 := rej ++ stale_chains kept rej in
      match overfill kept2 with
      | Some ch =>
          match last_by_k (adds_on ch) kept2 with
          | Some v => repair_norecheck fuel' (drop v kept2) (rej1 ++ v)
          | None => None
          end
      | None =>
          match balance (concat kept2) with
          | None => Some kept2
          | Some (ch, f) =>
              match balance_victim ch f kept2 with
              | Some v => repair_norecheck fuel' (drop v kept2) (rej1 ++ v)
              | None => None
              end
          end
      end
  end.

Definition lineage_only (kept : list (list nat)) (rej : list nat) : list (list nat) :=
  resort (fresh_kept kept rej).

(** *** Invariants *)

Definition pairwise_cf (l : list (list nat)) : Prop := pairwise (fun a b => conflict a b = false) l.
Definition no_cross_mix (l : list (list nat)) : Prop := pairwise (fun a b => cross_mix a b = false) l.
Definition lineage_closed (l : list (list nat)) (rej : list nat) : Prop :=
  forall x, In x (concat l) -> stale (rejected_blocks rej) x = false.
Definition forced (p : list nat -> bool) (K : list (list nat)) : Prop :=
  forall c, In c K -> p c = true -> pinned_cand c = true.
Definition balance_forced (ch : nat) (f : failure) (K : list (list nat)) : Prop :=
  match f with
  | Overflow => forced (contributes ch is_positive) K
  | Negative =>
      forced (contributes ch is_negative) K /\
      ((exists c, In c K /\ contributes ch is_negative c = true) \/
       forced (contributes ch any_sign) K)
  end.

(** What a walk rejection depended on: [prior] holds the candidates that
    came earlier in K. *)
Definition walk_ok (prior : list (list nat)) (c : list nat) (w : witness) : Prop :=
  match w with
  | WConflict k => In k prior /\ conflict c k = true
  | WMixing K => incl K prior /\ mixes K c = true
  | WLedger K => incl K prior /\ try_add K c = false
  | _ => False
  end.

(** What a repair rejection depended on. *)
Definition repair_ok (rejf : list nat) (c : list nat) (w : witness) : Prop :=
  match w with
  | WStale r =>
      exists x, c = [x] /\ pinned x = false /\ anc r (src x) = true /\ In r (rejected_blocks rejf)
  | WLineageConflict e => conflict e c = true /\ pfirst e c
  | WOverfill ch K => In c K /\ adds_on ch c = true /\ overfill K = Some ch /\
                      last_by_k (adds_on ch) K = Some c
  | WBalance ch f K => In c K /\ balance (concat K) = Some (ch, f) /\ balance_victim ch f K = Some c
  | _ => False
  end.

(** The static content of a witness: the condition held when the pass
    recorded it. *)
Definition holds (rejf : list nat) (c : list nat) (w : witness) : Prop :=
  match w with
  | WConflict k => conflict c k = true
  | WMixing K => mixes K c = true
  | WLedger K => try_add K c = false
  | WStale r =>
      exists x, c = [x] /\ pinned x = false /\ anc r (src x) = true /\ In r (rejected_blocks rejf)
  | WLineageConflict e => conflict e c = true
  | WOverfill ch K => In c K /\ adds_on ch c = true /\ overfill K = Some ch
  | WBalance ch f K => In c K /\ balance (concat K) = Some (ch, f) /\ balance_victim ch f K = Some c
  end.

(** L8: the cases in which a pinned candidate can lose. *)
Definition pinned_loss_allowed (w : witness) : Prop :=
  match w with
  | WConflict k => pinned_cand k = true
  | WMixing K => Forall (fun k => pinned_cand k = true) K
  | WLedger K => Forall (fun k => pinned_cand k = true) K
  | WStale _ => False
  | WLineageConflict k => pinned_cand k = true
  | WOverfill ch K => forced (adds_on ch) K
  | WBalance ch f K => balance_forced ch f K
  end.

(** L9: a walk rejection that names only pinned candidates. *)
Definition names_only_pinned (w : witness) : Prop :=
  match w with
  | WConflict k => pinned_cand k = true
  | WMixing K => Forall (fun k => pinned_cand k = true) K
  | WLedger K => Forall (fun k => pinned_cand k = true) K
  | _ => False
  end.

(** *** Basic facts *)

Lemma cross_mix_sym : forall a b, cross_mix a b = cross_mix b a.
Proof.
  intros a b. unfold cross_mix.
  rewrite (meets_sym (flat_map folded a)), (meets_sym (flat_map plain a)). apply orb_comm.
Qed.

Lemma cross_mix_antitone : antitone (fun a b => cross_mix a b = false).
Proof.
  intros a b a' b' H Ha Hb. unfold cross_mix in *. apply orb_false_iff in H as [H1 H2].
  apply orb_false_iff. split.
  - apply (meets_incl _ _ _ _ H1); apply flat_map_incl; assumption.
  - apply (meets_incl _ _ _ _ H2); apply flat_map_incl; assumption.
Qed.

Lemma walk_ok_mono : forall P P' c w, incl P P' -> walk_ok P c w -> walk_ok P' c w.
Proof.
  intros P P' c w Hi H. destruct w; simpl in *; try contradiction.
  - destruct H as [Hk Hc]. split; [apply Hi; exact Hk | exact Hc].
  - destruct H as [HK Hm]. split; [intros y Hy; apply Hi, HK, Hy | exact Hm].
  - destruct H as [HK Ht]. split; [intros y Hy; apply Hi, HK, Hy | exact Ht].
Qed.

Lemma rejected_blocks_app : forall rej more,
  incl (rejected_blocks rej) (rejected_blocks (rej ++ more)).
Proof.
  intros rej more r Hr. unfold rejected_blocks in *. apply in_map_iff in Hr as [y [Hy Hin]].
  apply in_map_iff. exists y. split; [exact Hy |].
  apply in_app_or in Hin as [Hin | Hin]; apply in_or_app; [left; apply in_or_app; left; exact Hin |
                                                         right; exact Hin].
Qed.

Lemma stale_witness_spec : forall rb x, stale rb x = true ->
  pinned x = false /\ anc (stale_witness rb x) (src x) = true /\ In (stale_witness rb x) rb.
Proof.
  intros rb x H. unfold stale in H. apply andb_true_iff in H as [Hp Hex].
  apply negb_true_iff in Hp. split; [exact Hp |].
  unfold stale_witness. destruct (find (fun r => anc r (src x)) rb) as [r |] eqn:Hf.
  - apply find_some in Hf as [Hin Hanc]. split; assumption.
  - apply existsb_exists in Hex as [r [Hin Hanc]].
    rewrite (find_none _ _ Hf r Hin) in Hanc. discriminate.
Qed.

Lemma concat_fresh_kept : forall kept rej,
  concat (fresh_kept kept rej) = filter (fun x => negb (stale (rejected_blocks rej) x)) (concat kept).
Proof.
  intros kept rej. unfold fresh_kept, keep_fresh. rewrite concat_filter_nonempty.
  apply concat_map_filter.
Qed.

Lemma fresh_kept_nonempty : forall kept rej, all_nonempty (fresh_kept kept rej).
Proof.
  intros kept rej c Hc Heq. subst c. unfold fresh_kept in Hc.
  apply filter_In in Hc as [_ Hc]. discriminate Hc.
Qed.

Lemma fresh_kept_unchanged : forall kept rej,
  all_nonempty kept -> nonempty (stale_chains kept rej) = false -> fresh_kept kept rej = kept.
Proof.
  intros kept rej Hne Hs.
  assert (Hnone : forall x, In x (concat kept) -> stale (rejected_blocks rej) x = false).
  { intros x Hx. destruct (stale (rejected_blocks rej) x) eqn:Hst; [| reflexivity].
    unfold stale_chains in Hs.
    assert (Hin : In x (filter (stale (rejected_blocks rej)) (concat kept)))
      by (apply filter_In; split; assumption).
    destruct (filter (stale (rejected_blocks rej)) (concat kept)); [contradiction | discriminate Hs]. }
  unfold fresh_kept.
  assert (Hmap : map (keep_fresh (rejected_blocks rej)) kept = kept).
  { clear Hne Hs. revert Hnone. induction kept as [| c kept IHk]; intros Hnone; simpl; [reflexivity |].
    f_equal.
    - unfold keep_fresh. apply filter_id_all. intros x Hx.
      rewrite (Hnone x); [reflexivity |]. simpl. apply in_or_app. left. exact Hx.
    - apply IHk. intros x Hx. apply Hnone. simpl. apply in_or_app. right. exact Hx. }
  rewrite Hmap. apply filter_id_all. intros c Hc.
  destruct c as [| n c]; [exfalso; exact (Hne [] Hc eq_refl) | reflexivity].
Qed.

Lemma fresh_kept_pairwise : forall R kept rej,
  antitone R -> pairwise R kept -> pairwise R (fresh_kept kept rej).
Proof.
  intros R kept rej Hant H. unfold fresh_kept.
  apply (pairwise_incl _ (map (keep_fresh (rejected_blocks rej)) kept)).
  - apply pairwise_shrink; [exact Hant | | exact H].
    intros c x Hx. unfold keep_fresh in Hx. apply filter_In in Hx. exact (proj1 Hx).
  - intros c Hc. apply filter_In in Hc. exact (proj1 Hc).
Qed.

(** One pass of lineage closure is closed: ancestry is transitive, so a
    chain that descends from a chain removed in this pass also descends from
    the block that removed it (dag_merger.rs:1863-1865). *)
Lemma lineage_after_close : forall kept rej x,
  In x (concat kept) -> stale (rejected_blocks rej) x = false ->
  stale (rejected_blocks (rej ++ stale_chains kept rej)) x = false.
Proof.
  intros kept rej x Hx Hfresh.
  destruct (stale (rejected_blocks (rej ++ stale_chains kept rej)) x) eqn:Hst; [| reflexivity].
  exfalso. unfold stale in Hst. apply andb_true_iff in Hst as [Hp Hex].
  apply existsb_exists in Hex as [r [Hr Hanc]].
  unfold rejected_blocks in Hr. apply in_map_iff in Hr as [y [Hy Hyin]]. subst r.
  rewrite <- app_assoc in Hyin. apply in_app_or in Hyin as [Hyr | Hyin].
  - assert (Hs : stale (rejected_blocks rej) x = true).
    { unfold stale. rewrite Hp. simpl. apply existsb_exists. exists (src y). split; [| exact Hanc].
      unfold rejected_blocks. apply in_map. apply in_or_app. left. exact Hyr. }
    congruence.
  - apply in_app_or in Hyin as [Hys | Hyb].
    + unfold stale_chains in Hys. apply filter_In in Hys as [_ Hys].
      unfold stale in Hys. apply andb_true_iff in Hys as [_ Hex'].
      apply existsb_exists in Hex' as [r0 [Hr0 Hanc0]].
      assert (Hs : stale (rejected_blocks rej) x = true).
      { unfold stale. rewrite Hp. simpl. apply existsb_exists. exists r0.
        split; [exact Hr0 | exact (anc_trans _ _ _ Hanc0 Hanc)]. }
      congruence.
    + assert (Hs : stale (rejected_blocks rej) x = true).
      { unfold stale. rewrite Hp. simpl. apply existsb_exists. exists (src y). split; [| exact Hanc].
        unfold rejected_blocks. apply in_map. apply in_or_app. right. exact Hyb. }
      congruence.
Qed.

Lemma first_conflict_none : forall l, first_conflict l = None -> pairwise_cf l.
Proof.
  induction l as [| x rest IH]; intros H; simpl in H.
  - intros a b Ha. contradiction.
  - destruct (find (conflict x) rest) eqn:Hf; [discriminate |].
    specialize (IH H).
    intros a b Ha Hb Hab. destruct Ha as [Ha | Ha]; destruct Hb as [Hb | Hb].
    + subst a b. exfalso. apply Hab. reflexivity.
    + subst a. exact (find_none _ _ Hf b Hb).
    + subst b. rewrite conflict_sym. exact (find_none _ _ Hf a Ha).
    + exact (IH a b Ha Hb Hab).
Qed.

Lemma first_conflict_some : forall l e v, first_conflict l = Some (e, v) ->
  In e l /\ In v l /\ conflict e v = true.
Proof.
  induction l as [| x rest IH]; intros e v H; simpl in H; [discriminate |].
  destruct (find (conflict x) rest) as [b |] eqn:Hf.
  - injection H as He Hv. subst e v. apply find_some in Hf as [Hin Hc].
    split; [left; reflexivity | split; [right; exact Hin | exact Hc]].
  - destruct (IH e v H) as [He [Hv Hc]].
    split; [right; exact He | split; [right; exact Hv | exact Hc]].
Qed.

Lemma first_conflict_pfirst : forall l e v,
  StronglySorted pfirst l -> first_conflict l = Some (e, v) -> pfirst e v.
Proof.
  induction l as [| x rest IH]; intros e v Hs H; simpl in H; [discriminate |].
  apply StronglySorted_inv in Hs as [Hs Hx].
  destruct (find (conflict x) rest) as [b |] eqn:Hf.
  - injection H as He Hv. subst e v. apply find_some in Hf as [Hin _].
    rewrite Forall_forall in Hx. exact (Hx b Hin).
  - exact (IH e v Hs H).
Qed.

Lemma sorted_prefix_pinned : forall pre c post,
  StronglySorted pfirst (pre ++ c :: post) -> pinned_cand c = true ->
  forall p, In p pre -> pinned_cand p = true.
Proof.
  induction pre as [| q pre IHp]; intros c post Hs Hc p Hp; [contradiction |].
  simpl in Hs. apply StronglySorted_inv in Hs as [Hs Hq].
  destruct Hp as [Hp | Hp].
  - subst q. rewrite Forall_forall in Hq.
    assert (Hpf : pfirst p c) by (apply Hq; apply in_or_app; right; left; reflexivity).
    destruct Hpf as [Hpf | Hpf]; [exact Hpf | rewrite Hc in Hpf; discriminate].
  - exact (IHp c post Hs Hc p Hp).
Qed.

Lemma last_by_k_some : forall p l v, last_by_k p l = Some v -> In v l /\ p v = true.
Proof.
  intros p l v H. unfold last_by_k in H.
  destruct (last_sat (fun c => negb (pinned_cand c) && p c) l) as [v' |] eqn:H1.
  - injection H as Hv. subst v'. apply last_sat_some in H1 as [Hin Hp].
    apply andb_true_iff in Hp as [_ Hp]. split; assumption.
  - apply last_sat_some in H as [Hin Hp]. apply andb_true_iff in Hp as [_ Hp]. split; assumption.
Qed.

Lemma last_by_k_exists : forall p l c, In c l -> p c = true -> exists v, last_by_k p l = Some v.
Proof.
  intros p l c Hc Hp. unfold last_by_k.
  destruct (last_sat (fun c => negb (pinned_cand c) && p c) l) as [v |] eqn:H1;
    [exists v; reflexivity |].
  destruct (last_sat (fun c => pinned_cand c && p c) l) as [v |] eqn:H2;
    [exists v; reflexivity |].
  exfalso.
  pose proof (last_sat_none _ _ H1 c Hc) as N1. pose proof (last_sat_none _ _ H2 c Hc) as N2.
  simpl in N1, N2. rewrite Hp in N1, N2.
  destruct (pinned_cand c); simpl in N1, N2; discriminate.
Qed.

Lemma last_by_k_forced : forall p l v, last_by_k p l = Some v -> pinned_cand v = true -> forced p l.
Proof.
  intros p l v H Hv c Hc Hp. unfold last_by_k in H.
  destruct (last_sat (fun c => negb (pinned_cand c) && p c) l) as [v' |] eqn:H1.
  - injection H as Heq. subst v'. apply last_sat_some in H1 as [_ Hq].
    apply andb_true_iff in Hq as [Hq _]. rewrite Hv in Hq. discriminate.
  - pose proof (last_sat_none _ _ H1 c Hc) as N. simpl in N. rewrite Hp in N.
    destruct (pinned_cand c); [reflexivity | discriminate].
Qed.

Lemma balance_victim_some : forall ch f l v, balance_victim ch f l = Some v -> In v l.
Proof.
  intros ch f l v H. destruct f; simpl in H.
  - destruct (last_by_k (contributes ch is_negative) l) as [v' |] eqn:H1.
    + injection H as Hv. subst v'. exact (proj1 (last_by_k_some _ _ _ H1)).
    + exact (proj1 (last_by_k_some _ _ _ H)).
  - exact (proj1 (last_by_k_some _ _ _ H)).
Qed.

Lemma balance_victim_exists : forall ch f l,
  balance (concat l) = Some (ch, f) -> exists v, balance_victim ch f l = Some v.
Proof.
  intros ch f l H. destruct (balance_has_contributor _ _ _ H) as [x [d [Hx [Hd Hpos]]]].
  apply in_concat in Hx as [c [Hc Hxc]].
  destruct f; simpl.
  - destruct (last_by_k (contributes ch is_negative) l) as [v |] eqn:H1; [exists v; reflexivity |].
    apply (last_by_k_exists _ _ c Hc). unfold contributes. apply existsb_exists.
    exists x. split; [exact Hxc |]. rewrite Hd. reflexivity.
  - apply (last_by_k_exists _ _ c Hc). unfold contributes. apply existsb_exists.
    exists x. split; [exact Hxc |]. rewrite Hd. unfold is_positive. apply Z.ltb_lt.
    apply Hpos. reflexivity.
Qed.

Lemma balance_victim_forced : forall ch f l v,
  balance_victim ch f l = Some v -> pinned_cand v = true -> balance_forced ch f l.
Proof.
  intros ch f l v H Hv. destruct f; simpl in H; simpl.
  - destruct (last_by_k (contributes ch is_negative) l) as [v' |] eqn:H1.
    + injection H as Heq. subst v'. split; [exact (last_by_k_forced _ _ _ H1 Hv) |].
      left. exists v. exact (last_by_k_some _ _ _ H1).
    + split.
      * intros c Hc Hp. destruct (last_by_k_exists _ _ c Hc Hp) as [w Hw].
        rewrite Hw in H1. discriminate.
      * right. exact (last_by_k_forced _ _ _ H Hv).
  - exact (last_by_k_forced _ _ _ H Hv).
Qed.

(** *** S5: the walk *)

Lemma walk_prefix : forall cands kept log kept' log', walk cands kept log = (kept', log') ->
  exists added, kept' = kept ++ added /\ incl added cands.
Proof.
  induction cands as [| c rest IH]; intros kept log kept' log' Hw; simpl in Hw.
  - injection Hw as Hk Hl. subst kept'. exists []. split; [rewrite app_nil_r; reflexivity |].
    intros x Hx. contradiction.
  - destruct (find (conflict c) kept) as [k |] eqn:Hf.
    + destruct (IH _ _ _ _ Hw) as [added [Heq Hi]]. exists added. split; [exact Heq |].
      intros x Hx. right. apply Hi. exact Hx.
    + destruct (mixes kept c) eqn:Hm.
      * destruct (IH _ _ _ _ Hw) as [added [Heq Hi]]. exists added. split; [exact Heq |].
        intros x Hx. right. apply Hi. exact Hx.
      * destruct (try_add kept c) eqn:Ht.
        -- destruct (IH _ _ _ _ Hw) as [added [Heq Hi]]. exists (c :: added).
           split; [rewrite Heq, <- app_assoc; reflexivity |].
           intros x [Hx | Hx]; [left; exact Hx | right; apply Hi; exact Hx].
        -- destruct (IH _ _ _ _ Hw) as [added [Heq Hi]]. exists added. split; [exact Heq |].
           intros x Hx. right. apply Hi. exact Hx.
Qed.

Lemma walk_inv : forall cands kept log kept' log', walk cands kept log = (kept', log') ->
  pairwise_cf kept -> no_cross_mix kept -> ledger_ok (concat kept) = true ->
  pairwise_cf kept' /\ no_cross_mix kept' /\ ledger_ok (concat kept') = true.
Proof.
  induction cands as [| c rest IH]; intros kept log kept' log' Hw Hcf Hmx Hl; simpl in Hw.
  - injection Hw as Hk _. subst kept'. split; [exact Hcf | split; [exact Hmx | exact Hl]].
  - destruct (find (conflict c) kept) as [k |] eqn:Hf; [exact (IH _ _ _ _ Hw Hcf Hmx Hl) |].
    destruct (mixes kept c) eqn:Hm; [exact (IH _ _ _ _ Hw Hcf Hmx Hl) |].
    destruct (try_add kept c) eqn:Ht; [| exact (IH _ _ _ _ Hw Hcf Hmx Hl)].
    apply (IH _ _ _ _ Hw).
    + intros a b Ha Hb Hab. apply in_app_or in Ha. apply in_app_or in Hb.
      destruct Ha as [Ha | [Ha | []]]; destruct Hb as [Hb | [Hb | []]].
      * exact (Hcf a b Ha Hb Hab).
      * subst b. rewrite conflict_sym. exact (find_none _ _ Hf a Ha).
      * subst a. exact (find_none _ _ Hf b Hb).
      * subst a b. exfalso. apply Hab. reflexivity.
    + intros a b Ha Hb Hab. apply in_app_or in Ha. apply in_app_or in Hb.
      destruct Ha as [Ha | [Ha | []]]; destruct Hb as [Hb | [Hb | []]].
      * exact (Hmx a b Ha Hb Hab).
      * subst b. rewrite cross_mix_sym. exact (existsb_false _ _ _ Hm a Ha).
      * subst a. exact (existsb_false _ _ _ Hm b Hb).
      * subst a b. exfalso. apply Hab. reflexivity.
    + rewrite concat_app. simpl. rewrite app_nil_r. exact Ht.
Qed.

Lemma walk_nodup : forall cands kept log kept' log', walk cands kept log = (kept', log') ->
  NoDup (concat (kept ++ cands)) ->
  NoDup (concat kept') /\ length (concat kept') <= length (concat (kept ++ cands)).
Proof.
  induction cands as [| c rest IH]; intros kept log kept' log' Hw Hnd; simpl in Hw.
  - injection Hw as Hk _. subst kept'. rewrite app_nil_r in Hnd |- *. split; [exact Hnd | lia].
  - assert (Hdrop : NoDup (concat (kept ++ rest)) /\
                    length (concat (kept ++ rest)) <= length (concat (kept ++ c :: rest))).
    { rewrite !concat_app in Hnd. simpl in Hnd. split.
      - rewrite concat_app. exact (nodup_remove_middle _ _ _ _ Hnd).
      - rewrite !concat_app. simpl. rewrite !length_app. lia. }
    destruct Hdrop as [Hnd' Hle].
    destruct (find (conflict c) kept) as [k |] eqn:Hf.
    + destruct (IH _ _ _ _ Hw Hnd') as [H1 H2]. split; [exact H1 | lia].
    + destruct (mixes kept c) eqn:Hm.
      * destruct (IH _ _ _ _ Hw Hnd') as [H1 H2]. split; [exact H1 | lia].
      * destruct (try_add kept c) eqn:Ht.
        -- assert (Heq : (kept ++ [c]) ++ rest = kept ++ c :: rest) by (rewrite <- app_assoc; reflexivity).
           rewrite <- Heq in Hnd |- *. exact (IH _ _ _ _ Hw Hnd).
        -- destruct (IH _ _ _ _ Hw Hnd') as [H1 H2]. split; [exact H1 | lia].
Qed.

Lemma walk_log : forall cands kept log kept' log', walk cands kept log = (kept', log') ->
  exists new, log' = log ++ new /\
    (forall c w, In (c, w) new -> exists pre post, cands = pre ++ c :: post /\ walk_ok (kept ++ pre) c w) /\
    (forall c, In c cands -> In c kept' \/ exists w, In (c, w) new) /\
    (NoDup (concat (kept ++ cands)) -> forall c w x, In (c, w) new -> In x c -> ~ In x (concat kept')).
Proof.
  induction cands as [| c rest IH]; intros kept log kept' log' Hw; simpl in Hw.
  - injection Hw as Hk Hl. subst kept' log'. exists []. split; [rewrite app_nil_r; reflexivity |].
    split; [intros c w H; contradiction |]. split; [intros c H; contradiction |].
    intros _ c w x H. contradiction.
  - (* the shared shape of the three rejection cases *)
    assert (Hreject : forall w0, walk_ok kept c w0 ->
      walk rest kept (log ++ [(c, w0)]) = (kept', log') ->
      exists new, log' = log ++ new /\
        (forall c' w, In (c', w) new -> exists pre post, c :: rest = pre ++ c' :: post /\
                                                   walk_ok (kept ++ pre) c' w) /\
        (forall c', In c' (c :: rest) -> In c' kept' \/ exists w, In (c', w) new) /\
        (NoDup (concat (kept ++ c :: rest)) ->
         forall c' w x, In (c', w) new -> In x c' -> ~ In x (concat kept'))).
    { intros w0 Hok Hw'.
      destruct (IH _ _ _ _ Hw') as [new' [Hlog [Hpos [Hpart Hdis]]]].
      destruct (walk_prefix _ _ _ _ _ Hw') as [added [Hk Hadded]].
      exists ((c, w0) :: new'). split; [rewrite Hlog, <- app_assoc; reflexivity |].
      split.
      - intros c' w [Heq | Hin].
        + injection Heq as Hc Hw0. subst c' w. exists [], rest. split; [reflexivity |].
          rewrite app_nil_r. exact Hok.
        + destruct (Hpos c' w Hin) as [pre [post [Hrest Hok']]].
          exists (c :: pre), post. split; [rewrite Hrest; reflexivity |].
          apply (walk_ok_mono (kept ++ pre)); [| exact Hok'].
          intros y Hy. apply in_app_or in Hy as [Hy | Hy]; apply in_or_app; [left; exact Hy |].
          right. right. exact Hy.
      - split.
        + intros c' [Heq | Hin].
          * subst c'. right. exists w0. left. reflexivity.
          * destruct (Hpart c' Hin) as [Hk' | [w Hw'']]; [left; exact Hk' |].
            right. exists w. right. exact Hw''.
        + intros Hnd c' w x [Heq | Hin] Hx.
          * injection Heq as Hc _. subst c'.
            destruct (nodup_concat_split kept rest c x Hnd Hx) as [HA HB].
            rewrite Hk, concat_app. intros Hx'. apply in_app_or in Hx' as [Hx' | Hx'];
              [exact (HA Hx') |].
            apply HB. apply in_concat in Hx' as [d [Hd Hxd]]. apply in_concat.
            exists d. split; [apply Hadded; exact Hd | exact Hxd].
          * assert (Hnd' : NoDup (concat (kept ++ rest))).
            { rewrite concat_app in Hnd |- *. simpl in Hnd. exact (nodup_remove_middle _ _ _ _ Hnd). }
            exact (Hdis Hnd' c' w x Hin Hx). }
    destruct (find (conflict c) kept) as [k |] eqn:Hf.
    + apply (Hreject (WConflict k)); [| exact Hw].
      apply find_some in Hf as [Hk Hc]. split; [exact Hk | exact Hc].
    + destruct (mixes kept c) eqn:Hm.
      * apply (Hreject (WMixing kept)); [| exact Hw]. split; [intros y Hy; exact Hy | exact Hm].
      * destruct (try_add kept c) eqn:Ht.
        -- destruct (IH _ _ _ _ Hw) as [new [Hlog [Hpos [Hpart Hdis]]]].
           destruct (walk_prefix _ _ _ _ _ Hw) as [added [Hk _]].
           exists new. split; [exact Hlog |]. split.
           ++ intros c' w Hin. destruct (Hpos c' w Hin) as [pre [post [Hrest Hok]]].
              exists (c :: pre), post. split; [rewrite Hrest; reflexivity |].
              rewrite <- app_assoc in Hok. exact Hok.
           ++ split.
              ** intros c' [Heq | Hin]; [| exact (Hpart c' Hin)].
                 subst c'. left. rewrite Hk. apply in_or_app. left. apply in_or_app. right.
                 left. reflexivity.
              ** intros Hnd. apply Hdis. rewrite <- app_assoc. exact Hnd.
        -- apply (Hreject (WLedger kept)); [| exact Hw]. split; [intros y Hy; exact Hy | exact Ht].
Qed.

(** *** S6: the repair loop *)

(** Everything the repair loop guarantees about its result. *)
Definition repair_post (kept : list (list nat)) (rej : list nat) (log : list (list nat * witness))
  (kept' : list (list nat)) (rej' : list nat) (log' : list (list nat * witness)) : Prop :=
  pairwise_cf kept' /\ overfill kept' = None /\ balance (concat kept') = None /\
  lineage_closed kept' rej' /\ all_nonempty kept' /\ NoDup (concat kept') /\
  incl (concat kept') (concat kept) /\
  (forall R, antitone R -> pairwise R kept -> pairwise R kept') /\
  (forall U, filter_closed U -> (forall c, In c kept -> U c) -> forall c, In c kept' -> U c) /\
  (exists rnew new, rej' = rej ++ rnew /\ log' = log ++ new /\
     (forall c w, In (c, w) new -> repair_ok rej' c w) /\
     (forall x, In x (concat kept) -> In x (concat kept') \/ exists c w, In (c, w) new /\ In x c) /\
     (forall x c w, In (c, w) new -> In x c -> ~ In x (concat kept')) /\
     (forall x, In x rnew -> exists c w, In (c, w) new /\ In x c)).

Lemma repair_spec : forall fuel recheck kept rej log,
  length (concat kept) < fuel -> all_nonempty kept -> NoDup (concat kept) ->
  (recheck = false -> pairwise_cf kept) ->
  exists kept' rej' log', repair fuel recheck kept rej log = Some (kept', rej', log') /\
    repair_post kept rej log kept' rej' log'.
Proof.
  induction fuel as [| fuel IH]; intros recheck kept rej log Hlen Hne Hnd Hcf; [lia |].
  cbn [repair].
  set (sc := stale_chains kept rej).
  set (kept1 := fresh_kept kept rej).
  set (kept2 := resort kept1).
  set (log1 := log ++ stale_log kept rej).
  assert (Hc1 : concat kept1 = filter (fun x => negb (stale (rejected_blocks rej) x)) (concat kept))
    by apply concat_fresh_kept.
  assert (Hp2 : Permutation kept2 kept1) by apply resort_perm.
  assert (Hcp2 : Permutation (concat kept2) (concat kept1)) by (apply perm_concat; exact Hp2).
  assert (Hin21 : forall c, In c kept2 -> In c kept1) by (intros c Hc; exact (Permutation_in c Hp2 Hc)).
  assert (Hcin2 : forall x, In x (concat kept2) <->
                            In x (concat kept) /\ stale (rejected_blocks rej) x = false).
  { intros x. split.
    - intros Hx. apply (Permutation_in x Hcp2) in Hx. rewrite Hc1 in Hx.
      apply filter_In in Hx as [Hx Hs]. split; [exact Hx |].
      destruct (stale (rejected_blocks rej) x); [discriminate | reflexivity].
    - intros [Hx Hs]. apply (Permutation_in x (Permutation_sym Hcp2)). rewrite Hc1.
      apply filter_In. split; [exact Hx | rewrite Hs; reflexivity]. }
  assert (Hne2 : all_nonempty kept2)
    by (intros c Hc; exact (fresh_kept_nonempty kept rej c (Hin21 c Hc))).
  assert (Hnd2 : NoDup (concat kept2)).
  { apply (Permutation_NoDup (Permutation_sym Hcp2)). rewrite Hc1. apply NoDup_filter. exact Hnd. }
  assert (Hlen2 : length (concat kept2) <= length (concat kept)).
  { rewrite (Permutation_length Hcp2), Hc1. apply filter_length_le. }
  assert (Hincl2 : incl (concat kept2) (concat kept)) by (intros x Hx; exact (proj1 (proj1 (Hcin2 x) Hx))).
  assert (Hsorted2 : StronglySorted pfirst kept2) by apply resort_pinned_first.
  assert (Hclosed2 : lineage_closed kept2 (rej ++ sc)).
  { intros x Hx. apply Hcin2 in Hx as [Hx Hs]. exact (lineage_after_close kept rej x Hx Hs). }
  assert (Hcf2 : (recheck || nonempty sc) = false -> pairwise_cf kept2).
  { intros H. apply orb_false_iff in H as [Hr Hs].
    assert (Hk : kept1 = kept) by (apply fresh_kept_unchanged; [exact Hne | exact Hs]).
    apply (pairwise_incl _ kept); [exact (Hcf Hr) |]. intros c Hc. rewrite <- Hk. exact (Hin21 c Hc). }
  assert (Hant2 : forall R, antitone R -> pairwise R kept -> pairwise R kept2).
  { intros R Hant H. apply (pairwise_incl _ kept1); [| intros c Hc; exact (Hin21 c Hc)].
    apply fresh_kept_pairwise; assumption. }
  assert (Huna2 : forall U, filter_closed U -> (forall c, In c kept -> U c) -> forall c, In c kept2 -> U c).
  { intros U HU H c Hc. apply Hin21 in Hc. unfold kept1, fresh_kept in Hc.
    apply filter_In in Hc as [Hc _]. apply in_map_iff in Hc as [c0 [Hc0eq Hc0]]. subst c.
    unfold keep_fresh. apply HU. apply H. exact Hc0. }
  assert (Hstale_ok : forall c w, In (c, w) (stale_log kept rej) ->
                      forall rej', incl (rejected_blocks rej) (rejected_blocks rej') -> repair_ok rej' c w).
  { intros c w Hcw rej' Hi. unfold stale_log in Hcw. apply in_map_iff in Hcw as [x [Heq Hx]].
    injection Heq as Hc Hw. subst c w. unfold stale_chains in Hx. apply filter_In in Hx as [_ Hst].
    destruct (stale_witness_spec _ _ Hst) as [Hpin [Hanc Hin]].
    exists x. split; [reflexivity | split; [exact Hpin | split; [exact Hanc | apply Hi; exact Hin]]]. }
  assert (Hstale_cover : forall x, In x sc -> exists c w, In (c, w) (stale_log kept rej) /\ In x c).
  { intros x Hx. exists [x], (WStale (stale_witness (rejected_blocks rej) x)). split; [| left; reflexivity].
    unfold stale_log. apply in_map_iff. exists x. split; [reflexivity | exact Hx]. }
  assert (Hstale_disj : forall x c w, In (c, w) (stale_log kept rej) -> In x c -> ~ In x (concat kept2)).
  { intros x c w Hcw Hx Hx2. unfold stale_log in Hcw. apply in_map_iff in Hcw as [y [Heq Hy]].
    injection Heq as Hc _. subst c. destruct Hx as [Hx | []]. subst y.
    unfold stale_chains in Hy. apply filter_In in Hy as [_ Hst].
    apply Hcin2 in Hx2 as [_ Hs]. congruence. }
  assert (Hcover2 : forall x, In x (concat kept) -> In x (concat kept2) \/ In x sc).
  { intros x Hx. destruct (stale (rejected_blocks rej) x) eqn:Hst.
    - right. unfold sc, stale_chains. apply filter_In. split; assumption.
    - left. apply Hcin2. split; assumption. }
  (* The step that drops one whole candidate and recurs. *)
  assert (Hstep : forall v w recheck',
    In v kept2 -> (forall rej', repair_ok rej' v w) ->
    (recheck' = false -> pairwise_cf (drop v kept2)) ->
    exists kept' rej' log',
      repair fuel recheck' (drop v kept2) ((rej ++ sc) ++ v) (log1 ++ [(v, w)]) = Some (kept', rej', log') /\
      repair_post kept rej log kept' rej' log').
  { intros v w recheck' Hv Hw Hcf'.
    assert (Hvne : v <> []) by (apply Hne2; exact Hv).
    assert (Hlen' : length (concat (drop v kept2)) < fuel)
      by (pose proof (length_concat_drop v kept2 Hv Hvne); lia).
    assert (Hne' : all_nonempty (drop v kept2))
      by (intros c Hc; apply in_drop in Hc as [Hc _]; apply Hne2; exact Hc).
    assert (Hnd' : NoDup (concat (drop v kept2))) by (apply nodup_concat_drop; exact Hnd2).
    destruct (IH recheck' (drop v kept2) ((rej ++ sc) ++ v) (log1 ++ [(v, w)]) Hlen' Hne' Hnd' Hcf')
      as [kept' [rej' [log' [Hrun Hpost]]]].
    + exists kept', rej', log'. split; [exact Hrun |].
      destruct Hpost as [Pcf [Pov [Pbal [Pcl [Pne [Pnd [Pincl [Pant [Puna Prest]]]]]]]]].
      destruct Prest as [rnew [new [Hrej [Hlog [Hok [Hcov [Hdis Hrn]]]]]]].
      assert (Hdrop2 : incl (concat (drop v kept2)) (concat kept2)) by apply incl_concat_drop.
      split; [exact Pcf |]. split; [exact Pov |]. split; [exact Pbal |]. split; [exact Pcl |].
      split; [exact Pne |]. split; [exact Pnd |].
      split; [intros x Hx; apply Hincl2, Hdrop2, Pincl, Hx |].
      split.
      { intros R Hant H. apply Pant; [exact Hant |]. apply (pairwise_incl _ kept2); [apply Hant2; assumption |].
        intros c Hc. apply in_drop in Hc. exact (proj1 Hc). }
      split.
      { intros U HU H c Hc. apply (Puna U HU); [| exact Hc]. intros c0 Hc0.
        apply in_drop in Hc0 as [Hc0 _]. exact (Huna2 U HU H c0 Hc0). }
      exists (sc ++ v ++ rnew), (stale_log kept rej ++ (v, w) :: new).
      split; [rewrite Hrej, <- !app_assoc; reflexivity |].
      split; [rewrite Hlog; unfold log1; rewrite <- !app_assoc; reflexivity |].
      split.
      { intros c w' Hin. apply in_app_or in Hin as [Hin | [Heq | Hin]].
        - apply (Hstale_ok c w' Hin). rewrite Hrej, <- !app_assoc. apply rejected_blocks_app.
        - injection Heq as Hc Hw'. subst c w'. apply Hw.
        - exact (Hok c w' Hin). }
      split.
      { intros x Hx. destruct (Hcover2 x Hx) as [Hx2 | Hxs].
        - destruct (concat_drop_split v kept2 x Hv Hx2) as [Hxd | Hxv].
          + destruct (Hcov x Hxd) as [Hk | [c [w' [Hin Hxc]]]]; [left; exact Hk |].
            right. exists c, w'. split; [apply in_or_app; right; right; exact Hin | exact Hxc].
          + right. exists v, w. split; [apply in_or_app; right; left; reflexivity | exact Hxv].
        - right. destruct (Hstale_cover x Hxs) as [c [w' [Hin Hxc]]].
          exists c, w'. split; [apply in_or_app; left; exact Hin | exact Hxc]. }
      split.
      { intros x c w' Hin Hx Hk. apply in_app_or in Hin as [Hin | [Heq | Hin]].
        - apply (Hstale_disj x c w' Hin Hx). apply Hdrop2, Pincl, Hk.
        - injection Heq as Hc _. subst c.
          apply (nodup_concat_drop_disjoint v kept2 x Hnd2 Hv Hx). apply Pincl, Hk.
        - exact (Hdis x c w' Hin Hx Hk). }
      { intros x Hx. apply in_app_or in Hx as [Hx | Hx].
        - destruct (Hstale_cover x Hx) as [c [w' [Hin Hxc]]].
          exists c, w'. split; [apply in_or_app; left; exact Hin | exact Hxc].
        - apply in_app_or in Hx as [Hx | Hx].
          + exists v, w. split; [apply in_or_app; right; left; reflexivity | exact Hx].
          + destruct (Hrn x Hx) as [c [w' [Hin Hxc]]].
            exists c, w'. split; [apply in_or_app; right; right; exact Hin | exact Hxc]. } }
  destruct (if recheck || nonempty sc then first_conflict kept2 else None) as [[e v] |] eqn:Hfc.
  - assert (Hfc' : first_conflict kept2 = Some (e, v)).
    { revert Hfc. destruct (recheck || nonempty sc); intros Hfc; [exact Hfc | discriminate Hfc]. }
    destruct (first_conflict_some _ _ _ Hfc') as [He [Hv Hconf]].
    apply (Hstep v (WLineageConflict e) true Hv).
    + intros rej'. simpl. split; [exact Hconf | exact (first_conflict_pfirst _ _ _ Hsorted2 Hfc')].
    + intros H. discriminate.
  - assert (Hcf3 : pairwise_cf kept2).
    { revert Hfc. destruct (recheck || nonempty sc) eqn:Hrs; intros Hfc.
      - apply first_conflict_none. exact Hfc.
      - apply Hcf2. reflexivity. }
    assert (Hcfd : forall v, pairwise_cf (drop v kept2)).
    { intros v. apply (pairwise_incl _ kept2); [exact Hcf3 |]. intros c Hc.
      apply in_drop in Hc. exact (proj1 Hc). }
    destruct (overfill kept2) as [ch |] eqn:Hov.
    + destruct (overfill_has_adder _ _ Hov) as [c [x [Hc [Hx Hadd]]]].
      assert (Hadds : adds_on ch c = true)
        by (unfold adds_on; apply existsb_exists; exists x; split; assumption).
      destruct (last_by_k_exists (adds_on ch) kept2 c Hc Hadds) as [v Hv]. rewrite Hv.
      destruct (last_by_k_some _ _ _ Hv) as [Hvin Hvadd].
      apply (Hstep v (WOverfill ch kept2) false Hvin).
      * intros rej'. simpl. split; [exact Hvin | split; [exact Hvadd | split; [exact Hov | exact Hv]]].
      * intros _. apply Hcfd.
    + destruct (balance (concat kept2)) as [[ch f] |] eqn:Hbal.
      * destruct (balance_victim_exists ch f kept2 Hbal) as [v Hv]. rewrite Hv.
        assert (Hvin : In v kept2) by exact (balance_victim_some _ _ _ _ Hv).
        apply (Hstep v (WBalance ch f kept2) false Hvin).
        -- intros rej'. simpl. split; [exact Hvin | split; [exact Hbal | exact Hv]].
        -- intros _. apply Hcfd.
      * exists kept2, (rej ++ sc), log1. split; [reflexivity |].
        split; [exact Hcf3 |]. split; [exact Hov |]. split; [exact Hbal |]. split; [exact Hclosed2 |].
        split; [exact Hne2 |]. split; [exact Hnd2 |]. split; [exact Hincl2 |].
        split; [exact Hant2 |]. split; [exact Huna2 |].
        exists sc, (stale_log kept rej). split; [reflexivity |]. split; [reflexivity |].
        split; [intros c w Hin; apply (Hstale_ok c w Hin); apply rejected_blocks_app |].
        split.
        { intros x Hx. destruct (Hcover2 x Hx) as [Hx2 | Hxs]; [left; exact Hx2 |].
          right. exact (Hstale_cover x Hxs). }
        split; [exact Hstale_disj | exact Hstale_cover].
Qed.

(** *** The pass *)

Lemma pass_spec : forall cands rej0,
  all_nonempty cands -> NoDup (concat cands) ->
  exists kept0 log0 kept' rej' log',
    walk (resort cands) [] [] = (kept0, log0) /\
    ordered_pass (S (length (concat cands))) cands rej0 = Some (kept', rej', log') /\
    pairwise_cf kept0 /\ no_cross_mix kept0 /\ ledger_ok (concat kept0) = true /\
    incl kept0 (resort cands) /\ NoDup (concat kept0) /\
    repair_post kept0 (rej0 ++ concat (map fst log0)) log0 kept' rej' log' /\
    (forall c w, In (c, w) log0 -> exists pre post, resort cands = pre ++ c :: post /\ walk_ok pre c w) /\
    (forall c, In c (resort cands) -> In c kept0 \/ exists w, In (c, w) log0) /\
    (forall c w x, In (c, w) log0 -> In x c -> ~ In x (concat kept0)).
Proof.
  intros cands rej0 Hne Hnd.
  assert (Hp : Permutation (resort cands) cands) by apply resort_perm.
  assert (Hndr : NoDup (concat (resort cands)))
    by (apply (Permutation_NoDup (Permutation_sym (perm_concat _ _ Hp))); exact Hnd).
  destruct (walk (resort cands) [] []) as [kept0 log0] eqn:Hw.
  assert (Hcf_nil : pairwise_cf []) by (intros a b Ha; contradiction).
  assert (Hmx_nil : no_cross_mix []) by (intros a b Ha; contradiction).
  destruct (walk_inv _ _ _ _ _ Hw Hcf_nil Hmx_nil ledger_ok_nil) as [Hcf0 [Hmx0 Hl0]].
  destruct (walk_prefix _ _ _ _ _ Hw) as [added [Hk0 Hadded]]. simpl in Hk0. subst kept0.
  destruct (walk_nodup _ _ _ _ _ Hw Hndr) as [Hnd0 Hlen0].
  destruct (walk_log _ _ _ _ _ Hw) as [new0 [Hlog0 [Hpos0 [Hpart0 Hdis0]]]]. simpl in Hlog0. subst log0.
  assert (Hne0 : all_nonempty added).
  { intros c Hc. apply Hne. exact (Permutation_in c Hp (Hadded c Hc)). }
  assert (Hlen0' : length (concat added) < S (length (concat cands))).
  { simpl in Hlen0. rewrite (Permutation_length (perm_concat _ _ Hp)) in Hlen0. lia. }
  destruct (repair_spec (S (length (concat cands))) false added (rej0 ++ concat (map fst new0)) new0
              Hlen0' Hne0 Hnd0 (fun _ => Hcf0)) as [kept' [rej' [log' [Hrun Hpost]]]].
  exists added, new0, kept', rej', log'.
  split; [reflexivity |].
  split; [unfold ordered_pass; rewrite Hw; exact Hrun |].
  split; [exact Hcf0 |]. split; [exact Hmx0 |]. split; [exact Hl0 |].
  split; [exact Hadded |]. split; [exact Hnd0 |]. split; [exact Hpost |].
  split; [exact Hpos0 |]. split; [exact Hpart0 |].
  intros c w x Hin Hx. exact (Hdis0 Hndr c w x Hin Hx).
Qed.

(** S5 and S6 together, for all theorems below. *)
Lemma ordered_pass_valid : forall cands rej0,
  all_nonempty cands -> NoDup (concat cands) ->
  exists kept' rej' log',
    ordered_pass (S (length (concat cands))) cands rej0 = Some (kept', rej', log') /\
    pairwise_cf kept' /\ no_cross_mix kept' /\ overfill kept' = None /\
    balance (concat kept') = None /\ ledger_ok (concat kept') = true /\
    lineage_closed kept' rej' /\ all_nonempty kept' /\ NoDup (concat kept') /\
    incl (concat kept') (concat cands).
Proof.
  intros cands rej0 Hne Hnd.
  destruct (pass_spec cands rej0 Hne Hnd) as
      [kept0 [log0 [kept' [rej' [log' [Hw [Hrun [Hcf0 [Hmx0 [Hl0 [Hinc0 [Hnd0 [Hpost _]]]]]]]]]]]]].
  destruct Hpost as [Pcf [Pov [Pbal [Pcl [Pne [Pnd [Pincl [Pant [Puna _]]]]]]]]].
  exists kept', rej', log'. split; [exact Hrun |].
  split; [exact Pcf |]. split; [exact (Pant _ cross_mix_antitone Hmx0) |].
  split; [exact Pov |]. split; [exact Pbal |].
  split; [apply (ledger_ok_incl (concat kept0)); assumption |].
  split; [exact Pcl |]. split; [exact Pne |]. split; [exact Pnd |].
  intros x Hx. apply Pincl in Hx. apply in_concat in Hx as [c [Hc Hxc]].
  apply in_concat. exists c. split; [| exact Hxc].
  apply (Permutation_in c (resort_perm cands)). apply Hinc0. exact Hc.
Qed.

(** ** Main theorems *)

Theorem survivors_pairwise_conflict_free : forall cands rej0,
  all_nonempty cands -> NoDup (concat cands) ->
  exists kept' rej' log',
    ordered_pass (S (length (concat cands))) cands rej0 = Some (kept', rej', log') /\
    pairwise_cf kept'.
Proof.
  intros cands rej0 Hne Hnd.
  destruct (ordered_pass_valid cands rej0 Hne Hnd) as [kept' [rej' [log' [Hrun [Hcf _]]]]].
  exists kept', rej', log'. split; assumption.
Qed.

(** MEDIUM-1: after per-chain lineage removal shrinks a candidate, the
    conflict re-check restores conflict freedom before the loop returns, even
    when the input set has conflicts. *)
Theorem lineage_recheck_restores_conflict_freedom : forall fuel kept rej log,
  length (concat kept) < fuel -> all_nonempty kept -> NoDup (concat kept) ->
  exists kept' rej' log', repair fuel true kept rej log = Some (kept', rej', log') /\
    pairwise_cf kept'.
Proof.
  intros fuel kept rej log Hlen Hne Hnd.
  assert (Hvac : true = false -> pairwise_cf kept) by (intros H; discriminate H).
  destruct (repair_spec fuel true kept rej log Hlen Hne Hnd Hvac) as [kept' [rej' [log' [Hrun [Hcf _]]]]].
  exists kept', rej', log'. split; assumption.
Qed.

(** The repair loop ends within [fuel] rounds with a valid set: no conflict,
    no folded mixing, no overfill, every balance in range, every sign split
    in range, and lineage closed. *)
Theorem repair_loop_terminates_valid : forall fuel kept rej log,
  length (concat kept) < fuel -> all_nonempty kept -> NoDup (concat kept) ->
  pairwise_cf kept -> no_cross_mix kept -> ledger_ok (concat kept) = true ->
  exists kept' rej' log', repair fuel false kept rej log = Some (kept', rej', log') /\
    pairwise_cf kept' /\ no_cross_mix kept' /\ overfill kept' = None /\
    balance (concat kept') = None /\ ledger_ok (concat kept') = true /\ lineage_closed kept' rej'.
Proof.
  intros fuel kept rej log Hlen Hne Hnd Hcf Hmx Hl.
  destruct (repair_spec fuel false kept rej log Hlen Hne Hnd (fun _ => Hcf)) as
      [kept' [rej' [log' [Hrun Hpost]]]].
  destruct Hpost as [Pcf [Pov [Pbal [Pcl [Pne [Pnd [Pincl [Pant _]]]]]]]].
  exists kept', rej', log'. split; [exact Hrun |].
  split; [exact Pcf |]. split; [exact (Pant _ cross_mix_antitone Hmx) |].
  split; [exact Pov |]. split; [exact Pbal |].
  split; [apply (ledger_ok_incl (concat kept)); assumption | exact Pcl].
Qed.

Theorem survivors_free_of_base_and_pinned_conflicts : forall cands rej0,
  all_nonempty cands -> NoDup (concat cands) ->
  (forall x, In x (concat cands) -> ~ In x base_conflicting /\ ~ In x settled_conflicting) ->
  exists kept' rej' log',
    ordered_pass (S (length (concat cands))) cands rej0 = Some (kept', rej', log') /\
    (forall x, In x (concat kept') -> ~ In x base_conflicting /\ ~ In x settled_conflicting) /\
    (forall x y, In x (concat kept') -> pinned x = false ->
       In y (base_conflicting ++ settled_conflicting) -> anc (src y) (src x) = false) /\
    (forall a b, In a kept' -> In b kept' -> a <> b -> pinned_cand a = true -> conflict a b = false).
Proof.
  intros cands rej0 Hne Hnd Hdisj.
  destruct (ordered_pass_valid cands rej0 Hne Hnd) as
      [kept' [rej' [log' [Hrun [Hcf [_ [_ [_ [_ [Hcl [_ [_ Hincl]]]]]]]]]]]].
  exists kept', rej', log'. split; [exact Hrun |]. split.
  - intros x Hx. apply Hdisj. apply Hincl. exact Hx.
  - split.
    + intros x y Hx Hp Hy. pose proof (Hcl x Hx) as Hs. unfold stale in Hs. rewrite Hp in Hs.
      change (existsb (fun r => anc r (src x)) (rejected_blocks rej') = false) in Hs.
      destruct (anc (src y) (src x)) eqn:Hanc; [| reflexivity].
      assert (Hex : existsb (fun r => anc r (src x)) (rejected_blocks rej') = true).
      { apply existsb_exists. exists (src y). split; [| exact Hanc].
        unfold rejected_blocks. apply in_map. apply in_or_app. right. exact Hy. }
      congruence.
    + intros a b Ha Hb Hab _. exact (Hcf a b Ha Hb Hab).
Qed.

Theorem survivors_no_folded_mixing : forall cands rej0,
  all_nonempty cands -> NoDup (concat cands) ->
  exists kept' rej' log',
    ordered_pass (S (length (concat cands))) cands rej0 = Some (kept', rej', log') /\
    no_cross_mix kept'.
Proof.
  intros cands rej0 Hne Hnd.
  destruct (ordered_pass_valid cands rej0 Hne Hnd) as [kept' [rej' [log' [Hrun [_ [Hmx _]]]]]].
  exists kept', rej', log'. split; assumption.
Qed.

Theorem survivors_no_overfill : forall cands rej0,
  all_nonempty cands -> NoDup (concat cands) ->
  exists kept' rej' log',
    ordered_pass (S (length (concat cands))) cands rej0 = Some (kept', rej', log') /\
    overfill kept' = None /\ balance (concat kept') = None /\ ledger_ok (concat kept') = true.
Proof.
  intros cands rej0 Hne Hnd.
  destruct (ordered_pass_valid cands rej0 Hne Hnd) as
      [kept' [rej' [log' [Hrun [_ [_ [Hov [Hbal [Hl _]]]]]]]]].
  exists kept', rej', log'. split; [exact Hrun | split; [exact Hov | split; assumption]].
Qed.

Lemma nodup_ids_concat : forall (ids : nat -> list nat) l,
  NoDup (concat l) -> all_nonempty l ->
  pairwise (fun a b => forall i, In i (flat_map ids a) -> ~ In i (flat_map ids b)) l ->
  (forall c, In c l -> NoDup (flat_map ids c)) ->
  NoDup (flat_map ids (concat l)).
Proof.
  intros ids l. induction l as [| c l IH]; intros Hnd Hne Hpw Heach; simpl; [constructor |].
  rewrite flat_map_app. apply NoDup_app.
  - apply Heach. left. reflexivity.
  - apply IH.
    + exact (NoDup_app_remove_l _ _ Hnd).
    + intros d Hd. apply Hne. right. exact Hd.
    + apply (pairwise_incl _ (c :: l)); [exact Hpw |]. intros d Hd. right. exact Hd.
    + intros d Hd. apply Heach. right. exact Hd.
  - intros i Hi Hi'.
    apply in_flat_map in Hi' as [y [Hy Hiy]]. apply in_concat in Hy as [d [Hd Hyd]].
    assert (Hcd : c <> d).
    { intros Heq. subst d. destruct c as [| z c']; [exact (Hne [] (or_introl eq_refl) eq_refl) |].
      apply (nodup_app_disjoint _ (z :: c') (concat l) Hnd z); [left; reflexivity |].
      apply in_concat. exists (z :: c'). split; [exact Hd | left; reflexivity]. }
    apply (Hpw c d (or_introl eq_refl) (or_intror Hd) Hcd i Hi).
    apply in_flat_map. exists y. split; assumption.
Qed.

(** C2: every user deploy id survives at most once. Two candidates that
    share a user deploy id conflict: dev's conflict map adds a
    same-user-deploy-id pass to the event-log conflicts
    (dag_merger.rs:1732-1760). Inside one candidate, the premise that the
    chains carry distinct ids is the availability walk's result: two copies
    consume one fee-cursor datum, so the walk rejects the second
    (CursorLinearity.first_use_copies_collide). *)
Theorem survivor_ids_exactly_once : forall (ids : nat -> list nat) cands rej0,
  all_nonempty cands -> NoDup (concat cands) ->
  (forall c, In c cands -> NoDup (flat_map ids c)) ->
  (forall a b, In a cands -> In b cands -> a <> b ->
     (exists i, In i (flat_map ids a) /\ In i (flat_map ids b)) -> conflict a b = true) ->
  exists kept' rej' log',
    ordered_pass (S (length (concat cands))) cands rej0 = Some (kept', rej', log') /\
    NoDup (flat_map ids (concat kept')).
Proof.
  intros ids cands rej0 Hne Hnd Heach Hshare.
  destruct (pass_spec cands rej0 Hne Hnd) as
      [kept0 [log0 [kept' [rej' [log' [Hw [Hrun [Hcf0 [Hmx0 [Hl0 [Hinc0 [Hnd0 [Hpost _]]]]]]]]]]]]].
  destruct Hpost as [Pcf [Pov [Pbal [Pcl [Pne [Pnd [Pincl [Pant [Puna _]]]]]]]]].
  set (R := fun a b : list nat => forall i, In i (flat_map ids a) -> ~ In i (flat_map ids b)).
  assert (HR : antitone R).
  { intros a b a' b' H Ha Hb i Hi Hi'. apply (H i).
    - apply (flat_map_incl ids a a' Ha). exact Hi.
    - apply (flat_map_incl ids b b' Hb). exact Hi'. }
  assert (Hin0 : forall c, In c kept0 -> In c cands)
    by (intros c Hc; exact (Permutation_in c (resort_perm cands) (Hinc0 c Hc))).
  assert (HR0 : pairwise R kept0).
  { intros a b Ha Hb Hab i Hi Hi'.
    assert (Hc : conflict a b = true) by (apply Hshare; [apply Hin0; exact Ha | apply Hin0; exact Hb |
                                                       exact Hab | exists i; split; assumption]).
    rewrite (Hcf0 a b Ha Hb Hab) in Hc. discriminate. }
  set (U := fun c : list nat => NoDup (flat_map ids c)).
  assert (HU : filter_closed U).
  { intros c q Hc. unfold U in *. induction c as [| x c IHc]; simpl; [constructor |].
    simpl in Hc. destruct (q x); simpl.
    - apply NoDup_app.
      + exact (NoDup_app_remove_r _ _ Hc).
      + apply IHc. exact (NoDup_app_remove_l _ _ Hc).
      + intros i Hi Hi'. apply (nodup_app_disjoint _ _ _ Hc i Hi).
        apply in_flat_map in Hi' as [y [Hy Hiy]]. apply filter_In in Hy as [Hy _].
        apply in_flat_map. exists y. split; assumption.
    - apply IHc. exact (NoDup_app_remove_l _ _ Hc). }
  exists kept', rej', log'. split; [exact Hrun |].
  apply nodup_ids_concat; [exact Pnd | exact Pne | exact (Pant R HR HR0) |].
  apply (Puna U HU). intros c Hc. apply Heach. apply Hin0. exact Hc.
Qed.

(** L8: a pinned candidate is dropped only by the pre-check or its own
    unavailability (S3, outside the model, the chains of [rej0]), by a check
    against earlier candidates that are all pinned (S5), by a conflict re-check
    against an earlier pinned candidate (S6.2), or by the repair loop when no
    unpinned kept candidate could be dropped instead (S6.3, S6.4). *)
Theorem pinned_kept_unless_forced : forall cands rej0,
  all_nonempty cands -> NoDup (concat cands) ->
  exists kept' rej' log',
    ordered_pass (S (length (concat cands))) cands rej0 = Some (kept', rej', log') /\
    (forall c w, In (c, w) log' -> pinned_cand c = true -> pinned_loss_allowed w).
Proof.
  intros cands rej0 Hne Hnd.
  destruct (pass_spec cands rej0 Hne Hnd) as
      [kept0 [log0 [kept' [rej' [log' [Hw [Hrun [_ [_ [_ [_ [_ [Hpost [Hpos _]]]]]]]]]]]]]].
  destruct Hpost as [_ [_ [_ [_ [_ [_ [_ [_ [_ Prest]]]]]]]]].
  destruct Prest as [rnew [new [_ [Hlog [Hok _]]]]].
  exists kept', rej', log'. split; [exact Hrun |].
  intros c w Hin Hpc. rewrite Hlog in Hin. apply in_app_or in Hin as [Hin | Hin].
  - destruct (Hpos c w Hin) as [pre [post [Hsplit Hok']]].
    assert (Hs : StronglySorted pfirst (pre ++ c :: post)) by (rewrite <- Hsplit; apply resort_pinned_first).
    pose proof (sorted_prefix_pinned pre c post Hs Hpc) as Hpre.
    destruct w; simpl in Hok' |- *; try contradiction.
    + destruct Hok' as [Hk _]. exact (Hpre k Hk).
    + destruct Hok' as [HK _]. apply Forall_forall. intros k Hk. exact (Hpre k (HK k Hk)).
    + destruct Hok' as [HK _]. apply Forall_forall. intros k Hk. exact (Hpre k (HK k Hk)).
  - pose proof (Hok c w Hin) as Hr. destruct w; simpl in Hr |- *; try contradiction.
    + destruct Hr as [x [Hc [Hp _]]]. subst c. unfold pinned_cand in Hpc. simpl in Hpc.
      rewrite Hp in Hpc. discriminate Hpc.
    + destruct Hr as [_ [Hpf | Hpf]]; [exact Hpf | rewrite Hpc in Hpf; discriminate].
    + destruct Hr as [_ [_ [_ Hv]]]. exact (last_by_k_forced _ _ _ Hv Hpc).
    + destruct Hr as [_ [_ Hv]]. exact (balance_victim_forced _ _ _ _ Hv Hpc).
Qed.

(** L9: the first unpinned candidate in K never loses in S5 to a later
    candidate. Every S5 rejection of it names only earlier candidates, and
    every candidate before it is pinned. Its other losses are the listed
    exceptions: the pre-check, unavailability, and S6. *)
Theorem first_unpinned_in_k_not_lost_to_later : forall cands pre u post,
  all_nonempty cands -> NoDup (concat cands) ->
  resort cands = pre ++ u :: post -> pinned_cand u = false ->
  Forall (fun p => pinned_cand p = true) pre ->
  forall kept0 log0, walk (resort cands) [] [] = (kept0, log0) ->
  forall w, In (u, w) log0 -> names_only_pinned w.
Proof.
  intros cands pre u post Hne Hnd Hsplit Hu Hpre kept0 log0 Hw w Hin.
  destruct (walk_log _ _ _ _ _ Hw) as [new [Hlog [Hpos _]]]. simpl in Hlog. subst log0.
  destruct (Hpos u w Hin) as [pre' [post' [Hsplit' Hok]]]. simpl in Hok.
  assert (Hndr : NoDup (resort cands)).
  { apply nodup_outer.
    - apply (Permutation_NoDup (Permutation_sym (perm_concat _ _ (resort_perm cands)))). exact Hnd.
    - intros c Hc. apply Hne. exact (Permutation_in c (resort_perm cands) Hc). }
  assert (Heq : pre = pre') by exact (nodup_split_unique _ _ _ _ _ _ Hndr Hsplit Hsplit').
  subst pre'. rewrite Forall_forall in Hpre.
  destruct w; simpl in Hok |- *; try contradiction.
  - destruct Hok as [Hk _]. exact (Hpre k Hk).
  - destruct Hok as [HK _]. apply Forall_forall. intros k Hk. exact (Hpre k (HK k Hk)).
  - destruct Hok as [HK _]. apply Forall_forall. intros k Hk. exact (Hpre k (HK k Hk)).
Qed.

(** Every rejection carries a witness that held when the pass recorded it,
    every chain of the candidates is kept or rejected, and no kept chain is
    rejected. *)
Theorem rejection_has_witness : forall cands rej0,
  all_nonempty cands -> NoDup (concat cands) ->
  exists kept' rej' log',
    ordered_pass (S (length (concat cands))) cands rej0 = Some (kept', rej', log') /\
    (forall c w, In (c, w) log' -> holds rej' c w) /\
    (forall x, In x (concat cands) -> In x (concat kept') \/ exists c w, In (c, w) log' /\ In x c) /\
    (forall x c w, In (c, w) log' -> In x c -> ~ In x (concat kept')) /\
    (forall x, In x rej' -> In x rej0 \/ exists c w, In (c, w) log' /\ In x c).
Proof.
  intros cands rej0 Hne Hnd.
  destruct (pass_spec cands rej0 Hne Hnd) as
      [kept0 [log0 [kept' [rej' [log' [Hw [Hrun [_ [_ [_ [Hinc0 [_ [Hpost [Hpos [Hpart Hdis0]]]]]]]]]]]]]]].
  destruct Hpost as [_ [_ [_ [_ [_ [_ [Pincl [_ [_ Prest]]]]]]]]].
  destruct Prest as [rnew [new [Hrej [Hlog [Hok [Hcov [Hdis Hrn]]]]]]].
  exists kept', rej', log'. split; [exact Hrun |]. split.
  - intros c w Hin. rewrite Hlog in Hin. apply in_app_or in Hin as [Hin | Hin].
    + destruct (Hpos c w Hin) as [pre [post [_ Hok']]].
      destruct w; simpl in Hok' |- *; try contradiction.
      * exact (proj2 Hok').
      * exact (proj2 Hok').
      * exact (proj2 Hok').
    + pose proof (Hok c w Hin) as Hr. destruct w; simpl in Hr |- *; try contradiction.
      * exact Hr.
      * exact (proj1 Hr).
      * destruct Hr as [H1 [H2 [H3 _]]]. split; [exact H1 | split; assumption].
      * exact Hr.
  - split.
    + intros x Hx. apply in_concat in Hx as [c [Hc Hxc]].
      assert (Hcr : In c (resort cands))
        by exact (Permutation_in c (Permutation_sym (resort_perm cands)) Hc).
      destruct (Hpart c Hcr) as [Hk | [w Hcw]].
      * assert (Hx0 : In x (concat kept0)) by (apply in_concat; exists c; split; assumption).
        destruct (Hcov x Hx0) as [Hk' | [c' [w' [Hin Hxc']]]]; [left; exact Hk' |].
        right. exists c', w'. split; [rewrite Hlog; apply in_or_app; right; exact Hin | exact Hxc'].
      * right. exists c, w. split; [rewrite Hlog; apply in_or_app; left; exact Hcw | exact Hxc].
    + split.
      * intros x c w Hin Hx Hk. rewrite Hlog in Hin. apply in_app_or in Hin as [Hin | Hin].
        -- apply (Hdis0 c w x Hin Hx). apply Pincl. exact Hk.
        -- exact (Hdis x c w Hin Hx Hk).
      * intros x Hx. rewrite Hrej in Hx. apply in_app_or in Hx as [Hx | Hx].
        -- apply in_app_or in Hx as [Hx | Hx]; [left; exact Hx |].
           right. apply in_concat in Hx as [c [Hc Hxc]]. apply in_map_iff in Hc as [[c' w] [Hc' Hin]].
           simpl in Hc'. subst c'. exists c, w. split; [rewrite Hlog; apply in_or_app; left; exact Hin | exact Hxc].
        -- right. destruct (Hrn x Hx) as [c [w [Hin Hxc]]].
           exists c, w. split; [rewrite Hlog; apply in_or_app; right; exact Hin | exact Hxc].
Qed.

(** *** L5: the fast path equals the ordered pass *)

Lemma walk_keeps_all : forall cands kept log,
  NoDup (concat (kept ++ cands)) -> all_nonempty (kept ++ cands) ->
  pairwise_cf (kept ++ cands) -> no_cross_mix (kept ++ cands) ->
  ledger_ok (concat (kept ++ cands)) = true ->
  walk cands kept log = (kept ++ cands, log).
Proof.
  induction cands as [| c rest IH]; intros kept log Hnd Hne Hcf Hmx Hl; simpl.
  - rewrite app_nil_r. reflexivity.
  - assert (Hc : In c (kept ++ c :: rest)) by (apply in_or_app; right; left; reflexivity).
    assert (Hdist : forall k, In k kept -> k <> c).
    { intros k Hk Heq. subst k.
      destruct c as [| z c']; [exact (Hne [] Hc eq_refl) |].
      rewrite concat_app in Hnd. simpl in Hnd.
      apply (nodup_app_disjoint _ (concat kept) ((z :: c') ++ concat rest) Hnd z).
      - apply in_concat. exists (z :: c'). split; [exact Hk | left; reflexivity].
      - left. reflexivity. }
    assert (Hf : find (conflict c) kept = None).
    { apply find_none_intro. intros k Hk. apply Hcf; [exact Hc | apply in_or_app; left; exact Hk |].
      intros Heq. exact (Hdist k Hk (eq_sym Heq)). }
    assert (Hm : mixes kept c = false).
    { apply existsb_false_intro. intros k Hk. apply Hmx; [exact Hc | apply in_or_app; left; exact Hk |].
      intros Heq. exact (Hdist k Hk (eq_sym Heq)). }
    assert (Ht : try_add kept c = true).
    { unfold try_add. apply (ledger_ok_incl (concat (kept ++ c :: rest))); [exact Hl | |].
      - intros x Hx. rewrite concat_app. simpl. apply in_app_or in Hx as [Hx | Hx];
          apply in_or_app; [left; exact Hx | right; apply in_or_app; left; exact Hx].
      - rewrite concat_app in Hnd. simpl in Hnd. rewrite app_assoc in Hnd.
        exact (NoDup_app_remove_r _ _ Hnd). }
    rewrite Hf, Hm, Ht.
    assert (Heq : (kept ++ [c]) ++ rest = kept ++ c :: rest) by (rewrite <- app_assoc; reflexivity).
    rewrite <- Heq. apply IH; rewrite Heq; assumption.
Qed.

(** L5: when the fast-path predicate P holds, the ordered pass keeps every
    candidate and rejects nothing. The fast path composes every chain, and
    compose folds in canonical order (compose.rs), so both paths reach the
    same state. Under P: F6 and F7 leave no late and no base-conflicting
    chain, and [chain_level_p5_empties_settled_partition] leaves no
    settled-conflicting chain, so [rej0], [base_conflicting] and
    [settled_conflicting] are empty; F10 and F12 give no conflict and no
    cross mixing; F12 gives no overfill; F13 gives a valid ledger and valid
    balances; F11 makes the candidates equal the branches. *)
Theorem fast_path_equals_ordered_pass : forall cands,
  all_nonempty cands -> NoDup (concat cands) ->
  pairwise_cf cands -> no_cross_mix cands -> ledger_ok (concat cands) = true ->
  overfill cands = None -> balance (concat cands) = None ->
  base_conflicting = [] -> settled_conflicting = [] ->
  exists kept', ordered_pass (S (length (concat cands))) cands [] = Some (kept', [], []) /\
    Permutation kept' cands.
Proof.
  intros cands Hne Hnd Hcf Hmx Hl Hov Hbal Hbc Hsc.
  assert (Hp : Permutation (resort cands) cands) by apply resort_perm.
  assert (Hcp : Permutation (concat (resort cands)) (concat cands)) by (apply perm_concat; exact Hp).
  assert (Hin : forall c, In c (resort cands) -> In c cands) by (intros c Hc; exact (Permutation_in c Hp Hc)).
  assert (Hw : walk (resort cands) [] [] = ([] ++ resort cands, [])).
  { apply walk_keeps_all; simpl.
    - apply (Permutation_NoDup (Permutation_sym Hcp)). exact Hnd.
    - intros c Hc. apply Hne. apply Hin. exact Hc.
    - apply (pairwise_incl _ cands); [exact Hcf | exact Hin].
    - apply (pairwise_incl _ cands); [exact Hmx | exact Hin].
    - apply (ledger_ok_incl (concat cands)); [exact Hl | |].
      + intros x Hx. exact (Permutation_in x Hcp Hx).
      + apply (Permutation_NoDup (Permutation_sym Hcp)). exact Hnd. }
  assert (Hsc0 : stale_chains (resort cands) [] = []).
  { unfold stale_chains. apply filter_all_false. intros x _.
    unfold stale, rejected_blocks. rewrite Hbc, Hsc. simpl. apply andb_false_r. }
  assert (Hfk : fresh_kept (resort cands) [] = resort cands).
  { apply fresh_kept_unchanged.
    - intros c Hc. apply Hne. apply Hin. exact Hc.
    - rewrite Hsc0. reflexivity. }
  assert (Hpp : Permutation (resort (resort cands)) cands)
    by (transitivity (resort cands); apply resort_perm).
  exists (resort (resort cands)). split; [| exact Hpp].
  unfold ordered_pass. rewrite Hw.
  change (repair (S (length (concat cands))) false (resort cands) [] []
          = Some (resort (resort cands), [], [])).
  cbn [repair]. rewrite Hsc0, Hfk. simpl.
  rewrite (overfill_perm _ _ Hpp), Hov. simpl.
  rewrite (balance_perm _ _ (perm_concat _ _ Hpp)), Hbal. simpl.
  unfold stale_log. rewrite Hsc0. reflexivity.
Qed.

End Pass.

(** [sort_by_k] (V6MergeOrder) satisfies the two Section hypotheses on
    [resort]: it permutes its input, and it puts every pinned candidate
    first. *)
Lemma sort_by_k_perm : forall pinned loss height l,
  Permutation (sort_by_k pinned loss height l) l.
Proof.
  intros pinned loss height l. unfold sort_by_k. symmetry. apply isort_perm.
Qed.

Lemma sort_by_k_pinned_first : forall pinned loss height,
  (forall c, (0 <= loss c)%Z) ->
  forall l, StronglySorted (pfirst pinned) (sort_by_k pinned loss height l).
Proof.
  intros pinned loss height Hl l.
  destruct (k_strict_total pinned loss height Hl) as [Hanti [Htrans _]].
  assert (Hle_trans : forall a b c, le_by (k_cmp pinned loss height) a b ->
                      le_by (k_cmp pinned loss height) b c -> le_by (k_cmp pinned loss height) a c).
  { apply (le_by_trans _ _ Hanti (k_cmp_eq_subst pinned loss height) Htrans). }
  assert (Hss : StronglySorted (le_by (k_cmp pinned loss height)) (sort_by_k pinned loss height l)).
  { apply Sorted_StronglySorted; [exact Hle_trans |]. unfold sort_by_k.
    apply isort_sorted. exact Hanti. }
  apply (StronglySorted_impl _ (le_by (k_cmp pinned loss height))); [| exact Hss].
  intros a b Hab. unfold pfirst, pinned_cand.
  destruct (existsb pinned a) eqn:Ha; [left; reflexivity |].
  destruct (existsb pinned b) eqn:Hb; [| right; reflexivity].
  exfalso. apply Hab. rewrite Hanti.
  rewrite (k_pinned_first pinned loss height b a Hb Ha). reflexivity.
Qed.

(** ** The overfill dry run (LOW-3) *)

(** A datum is a natural number. A channel change is a pair (added,
    removed) of datum lists (channel_change.rs:3-7). *)
Fixpoint remove_one (x : nat) (l : list nat) : list nat :=
  match l with
  | [] => []
  | y :: r => if Nat.eqb x y then r else y :: remove_one x r
  end.

(** [vec_diff] (channel_change.rs:73-81) removes one copy of each element of
    [to_remove]. [StateChange::multiset_diff] removes the same multiset. *)
Definition vec_diff (from to_remove : list nat) : list nat :=
  fold_left (fun acc x => remove_one x acc) to_remove from.

(** [vec_union] (channel_change.rs:53-58). *)
Definition vec_union (l r : list nat) : list nat := l ++ vec_diff r l.

(** [cancel_common] (channel_change.rs:60-71). *)
Fixpoint cancel_common (added removed : list nat) : list nat * list nat :=
  match added with
  | [] => ([], removed)
  | x :: rest =>
      if existsb (Nat.eqb x) removed
      then cancel_common rest (remove_one x removed)
      else let '(a, r) := cancel_common rest removed in (x :: a, r)
  end.

Definition join_change (x y : list nat * list nat) : list nat * list nat :=
  (vec_union (fst x) (fst y), vec_union (snd x) (snd y)).

(** [ChannelChange::combine] (channel_change.rs:17-20): join, then
    normalize. *)
Definition combine_change (x y : list nat * list nat) : list nat * list nat :=
  let '(a, r) := join_change x y in cancel_common a r.

(** [StateChange::combine] on one channel (state_change.rs:455-500): a
    channel that one side does not touch keeps the other side's change
    as it is; two changes combine. [None] is an item that does not touch the
    channel. *)
Definition step_change (acc x : option (list nat * list nat)) : option (list nat * list nat) :=
  match acc, x with
  | None, x => x
  | Some a, None => Some a
  | Some a, Some b => Some (combine_change a b)
  end.

(** compose (conflict_set_merger.rs:458-468) flattens the sorted branches
    into one item list and folds it. The dry run (compose.rs
    [first_overfill]) folds branch by branch, item by item, in the same
    canonical order. *)
Definition compose_change (branches : list (list (option (list nat * list nat)))) :=
  fold_left step_change (concat branches) None.
Definition dry_run_change (branches : list (list (option (list nat * list nat)))) :=
  fold_left (fun acc b => fold_left step_change b acc) branches None.

(** [check_single_value_cell_not_overfilled] (rholang_merging_logic.rs:237-267)
    behind the base and multi-writer test of [guarded_channel_action]
    (compose.rs). [num] says whether a datum is a number. *)
Definition guard_fails (num : nat -> bool) (base : list nat) (multi : bool)
  (chg : option (list nat * list nat)) : bool :=
  match chg with
  | None => false
  | Some (added, removed) =>
      nonempty added && (nonempty base || multi) &&
      ((Nat.eqb (length base) 1 && forallb num base) ||
       (negb (nonempty base) && forallb num added)) &&
      (1 <? length (vec_diff base removed) + length added)
  end.

(** One channel of the kept set, in channel order: its base values, the
    multi-writer flag, the number-channel flag (a folded diff takes the
    number override instead of the guard), and the sorted branches of item
    changes. *)
Record channel_input : Type := mk_channel {
  ci_id : nat;
  ci_base : list nat;
  ci_multi : bool;
  ci_folded : bool;
  ci_items : list (list (option (list nat * list nat)))
}.

Definition channel_fails (num : nat -> bool)
  (fold : list (list (option (list nat * list nat))) -> option (list nat * list nat))
  (c : channel_input) : bool :=
  negb (ci_folded c) && guard_fails num (ci_base c) (ci_multi c) (fold (ci_items c)).

(** The dry run returns the first failing channel; compose fails when any
    channel fails its guard. *)
Definition first_overfill_model (num : nat -> bool) (cs : list channel_input) : option nat :=
  match find (channel_fails num dry_run_change) cs with
  | Some c => Some (ci_id c)
  | None => None
  end.
Definition compose_fails (num : nat -> bool) (cs : list channel_input) : bool :=
  existsb (channel_fails num compose_change) cs.

Lemma fold_left_concat : forall (A B : Type) (f : A -> B -> A) (ll : list (list B)) (a : A),
  fold_left f (concat ll) a = fold_left (fun acc l => fold_left f l acc) ll a.
Proof.
  intros A B f ll. induction ll as [| l ll IH]; intros a; simpl; [reflexivity |].
  rewrite fold_left_app. apply IH.
Qed.

Lemma dry_run_is_compose : forall branches, dry_run_change branches = compose_change branches.
Proof.
  intros branches. unfold dry_run_change, compose_change. symmetry. apply fold_left_concat.
Qed.

(** LOW-3: the dry run folds the kept changes exactly as compose does, so it
    flags a channel exactly when compose's guard fails on it. *)
Theorem overfill_check_matches_guard : forall num cs,
  (forall c, channel_fails num dry_run_change c = channel_fails num compose_change c) /\
  (first_overfill_model num cs = None <-> compose_fails num cs = false) /\
  (forall ch, first_overfill_model num cs = Some ch ->
     exists c, In c cs /\ ci_id c = ch /\ channel_fails num compose_change c = true).
Proof.
  intros num cs.
  assert (Hsame : forall c, channel_fails num dry_run_change c = channel_fails num compose_change c).
  { intros c. unfold channel_fails. rewrite dry_run_is_compose. reflexivity. }
  split; [exact Hsame |]. split.
  - unfold first_overfill_model, compose_fails. split.
    + intros H. destruct (find (channel_fails num dry_run_change) cs) eqn:Hf; [discriminate |].
      apply existsb_false_intro. intros c Hc. rewrite <- Hsame. exact (find_none _ _ Hf c Hc).
    + intros H. rewrite (find_none_intro _ _ cs); [reflexivity |].
      intros c Hc. rewrite Hsame. exact (existsb_false _ _ _ H c Hc).
  - intros ch H. unfold first_overfill_model in H.
    destruct (find (channel_fails num dry_run_change) cs) as [c |] eqn:Hf; [| discriminate].
    injection H as Hch. apply find_some in Hf as [Hin Hfail].
    exists c. split; [exact Hin | split; [exact Hch | rewrite <- Hsame; exact Hfail]].
Qed.

(** A fold step without normalization: [join] where compose uses
    [combine]. *)
Definition step_unnormalized (acc x : option (list nat * list nat)) : option (list nat * list nat) :=
  match acc, x with
  | None, x => x
  | Some a, None => Some a
  | Some a, Some b => Some (join_change a b)
  end.

(** Negative control (LOW-3): one branch produces datum 5 on a cell whose
    base holds the number 9, and then consumes it. Compose's normalized fold
    cancels the pair, so the guard passes. A fold without normalization keeps
    an add and flags a second value. *)
Theorem nc_unnormalized_overfill_disagrees_with_guard :
  let num := fun _ : nat => true in
  let items := [[Some ([5], []); Some ([], [5])]] in
  guard_fails num [9] false (compose_change items) = false /\
  guard_fails num [9] false (fold_left (fun acc b => fold_left step_unnormalized b acc) items None) = true.
Proof. vm_compute. split; reflexivity. Qed.

(** ** P5': the chain-level conflict map *)

Section Settled.

(** dev's conflict test (merging_logic.rs:262-470) has three parts: races
    on one IO event and potential COMMs between two event logs ([rc] for two
    chains, [rlog] for a chain and a combined log, [rlogs] for two combined
    logs), and produces that touch base joins, which conflict with any log
    ([touch]). A race or potential COMM with a combined log is one with one
    of its chains, because the combined sets are unions and a destroyed event
    of the union is destroyed in each part (event_log_index.rs combine). *)
Variable touch : nat -> bool.
Variable rc : nat -> nat -> bool.
Variable rlog : nat -> list nat -> bool.
Variable rlogs : list nat -> list nat -> bool.
Hypothesis rlog_lifts : forall c l, rlog c l = true -> exists s, In s l /\ rc c s = true.
Hypothesis rlogs_lifts :
  forall A B, rlogs A B = true -> exists a b, In a A /\ In b B /\ rc a b = true.

Definition chain_conflict (a b : nat) : bool := rc a b || touch a || touch b.
Definition log_conflict (c : nat) (l : list nat) : bool := rlog c l || touch c || existsb touch l.
Definition branch_conflict (A B : list nat) : bool := rlogs A B || existsb touch A || existsb touch B.

(** LOW-5: when no two chains of the scope conflict (F10, P5') and no chain
    conflicts with the base's own content (F7), the settled partition
    (dag_merger.rs:1236-1283) is empty and so is the branch-level map. F7
    covers a chain's own touching produces when the settled log is empty. *)
Theorem chain_level_p5_empties_settled_partition : forall chains base_log settled,
  (forall a b, In a chains -> In b chains -> a <> b -> chain_conflict a b = false) ->
  (forall c, In c chains -> log_conflict c base_log = false) ->
  incl settled chains ->
  (forall c, In c chains -> ~ In c settled -> log_conflict c settled = false) /\
  (forall A B, incl A chains -> incl B chains -> A <> [] -> B <> [] ->
     (forall a b, In a A -> In b B -> a <> b) -> branch_conflict A B = false).
Proof.
  intros chains base_log settled Hp5 Hf7 Hsub. split.
  - intros c Hc Hns. unfold log_conflict.
    assert (Htc : touch c = false).
    { pose proof (Hf7 c Hc) as H. unfold log_conflict in H.
      apply orb_false_iff in H as [H _]. apply orb_false_iff in H as [_ H]. exact H. }
    assert (Hcs : forall s, In s settled -> chain_conflict c s = false).
    { intros s Hs. apply Hp5; [exact Hc | apply Hsub; exact Hs |].
      intros Heq. subst s. exact (Hns Hs). }
    destruct (rlog c settled) eqn:Hr.
    + destruct (rlog_lifts c settled Hr) as [s [Hs Hrc]].
      pose proof (Hcs s Hs) as H. unfold chain_conflict in H. rewrite Hrc in H. discriminate.
    + rewrite Htc. simpl. apply existsb_false_intro. intros s Hs.
      pose proof (Hcs s Hs) as H. unfold chain_conflict in H.
      apply orb_false_iff in H as [_ H]. exact H.
  - intros A B HA HB HAne HBne Hdisj. unfold branch_conflict.
    destruct A as [| a0 A']; [exfalso; apply HAne; reflexivity |].
    destruct B as [| b0 B']; [exfalso; apply HBne; reflexivity |].
    apply orb_false_iff. split; [apply orb_false_iff; split |].
    + destruct (rlogs (a0 :: A') (b0 :: B')) eqn:Hr; [| reflexivity].
      destruct (rlogs_lifts _ _ Hr) as [a [b [Ha [Hb Hrc]]]].
      pose proof (Hp5 a b (HA a Ha) (HB b Hb) (Hdisj a b Ha Hb)) as H.
      unfold chain_conflict in H. rewrite Hrc in H. discriminate.
    + apply existsb_false_intro. intros a Ha.
      pose proof (Hp5 a b0 (HA a Ha) (HB b0 (or_introl eq_refl)) (Hdisj a b0 Ha (or_introl eq_refl))) as H.
      unfold chain_conflict in H. apply orb_false_iff in H as [H _]. apply orb_false_iff in H as [_ H].
      exact H.
    + apply existsb_false_intro. intros b Hb.
      pose proof (Hp5 a0 b (HA a0 (or_introl eq_refl)) (HB b Hb) (Hdisj a0 b (or_introl eq_refl) Hb)) as H.
      unfold chain_conflict in H. apply orb_false_iff in H as [_ H]. exact H.
Qed.

End Settled.

(** Negative control (LOW-5): chain 1 is settled and chain 2 races with it,
    inside one branch. A branch-level map compares no pair in a one-branch
    scope, so a branch-level P5 passes. The settled partition rejects chain
    2, so the slow path rejects it, while a fast path with branch-level P5
    would compose it. The chain-level map sees the race. *)
Theorem nc_branch_level_p5_misses_settled_conflict :
  let touch := fun _ : nat => false in
  let rc := fun a b : nat => ((Nat.eqb a 1 && Nat.eqb b 2) || (Nat.eqb a 2 && Nat.eqb b 1))%bool in
  let rlog := fun (c : nat) (l : list nat) => existsb (rc c) l in
  (forall A B, In A [[1; 2]] -> In B [[1; 2]] -> A <> B -> False) /\
  log_conflict touch rlog 2 [1] = true /\
  chain_conflict touch rc 1 2 = true.
Proof.
  intros touch rc rlog. split.
  - intros A B [HA | []] [HB | []] Hne. subst A B. apply Hne. reflexivity.
  - split; reflexivity.
Qed.

(** ** The walk order (MEDIUM-2) *)

Section WalkOrder.

(** [dep c s]: chain [c] depends on chain [s]. [prio_loss c]: the stamped
    prior-loss count of chain [c]. *)
Variable dep : nat -> nat -> bool.
Variable prio_loss : nat -> Z.

(** [priority_cmp] of dag_merger.rs:94-98: more losses first, then the
    chain order. *)
Definition prio_cmp (a b : nat) : comparison :=
  then_cmp (Z.compare (prio_loss b) (prio_loss a)) (Nat.compare a b).

(** [Iterator::min_by]: the first of the minimal elements. *)
Fixpoint min_by (l : list nat) : option nat :=
  match l with
  | [] => None
  | x :: rest =>
      match min_by rest with
      | None => Some x
      | Some m => match prio_cmp x m with Gt => Some m | _ => Some x end
      end
  end.

(** The chains that depend on no other pending chain. *)
Definition available (pending : list nat) : list nat :=
  filter (fun c => negb (existsb (fun s => negb (Nat.eqb s c) && dep c s) pending)) pending.

(** [dependency_ordered_branch_items] (dag_merger.rs:83-120). *)
Fixpoint dep_order (fuel : nat) (pending : list nat) : list nat :=
  match fuel with
  | O => []
  | S f =>
      match pending with
      | [] => []
      | _ :: _ =>
          match (match min_by (available pending) with
                 | Some x => Some x
                 | None => min_by pending
                 end) with
          | Some x => x :: dep_order f (remove Nat.eq_dec x pending)
          | None => []
          end
      end
  end.

Definition prio_key (a : nat) : list Z := [Z.opp (prio_loss a); Z.of_nat a].

Lemma prio_cmp_key : forall a b, prio_cmp a b = lexZ (prio_key a) (prio_key b).
Proof.
  intros a b. unfold prio_cmp, prio_key. rewrite !lexZ_cons, Z.compare_opp, Nat2Z.inj_compare.
  simpl. destruct (Z.compare (prio_loss b) (prio_loss a)); simpl; try reflexivity.
  destruct (Nat.compare a b); reflexivity.
Qed.

Lemma prio_cmp_eq : forall a b, prio_cmp a b = Eq -> a = b.
Proof.
  intros a b H. rewrite prio_cmp_key in H. apply lexZ_eq in H. unfold prio_key in H.
  injection H as _ H. exact (Nat2Z.inj a b H).
Qed.

Lemma prio_le_trans : forall a b c, prio_cmp a b <> Gt -> prio_cmp b c <> Gt -> prio_cmp a c <> Gt.
Proof.
  intros a b c. rewrite !prio_cmp_key. intros Hab Hbc.
  destruct (lexZ (prio_key a) (prio_key b)) eqn:Eab; [| | exfalso; apply Hab; reflexivity].
  - apply lexZ_eq in Eab. rewrite Eab. exact Hbc.
  - destruct (lexZ (prio_key b) (prio_key c)) eqn:Ebc; [| | exfalso; apply Hbc; reflexivity].
    + apply lexZ_eq in Ebc. rewrite <- Ebc, Eab. discriminate.
    + rewrite (lexZ_trans _ _ _ Eab Ebc). discriminate.
Qed.

Lemma min_by_none : forall l, min_by l = None -> l = [].
Proof.
  intros [| x rest] H; [reflexivity |]. simpl in H.
  destruct (min_by rest); [destruct (prio_cmp x n); discriminate | discriminate].
Qed.

Lemma min_by_spec : forall l m, min_by l = Some m -> In m l /\ forall y, In y l -> prio_cmp m y <> Gt.
Proof.
  induction l as [| x rest IH]; intros m H; simpl in H; [discriminate |].
  destruct (min_by rest) as [m' |] eqn:Hr.
  - destruct (IH m' eq_refl) as [Hin Hmin].
    destruct (prio_cmp x m') eqn:Hxm; injection H as Hm; subst m.
    + split; [left; reflexivity |]. intros y [Hy | Hy].
      * subst y. rewrite prio_cmp_key, lexZ_refl. discriminate.
      * apply (prio_le_trans _ m'); [rewrite Hxm; discriminate | exact (Hmin y Hy)].
    + split; [left; reflexivity |]. intros y [Hy | Hy].
      * subst y. rewrite prio_cmp_key, lexZ_refl. discriminate.
      * apply (prio_le_trans _ m'); [rewrite Hxm; discriminate | exact (Hmin y Hy)].
    + split; [right; exact Hin |]. intros y [Hy | Hy].
      * subst y. rewrite prio_cmp_key, lexZ_antisym, <- prio_cmp_key, Hxm. discriminate.
      * exact (Hmin y Hy).
  - injection H as Hm. subst m. apply min_by_none in Hr. subst rest.
    split; [left; reflexivity |]. intros y [Hy | []]. subst y.
    rewrite prio_cmp_key, lexZ_refl. discriminate.
Qed.

Lemma min_by_perm : forall l1 l2, Permutation l1 l2 -> min_by l1 = min_by l2.
Proof.
  intros l1 l2 Hp. destruct (min_by l1) as [m1 |] eqn:H1; destruct (min_by l2) as [m2 |] eqn:H2.
  - destruct (min_by_spec _ _ H1) as [Hin1 Hmin1]. destruct (min_by_spec _ _ H2) as [Hin2 Hmin2].
    f_equal. apply prio_cmp_eq.
    assert (Ha : prio_cmp m1 m2 <> Gt)
      by (apply Hmin1; exact (Permutation_in m2 (Permutation_sym Hp) Hin2)).
    assert (Hb : prio_cmp m2 m1 <> Gt) by (apply Hmin2; exact (Permutation_in m1 Hp Hin1)).
    rewrite prio_cmp_key in Ha, Hb |- *. rewrite lexZ_antisym in Hb.
    destruct (lexZ (prio_key m1) (prio_key m2)); [reflexivity | exfalso; apply Hb; reflexivity |
                                                 exfalso; apply Ha; reflexivity].
  - apply min_by_none in H2. subst l2. apply Permutation_sym, Permutation_nil in Hp. subst l1.
    discriminate.
  - apply min_by_none in H1. subst l1. apply Permutation_nil in Hp. subst l2. discriminate.
  - reflexivity.
Qed.

Lemma perm_filter : forall (f : nat -> bool) l l', Permutation l l' -> Permutation (filter f l) (filter f l').
Proof.
  intros f l l' H. induction H; simpl.
  - reflexivity.
  - destruct (f x); [apply perm_skip |]; exact IHPermutation.
  - destruct (f x), (f y); try reflexivity; apply perm_swap.
  - transitivity (filter f l'); assumption.
Qed.

Lemma perm_remove : forall x l l', Permutation l l' ->
  Permutation (remove Nat.eq_dec x l) (remove Nat.eq_dec x l').
Proof.
  intros x l l' H. induction H; simpl.
  - reflexivity.
  - destruct (Nat.eq_dec x x0); [exact IHPermutation | apply perm_skip; exact IHPermutation].
  - destruct (Nat.eq_dec x y), (Nat.eq_dec x x0); try reflexivity; apply perm_swap.
  - transitivity (remove Nat.eq_dec x l'); assumption.
Qed.

Lemma available_perm : forall p1 p2, Permutation p1 p2 -> Permutation (available p1) (available p2).
Proof.
  intros p1 p2 Hp. unfold available.
  transitivity (filter (fun c => negb (existsb (fun s => negb (Nat.eqb s c) && dep c s) p1)) p2).
  - apply perm_filter. exact Hp.
  - assert (Hext : forall c, existsb (fun s => negb (Nat.eqb s c) && dep c s) p1 =
                             existsb (fun s => negb (Nat.eqb s c) && dep c s) p2)
      by (intros c; apply existsb_perm; exact Hp).
    rewrite (filter_ext _ _ (fun c => f_equal negb (Hext c))). reflexivity.
Qed.

(** MEDIUM-2: the walk order depends only on the set of chains and their
    stamps. The fast path stamps its raw chains with the counts that the
    slow path uses (dag_merger.rs:965, fast.rs F4), so both paths walk in one
    order, and the order-dependent availability walk returns one result. *)
Theorem stamped_walk_matches_slow_path : forall fuel l1 l2,
  Permutation l1 l2 -> dep_order fuel l1 = dep_order fuel l2.
Proof.
  induction fuel as [| f IH]; intros l1 l2 Hp; simpl; [reflexivity |].
  destruct l1 as [| a r1]; destruct l2 as [| b r2].
  - reflexivity.
  - apply Permutation_nil in Hp. discriminate.
  - apply Permutation_sym, Permutation_nil in Hp. discriminate.
  - rewrite (min_by_perm _ _ (available_perm _ _ Hp)), (min_by_perm _ _ Hp).
    destruct (match min_by (available (b :: r2)) with Some x => Some x | None => min_by (b :: r2) end)
      as [x |]; [| reflexivity].
    f_equal. apply IH. apply perm_remove. exact Hp.
Qed.

End WalkOrder.

(** The numeric part of dev's availability walk on one base-single-number
    cell (dag_merger.rs:62-80,230-310): [cur] values are available; a chain
    removes [rm] of them and then adds [ad]. A removal beyond [cur] is
    unavailable, and an add that leaves more than one value overfills. The
    result is the list of rejected chains. *)
Fixpoint cell_walk (cur : nat) (order : list nat) (rm ad : nat -> nat) : list nat :=
  match order with
  | [] => []
  | x :: rest =>
      if (rm x <=? cur) && negb ((0 <? ad x) && (1 <? cur - rm x + ad x))
      then cell_walk (cur - rm x + ad x) rest rm ad
      else x :: cell_walk cur rest rm ad
  end.

(** Negative control (MEDIUM-2, the arbiter's A/B example): chain 0 only
    removes, and chain 1 only adds, on one base-single-number cell, with no
    dependency between them. Unstamped, the walk takes 0 first and rejects
    nothing, so the fast path composes both. Stamped with one prior loss for
    chain 1, the slow path takes 1 first and rejects it. *)
Theorem nc_unstamped_walk_breaks_fast_equals_slow :
  let nodep := fun _ _ : nat => false in
  let unstamped := fun _ : nat => 0%Z in
  let stamped := fun c : nat => if Nat.eqb c 1 then 1%Z else 0%Z in
  let rm := fun c : nat => if Nat.eqb c 0 then 1 else 0 in
  let ad := fun c : nat => if Nat.eqb c 1 then 1 else 0 in
  dep_order nodep unstamped 2 [0; 1] = [0; 1] /\
  dep_order nodep stamped 2 [0; 1] = [1; 0] /\
  cell_walk 1 [0; 1] rm ad = [] /\
  cell_walk 1 [1; 0] rm ad = [1].
Proof. vm_compute. repeat split. Qed.

(** ** Negative controls of the ordered pass *)

(** Without the S5 conflict check, two conflicting candidates both
    survive. *)
Theorem nc_no_claim_check :
  let always := fun _ _ : list nat => true in
  let no_claims := fun _ : nat => @nil nat in
  let ok := fun _ : list nat => true in
  walk_unclaimed no_claims no_claims ok [[1]; [2]] [] = [[1]; [2]] /\
  always [1] [2] = true /\
  fst (walk always no_claims no_claims ok [[1]; [2]] [] []) = [[1]].
Proof. vm_compute. repeat split. Qed.

(** A potential-COMM model of dev's conflict test: a branch's net produces
    and net consumes are those its own COMMs do not destroy
    (merging_logic.rs:526-575). Chain 1 produces on channel 7, and chains 2
    and 3 consume on it. *)
Definition pc_prod (x : nat) : list nat := if Nat.eqb x 1 then [7] else [].
Definition pc_cons (x : nat) : list nat := if (Nat.eqb x 2 || Nat.eqb x 3)%bool then [7] else [].
Definition net (mine other : list nat) : list nat :=
  filter (fun ch => negb (existsb (Nat.eqb ch) other)) mine.
Definition pc_conflict (a b : list nat) : bool :=
  meets (net (flat_map pc_prod a) (flat_map pc_cons a)) (net (flat_map pc_cons b) (flat_map pc_prod b)) ||
  meets (net (flat_map pc_prod b) (flat_map pc_cons b)) (net (flat_map pc_cons a) (flat_map pc_prod a)).

(** Negative control (MEDIUM-1): the branch [1; 2] consumes its own
    produce, so it does not conflict with [3]. Lineage closure removes chain
    2 (its block descends from the base-conflicting block 9), and the rest
    [1] conflicts with [3]. Without the re-check both survive; with it the
    repair loop drops [3]. *)
Theorem nc_partial_lineage_creates_conflict :
  let pin := fun _ : nat => false in
  let no_overfill := fun _ : list (list nat) => @None nat in
  let no_adds := fun _ _ : nat => false in
  let no_balance := fun _ : list nat => @None (nat * failure) in
  let no_contrib := fun _ _ : nat => @None Z in
  let src := fun x : nat => x in
  let anc := fun a b : nat => (Nat.eqb a 9 && Nat.eqb b 2)%bool in
  let id_sort := fun l : list (list nat) => l in
  pc_conflict [1; 2] [3] = false /\ pc_conflict [1] [3] = true /\
  repair_norecheck pin no_overfill no_adds no_balance no_contrib src anc [9] [] id_sort
    3 [[1; 2]; [3]] [] = Some [[1]; [3]] /\
  repair pin pc_conflict no_overfill no_adds no_balance no_contrib src anc [9] [] id_sort
    3 false [[1; 2]; [3]] [] [] =
    Some ([[1]], [2; 3], [([2], WStale 9); ([3], WLineageConflict [1])]).
Proof. vm_compute. repeat split. Qed.

(** A number channel: a folded change adds a diff, and a plain change writes
    a value. With a folded change present, compose writes the base plus the
    folded diffs (rholang_merging_logic.rs:116-176), and the plain value is
    lost. *)
Inductive num_change : Type := Fold (d : Z) | Plain (v : Z).

Definition serial_number (base : Z) (l : list num_change) : Z :=
  fold_left (fun acc c => match c with Fold d => (acc + d)%Z | Plain v => v end) l base.

Definition compose_number (base : Z) (l : list num_change) : Z :=
  if existsb (fun c => match c with Fold _ => true | Plain _ => false end) l
  then (base + fold_left (fun acc c => match c with Fold d => (acc + d)%Z | Plain _ => acc end) l 0)%Z
  else serial_number base l.

(** Negative control (C1): with base 5, a folded +1 and a plain write of 10
    compose to 6, which no serial order gives. *)
Theorem nc_no_folded_mixing_drops_plain_value :
  compose_number 5 [Fold 1; Plain 10] = 6%Z /\
  serial_number 5 [Fold 1; Plain 10] = 10%Z /\
  serial_number 5 [Plain 10; Fold 1] = 11%Z.
Proof. vm_compute. repeat split. Qed.

(** A deploy chain with its deploy id, its cost and its post-state. *)
Record chain_rec : Type := mk_chain { cr_id : nat; cr_cost : nat; cr_post : nat }.

(** [DeployChainIndex] equality compares the deploys with their costs
    (deploy_chain_index.rs:163-171), not the post-state. *)
Definition chain_eqb (a b : chain_rec) : bool :=
  (Nat.eqb (cr_id a) (cr_id b) && Nat.eqb (cr_cost a) (cr_cost b))%bool.

(** A [HashSet] keeps one chain of each equality class. *)
Fixpoint dedup (l : list chain_rec) : list chain_rec :=
  match l with
  | [] => []
  | x :: rest => if existsb (chain_eqb x) rest then dedup rest else x :: dedup rest
  end.

(** [has_repeated_user_id] of claims.rs, on a list. *)
Fixpoint has_repeated_id (l : list chain_rec) : bool :=
  match l with
  | [] => false
  | x :: rest => existsb (fun y => Nat.eqb (cr_id x) (cr_id y)) rest || has_repeated_id rest
  end.

(** Negative control (C2): two equal-cost copies of deploy 7 with different
    post-states. The raw list shows the repeat (F5); the set collapses the
    copies and hides it; and composing the raw list applies deploy 7
    twice. *)
Theorem nc_set_collapse_hides_duplicate :
  let a := mk_chain 7 3 100 in
  let b := mk_chain 7 3 200 in
  has_repeated_id [a; b] = true /\ has_repeated_id (dedup [a; b]) = false /\
  length (filter (fun c => Nat.eqb (cr_id c) 7) [a; b]) = 2.
Proof. vm_compute. repeat split. Qed.

(** Negative control (LOW-4): a branch whose user parts overflow fails the
    S3 pre-check, which rejects it. Without the pre-check, the user fold of
    [compute_branch_derived] overflows and the merge aborts. *)
Theorem nc_overflow_aborts_without_precheck :
  let chains := [(MAX, (- MAX)%Z); (1%Z, 0%Z)] in
  ~ branch_valid chains /\ checked_fold 0%Z (users chains) = None.
Proof.
  intros chains. split.
  - intros [[Hpos _] _]. vm_compute in Hpos. apply Hpos. reflexivity.
  - vm_compute. reflexivity.
Qed.

(** Negative control (S6.3): the candidate [1; 2] consumes the base value
    of cell 5 (chain 1) and writes a new one (chain 2). Lineage closure
    removes chain 1, and the rest overfills the cell. A pass that stops after
    lineage closure keeps the overfill; the repair loop drops [2]. *)
Theorem nc_no_repair_after_lineage_overfills :
  let pin := fun _ : nat => false in
  let ov := fun K : list (list nat) =>
    if 1 <? (1 - length (filter (Nat.eqb 1) (concat K))) + length (filter (Nat.eqb 2) (concat K))
    then Some 5 else None in
  let adds := fun x ch : nat => (Nat.eqb x 2 && Nat.eqb ch 5)%bool in
  let src := fun x : nat => x in
  let anc := fun a b : nat => (Nat.eqb a 9 && Nat.eqb b 1)%bool in
  let id_sort := fun l : list (list nat) => l in
  ov [[1; 2]] = None /\
  lineage_only pin src anc [9] [] id_sort [[1; 2]] [] = [[2]] /\
  ov [[2]] = Some 5 /\
  repair pin (fun _ _ => false) ov adds (fun _ => None) (fun _ _ => None) src anc [9] [] id_sort
    3 false [[1; 2]] [] [] = Some ([], [1; 2], [([1], WStale 9); ([2], WOverfill 5 [[2]])]).
Proof. vm_compute. repeat split. Qed.

(** K without its pinned key, and K without its loss keys. *)
Definition k_without_pinned (loss height : nat -> Z) (a b : list nat) : comparison :=
  then_cmp (Z.compare (loss_max loss b) (loss_max loss a))
    (then_cmp (Z.compare (loss_sum loss b) (loss_sum loss a))
      (then_cmp (Z.compare (lowest_height height a) (lowest_height height b))
        (compare_branches a b))).

Definition k_without_losses (pinned : nat -> bool) (height : nat -> Z) (a b : list nat) : comparison :=
  then_cmp (bool_cmp (negb (bpinned pinned a)) (negb (bpinned pinned b)))
    (then_cmp (Z.compare (lowest_height height a) (lowest_height height b)) (compare_branches a b)).

(** Negative control (#341): chain 2 is settled, and chain 1 has one prior
    loss; the two conflict. K keeps the settled candidate. Without the
    pinned key, the loss puts [1] first, and the walk rejects the settled
    chain. *)
Theorem nc_pinned_not_first_rejects_settled :
  let pin := fun c : nat => Nat.eqb c 2 in
  let loss := fun c : nat => if Nat.eqb c 1 then 1%Z else 0%Z in
  let height := fun _ : nat => 0%Z in
  let always := fun _ _ : list nat => true in
  let no_claims := fun _ : nat => @nil nat in
  let ok := fun _ : list nat => true in
  bpinned pin [2] = true /\
  fst (walk always no_claims no_claims ok (sort_by_k pin loss height [[1]; [2]]) [] []) = [[2]] /\
  fst (walk always no_claims no_claims ok (isort (k_without_pinned loss height) [[1]; [2]]) [] []) = [[1]].
Proof. vm_compute. repeat split. Qed.

(** Negative control (#294): two unpinned candidates conflict, and [2] has
    five prior losses. K keeps [2]. Without the loss keys, dev's chain order
    puts [1] first, and [2] loses again, round after round. *)
Theorem nc_losses_not_first_starves :
  let pin := fun _ : nat => false in
  let loss := fun c : nat => if Nat.eqb c 2 then 5%Z else 0%Z in
  let height := fun _ : nat => 0%Z in
  let always := fun _ _ : list nat => true in
  let no_claims := fun _ : nat => @nil nat in
  let ok := fun _ : list nat => true in
  fst (walk always no_claims no_claims ok (sort_by_k pin loss height [[1]; [2]]) [] []) = [[2]] /\
  fst (walk always no_claims no_claims ok (isort (k_without_losses pin height) [[1]; [2]]) [] []) = [[1]].
Proof. vm_compute. repeat split. Qed.

(** Negative control (C3, the prefix-fold divergence): in arrival order
    [MAX; -MAX; 1], a prefix fold from base 0 accepts every diff, so a fast
    path with a prefix ledger would compose all three. The sign split does
    not fit, so the ordered pass rejects one candidate, and compose's fold in
    another order overflows. *)
Theorem nc_fast_path_prefix_ledger_differs :
  prefix_accept 0%Z [MAX; (- MAX)%Z; 1%Z] = [MAX; (- MAX)%Z; 1%Z] /\
  ~ fits [MAX; (- MAX)%Z; 1%Z] /\
  checked_fold 0%Z [MAX; 1%Z; (- MAX)%Z] = None.
Proof.
  split; [vm_compute; reflexivity |]. split.
  - intros [Hpos _]. vm_compute in Hpos. apply Hpos. reflexivity.
  - vm_compute. reflexivity.
Qed.
