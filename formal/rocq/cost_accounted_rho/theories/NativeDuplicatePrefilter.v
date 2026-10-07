(* D-S4 (epic 8946, Phase D item D-A4; decision record DR-90): the stored
   consume's duplicate check compares source hashes before it builds and
   compares continuation identities.

   A waiting continuation has patterns, a body, a persistence flag and a set
   of peeks. Its identity (the Debug text the check compares) determines all
   four. Its source hash is computed from the sorted pattern encodings, the
   body and the flag (the channels are shared by every continuation in one
   cache entry). The model takes the hash as an arbitrary function H of the
   sorted patterns, the body and the flag: no property of H (in particular
   no collision resistance) is assumed.

   Results:
   - identity_eq_implies_source_hash_eq: equal identities give equal source
     hashes, for every H;
   - prefiltered_duplicate_equals_scan: the duplicate decision with the
     source-hash prefilter equals the decision of the full identity scan;
   - prefilter_builds_identities_only_on_hash_ties: the prefiltered check
     builds identities only for stored continuations whose hash equals the
     new one;
   - permuted_patterns_share_hash: a proved negative control showing that a
     hash-only check would be wrong (two continuations with permuted
     patterns share the hash but differ in identity).

   Rust correspondence: rspace++/src/rspace/hot_store/native.rs
   (native_store_consume) and rspace++/src/rspace/hashing/native_source.rs
   (consume). *)

From Stdlib Require Import Lists.List Arith.PeanoNat Bool Sorting.Mergesort.
Import ListNotations.

Record continuation := {
  patterns : list nat;
  body : nat;
  persist : bool;
  peeks : list nat
}.

Definition identity (c : continuation) : list nat * nat * bool * list nat :=
  (patterns c, body c, persist c, peeks c).

Fixpoint nat_list_eqb (xs ys : list nat) : bool :=
  match xs, ys with
  | [], [] => true
  | x :: xs', y :: ys' => Nat.eqb x y && nat_list_eqb xs' ys'
  | _, _ => false
  end.

Lemma nat_list_eqb_true : forall xs ys, nat_list_eqb xs ys = true <-> xs = ys.
Proof.
  induction xs as [| x xs IH]; intros [| y ys]; cbn; try (split; congruence).
  rewrite andb_true_iff, Nat.eqb_eq, IH.
  split; [intros [-> ->]; reflexivity | intros same; injection same as -> ->; tauto].
Qed.

Definition identity_eqb (x y : continuation) : bool :=
  nat_list_eqb (patterns x) (patterns y) && Nat.eqb (body x) (body y)
  && Bool.eqb (persist x) (persist y) && nat_list_eqb (peeks x) (peeks y).

Section Hash.

Variable H : list nat -> nat -> bool -> nat.

Definition source_hash (c : continuation) : nat := H (NatSort.sort (patterns c)) (body c) (persist c).

Theorem identity_eq_implies_source_hash_eq : forall x y,
  identity x = identity y -> source_hash x = source_hash y.
Proof.
  intros [px bx sx kx] [py by_ sy ky] same. unfold identity in same. cbn in same.
  injection same as same_peeks same_persist same_body same_patterns.
  unfold source_hash. cbn. subst. reflexivity.
Qed.

Lemma identity_eqb_true : forall x y, identity_eqb x y = true <-> identity x = identity y.
Proof.
  intros [px bx sx kx] [py by_ sy ky]. unfold identity_eqb, identity. cbn.
  rewrite !andb_true_iff, !nat_list_eqb_true, Nat.eqb_eq, eqb_true_iff.
  split.
  - intros [[[-> ->] ->] ->]. reflexivity.
  - intros same. injection same as -> -> -> ->. tauto.
Qed.

(* The legacy check: compare the new identity with every stored one. *)
Definition scan_duplicate (stored : list continuation) (waiting : continuation) : bool :=
  existsb (fun prior => identity_eqb prior waiting) stored.

(* The D-S4 check: compare identities only on equal source hashes. *)
Definition prefiltered_duplicate (stored : list continuation) (waiting : continuation) : bool :=
  existsb (fun prior => Nat.eqb (source_hash prior) (source_hash waiting)
                        && identity_eqb prior waiting) stored.

Theorem prefiltered_duplicate_equals_scan : forall stored waiting,
  prefiltered_duplicate stored waiting = scan_duplicate stored waiting.
Proof.
  intros stored waiting. unfold prefiltered_duplicate, scan_duplicate.
  induction stored as [| prior rest IH]; [reflexivity |]. cbn. rewrite IH.
  destruct (identity_eqb prior waiting) eqn:same.
  - apply identity_eqb_true in same.
    rewrite (identity_eq_implies_source_hash_eq prior waiting same), Nat.eqb_refl. reflexivity.
  - rewrite andb_false_r. reflexivity.
Qed.

(* The identities the prefiltered check builds: the stored continuations
   whose source hash equals the new one (and the new identity, once, if
   there is at least one). *)
Definition built_identities (stored : list continuation) (waiting : continuation) : list continuation :=
  filter (fun prior => Nat.eqb (source_hash prior) (source_hash waiting)) stored.

Theorem prefilter_builds_identities_only_on_hash_ties : forall stored waiting prior,
  In prior (built_identities stored waiting) ->
  In prior stored /\ source_hash prior = source_hash waiting.
Proof.
  intros stored waiting prior member. unfold built_identities in member.
  apply filter_In in member as [stored_member same]. apply Nat.eqb_eq in same. tauto.
Qed.

End Hash.

Example permuted_patterns_share_hash :
  let x := {| patterns := [1; 2]; body := 7; persist := false; peeks := [] |} in
  let y := {| patterns := [2; 1]; body := 7; persist := false; peeks := [] |} in
  (forall H, source_hash H x = source_hash H y) /\ identity_eqb x y = false.
Proof.
  cbn. split.
  - intros H. unfold source_hash. cbn. vm_compute. reflexivity.
  - reflexivity.
Qed.

Print Assumptions identity_eq_implies_source_hash_eq.
Print Assumptions prefiltered_duplicate_equals_scan.
Print Assumptions prefilter_builds_identities_only_on_hash_ties.
Print Assumptions permuted_patterns_share_hash.
