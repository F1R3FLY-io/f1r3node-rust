(* D-M4 (epic 8946, Phase D item D-A3; decision record DR-91): the bound
   values of a match move out of the owned free map in key order instead of
   being looked up and copied level by level.

   The free map is an ordered map from levels (Z) to bound values; the model
   is its sorted association list. The legacy extraction looked up each level
   0, 1, ..., n - 1 (inspecting the whole map at every level) and copied the
   bound value; a missing level gives the default value. The D-M4 extraction
   consumes the owned map with a cursor: at each level it skips the keys below
   the level and takes the key equal to the level, if any.

   Results:
   - extraction_by_move_equals_lookup: for a sorted map and strictly
     increasing levels, the cursor extraction returns exactly the lookups at
     those levels (keys below the first level, between levels and above the
     last level are skipped, as the legacy lookups ignored them);
   - legacy_extraction_charge_quadratic and legacy_extraction_charge_example:
     the legacy charge is at least quadratic in the number of bindings (every
     level inspected the whole map and copied its binding), while the D-M4
     charge (move_extraction_charge) has no binding-size term.

   Rust correspondence: rholang/src/rust/interpreter/matcher/match.rs
   (Matcher::get_with_context, the bound_pars extraction). *)

From Stdlib Require Import Lists.List ZArith Lia Sorting.Sorted.
Import ListNotations.

Section Extraction.

Variable V : Type.

Fixpoint lookup (m : list (Z * V)) (level : Z) : option V :=
  match m with
  | [] => None
  | (k, v) :: rest => if Z.eqb k level then Some v else lookup rest level
  end.

Fixpoint skip_below (level : Z) (m : list (Z * V)) : list (Z * V) :=
  match m with
  | [] => []
  | (k, v) :: rest => if Z.ltb k level then skip_below level rest else m
  end.

Definition take_at (level : Z) (m : list (Z * V)) : option V * list (Z * V) :=
  match skip_below level m with
  | (k, v) :: rest => if Z.eqb k level then (Some v, rest) else (None, (k, v) :: rest)
  | [] => (None, [])
  end.

Fixpoint move_extract (levels : list Z) (m : list (Z * V)) : list (option V) :=
  match levels with
  | [] => []
  | level :: more => let '(found, rest) := take_at level m in found :: move_extract more rest
  end.

Definition sorted_keys (m : list (Z * V)) : Prop :=
  StronglySorted (fun left right => (fst left < fst right)%Z) m.

Lemma lookup_skip : forall level l m, (level <= l)%Z ->
  lookup (skip_below level m) l = lookup m l.
Proof.
  intros level l m bound. induction m as [| [k v] rest IH]; [reflexivity |].
  cbn. destruct (Z.ltb_spec k level) as [below | above].
  - rewrite IH. destruct (Z.eqb_spec k l); [lia | reflexivity].
  - reflexivity.
Qed.

Lemma skip_sorted : forall level m, sorted_keys m -> sorted_keys (skip_below level m).
Proof.
  intros level m sorted. induction m as [| [k v] rest IH]; [exact sorted |].
  cbn. inversion sorted as [| ? ? sorted_rest _]; subst.
  destruct (Z.ltb k level); [apply IH; exact sorted_rest | exact sorted].
Qed.

Lemma skip_head_at_least : forall level m k v rest,
  skip_below level m = (k, v) :: rest -> (level <= k)%Z.
Proof.
  intros level m. induction m as [| [k' v'] more IH]; intros k v rest skipped; [discriminate |].
  cbn in skipped. destruct (Z.ltb_spec k' level) as [below | above].
  - exact (IH _ _ _ skipped).
  - injection skipped as -> _ _. exact above.
Qed.

Lemma lookup_none_above : forall l k v rest,
  sorted_keys ((k, v) :: rest) -> (l <= k)%Z -> lookup rest l = None.
Proof.
  intros l k v rest sorted bound.
  inversion sorted as [| ? ? sorted_rest larger]; subst.
  clear sorted. revert sorted_rest larger. induction rest as [| [k' v'] more IH];
    intros sorted_rest larger; [reflexivity |].
  cbn. inversion larger as [| ? ? head tail]; subst. cbn in head.
  inversion sorted_rest as [| ? ? sorted_more _]; subst.
  destruct (Z.eqb_spec k' l); [lia |]. apply IH; assumption.
Qed.

Lemma take_found : forall level m, sorted_keys m ->
  fst (take_at level m) = lookup m level.
Proof.
  intros level m sorted. rewrite <- (lookup_skip level level m) by lia.
  unfold take_at. destruct (skip_below level m) as [| [k v] rest] eqn:skipped; [reflexivity |].
  pose proof (skip_head_at_least _ _ _ _ _ skipped) as at_least.
  pose proof (skip_sorted level m sorted) as sorted_skip. rewrite skipped in sorted_skip.
  cbn. destruct (Z.eqb_spec k level) as [same | different]; [reflexivity |].
  cbn. symmetry. apply (lookup_none_above level k v rest); [exact sorted_skip | lia].
Qed.

Lemma take_rest : forall level l m, sorted_keys m -> (level < l)%Z ->
  lookup (snd (take_at level m)) l = lookup m l.
Proof.
  intros level l m sorted bound. rewrite <- (lookup_skip level l m) by lia.
  unfold take_at. destruct (skip_below level m) as [| [k v] rest] eqn:skipped; [reflexivity |].
  pose proof (skip_head_at_least _ _ _ _ _ skipped) as at_least.
  destruct (Z.eqb_spec k level) as [same | different]; cbn; [| reflexivity].
  destruct (Z.eqb_spec k l); [lia | reflexivity].
Qed.

Lemma take_rest_sorted : forall level m, sorted_keys m -> sorted_keys (snd (take_at level m)).
Proof.
  intros level m sorted. pose proof (skip_sorted level m sorted) as sorted_skip.
  unfold take_at. destruct (skip_below level m) as [| [k v] rest]; [constructor |].
  inversion sorted_skip as [| ? ? sorted_rest _]; subst.
  destruct (Z.eqb k level); [exact sorted_rest | exact sorted_skip].
Qed.

Theorem extraction_by_move_equals_lookup : forall levels m,
  sorted_keys m -> StronglySorted Z.lt levels ->
  move_extract levels m = map (lookup m) levels.
Proof.
  induction levels as [| level more IH]; intros m sorted increasing; [reflexivity |].
  cbn. destruct (take_at level m) as [found rest] eqn:taken.
  inversion increasing as [| ? ? increasing_more larger]; subst.
  f_equal.
  - pose proof (take_found level m sorted) as found_spec. rewrite taken in found_spec.
    exact found_spec.
  - pose proof (take_rest_sorted level m sorted) as sorted_rest. rewrite taken in sorted_rest.
    rewrite (IH rest sorted_rest increasing_more).
    apply map_ext_in. intros l member.
    rewrite Forall_forall in larger. specialize (larger l member).
    pose proof (take_rest level l m sorted larger) as same. rewrite taken in same.
    exact same.
Qed.

End Extraction.

(* Charges, for n levels over a map of n bindings of size s each: the legacy
   extraction inspected the whole map (n * s) and copied one binding (s) at
   every level; D-M4 charges one step of slot bytes per level and one
   operation per skipped key, whatever the binding sizes. *)
Definition legacy_extraction_charge (n s : nat) : nat := n * (n * s + s).
Definition move_extraction_charge (n slot skipped : nat) : nat := n * slot + skipped.

Theorem legacy_extraction_charge_quadratic : forall n s,
  n * n * s <= legacy_extraction_charge n s.
Proof. intros n s. unfold legacy_extraction_charge. nia. Qed.

Example legacy_extraction_charge_example :
  legacy_extraction_charge 5 100 = 3000 /\ move_extraction_charge 5 16 0 = 80.
Proof. split; vm_compute; reflexivity. Qed.

Print Assumptions extraction_by_move_equals_lookup.
Print Assumptions legacy_extraction_charge_quadratic.
Print Assumptions legacy_extraction_charge_example.
