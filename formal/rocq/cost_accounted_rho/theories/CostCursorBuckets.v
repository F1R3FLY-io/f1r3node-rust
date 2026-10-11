(** * CostCursorBuckets

    DR-119 (bug 11004): the creation locks of the SystemVault cost cursors.

    Before DR-119, every cursor-bearing settlement consumed one global lock
    datum (SystemVault.rho:52,56,146). Two settlements of two different
    payers therefore consumed one base datum, and the race rule of the merge
    (merging_logic.rs:290-331) made them conflict in every merge.

    DR-119 removes the global lock:
    - A settlement on an existing cursor takes no lock. It consumes only the
      revision and position cells of its own scope.
    - A first use takes the creation lock of its bucket. The bucket is the
      first byte of keccak256(scope.toByteArray()). The cost-cursor
      TreeHashMap has depth 2 (SystemVault.rho:55), so a map leaf is the
      first two bytes of that hash (Registry.rho:85-89,130-137).

    The model abstracts the hash into two byte functions, so every result
    holds for every hash function. A merge outcome keeps settlements whose
    claims on base datums are pairwise disjoint, which is the race rule.
    The negative controls instantiate the model to show what each
    alternative breaks. *)

From Stdlib Require Import Lists.List PeanoNat.
Import ListNotations.

Lemma nth_error_nil_none : forall (A : Type) (n : nat), nth_error (@nil A) n = None.
Proof. intros A n. destruct n; reflexivity. Qed.

Section Buckets.

  (** A cohort scope, and the first two bytes of its keccak256 hash. *)
  Variable Scope : Type.
  Variable byte0 byte1 : Scope -> nat.

  (** The TreeHashMap leaf of a scope, and its creation bucket. *)
  Definition leaf (s : Scope) : nat * nat := (byte0 s, byte1 s).
  Definition bucket (s : Scope) : nat := byte0 s.

  (** B1: one leaf has one bucket. *)
  Theorem same_leaf_same_bucket :
    forall s t, leaf s = leaf t -> bucket s = bucket t.
  Proof.
    intros s t Hleaf.
    unfold leaf in Hleaf.
    unfold bucket.
    injection Hleaf as H0 H1.
    exact H0.
  Qed.

  (** A base datum that a settlement can consume. *)
  Inductive datum : Type :=
  | Revision (s : Scope)
  | Position (s : Scope)
  | BucketLock (b : nat)
  | GlobalLock.

  (** A settlement: its scope, and whether its cursor is absent at the base. *)
  Record settlement : Type := {
    target : Scope;
    first_use : bool
  }.

  (** The lock rule of SystemVault. *)
  Inductive lock_rule : Type :=
  | GlobalRule
  | BucketRule
  | NoCreationRule
  | RawByteRule (raw : Scope -> nat).

  (** The base cells of a settlement. A first use creates its two cells and
      consumes them in the same deployment, so it claims no base cell. *)
  Definition cells (x : settlement) : list datum :=
    if first_use x then [] else [Revision (target x); Position (target x)].

  (** The base datums that a settlement consumes under a lock rule. *)
  Definition claims (rule : lock_rule) (x : settlement) : list datum :=
    match rule with
    | GlobalRule => GlobalLock :: cells x
    | BucketRule => if first_use x then [BucketLock (bucket (target x))] else cells x
    | NoCreationRule => cells x
    | RawByteRule raw => if first_use x then [BucketLock (raw (target x))] else cells x
    end.

  (** Two settlements conflict when they consume a common base datum. *)
  Definition conflict (rule : lock_rule) (x y : settlement) : Prop :=
    exists d, In d (claims rule x) /\ In d (claims rule y).

  (** A merge outcome keeps settlements that conflict pairwise with none. *)
  Definition mergeable (rule : lock_rule) (kept : list settlement) : Prop :=
    forall i j x y,
      i <> j -> nth_error kept i = Some x -> nth_error kept j = Some y -> ~ conflict rule x y.

  (** B2: under DR-119, two kept first uses create distinct leaves. *)
  Theorem kept_creations_distinct_leaves :
    forall kept i j x y,
      mergeable BucketRule kept ->
      i <> j -> nth_error kept i = Some x -> nth_error kept j = Some y ->
      first_use x = true -> first_use y = true ->
      leaf (target x) <> leaf (target y).
  Proof.
    intros kept i j x y Hmerge Hij Hx Hy Hfx Hfy Hleaf.
    apply (Hmerge i j x y Hij Hx Hy).
    exists (BucketLock (bucket (target x))).
    unfold claims.
    rewrite Hfx, Hfy.
    split.
    - left. reflexivity.
    - left. rewrite (same_leaf_same_bucket (target x) (target y) Hleaf). reflexivity.
  Qed.

  Lemma cells_of_existing :
    forall x, first_use x = false -> cells x = [Revision (target x); Position (target x)].
  Proof. intros x Hx. unfold cells. rewrite Hx. reflexivity. Qed.

  (** L6: under DR-119, two settlements on existing cursors of distinct
      scopes claim disjoint datums, so they never conflict. *)
  Theorem existing_scopes_disjoint_claims :
    forall x y,
      first_use x = false -> first_use y = false ->
      target x <> target y ->
      ~ conflict BucketRule x y.
  Proof.
    intros x y Hx Hy Hneq [d [Hdx Hdy]].
    unfold claims in Hdx, Hdy.
    rewrite Hx in Hdx.
    rewrite Hy in Hdy.
    rewrite (cells_of_existing x Hx) in Hdx.
    rewrite (cells_of_existing y Hy) in Hdy.
    simpl in Hdx, Hdy.
    destruct Hdx as [Hdx | [Hdx | []]]; destruct Hdy as [Hdy | [Hdy | []]];
      subst d; try discriminate.
    - injection Hdy as Heq. apply Hneq. exact (eq_sym Heq).
    - injection Hdy as Heq. apply Hneq. exact (eq_sym Heq).
  Qed.

  (** IndependentScopesSurvive': settlements on existing cursors of
      pairwise distinct scopes form a mergeable set. *)
  Theorem independent_existing_scopes_survive :
    forall kept,
      (forall x, In x kept -> first_use x = false) ->
      (forall i j x y,
          i <> j -> nth_error kept i = Some x -> nth_error kept j = Some y ->
          target x <> target y) ->
      mergeable BucketRule kept.
  Proof.
    intros kept Hexisting Hdistinct i j x y Hij Hx Hy.
    apply existing_scopes_disjoint_claims.
    - apply Hexisting. exact (nth_error_In kept i Hx).
    - apply Hexisting. exact (nth_error_In kept j Hy).
    - exact (Hdistinct i j x y Hij Hx Hy).
  Qed.

  (** SameScopeKeepsOne: two settlements on one existing cursor conflict on
      its revision cell, so a merge keeps one of them. *)
  Theorem same_scope_existing_settlements_conflict :
    forall x y,
      first_use x = false -> first_use y = false ->
      target x = target y ->
      conflict BucketRule x y.
  Proof.
    intros x y Hx Hy Heq.
    exists (Revision (target x)).
    unfold claims.
    rewrite Hx, Hy.
    rewrite (cells_of_existing x Hx), (cells_of_existing y Hy).
    split.
    - left. reflexivity.
    - left. rewrite Heq. reflexivity.
  Qed.

  (** Negative control: under the global lock every two settlements conflict,
      also when their scopes differ, so independent scopes never survive. *)
  Theorem nc_global_lock_conflicts_distinct_scopes :
    forall x y, conflict GlobalRule x y.
  Proof.
    intros x y.
    exists GlobalLock.
    split; left; reflexivity.
  Qed.

  (** Negative control: without a creation lock, two first uses claim
      nothing, so a merge keeps both, whatever their leaves. *)
  Theorem nc_no_creation_lock_two_leaf_datums :
    forall x y,
      first_use x = true -> first_use y = true ->
      mergeable NoCreationRule [x; y].
  Proof.
    intros x y Hx Hy i j a b Hij Ha Hb [d [Hda Hdb]].
    unfold claims, cells in Hda.
    destruct i as [| [| i]]; simpl in Ha.
    - injection Ha as Ha. subst a. rewrite Hx in Hda. exact Hda.
    - injection Ha as Ha. subst a. rewrite Hy in Hda. exact Hda.
    - rewrite nth_error_nil_none in Ha. discriminate.
  Qed.

End Buckets.

(** A concrete instance: scopes are natural numbers, and every scope lies in
    leaf (0, 0). It is a valid instance of the abstract hash, and it shows
    the two unsafe rules on one leaf. *)
Definition one_leaf (_ : nat) : nat := 0.

Definition first_use_of (s : nat) : settlement nat := {| target := s; first_use := true |}.

(** Negative control: a bucket from the raw scope bytes does not respect the
    leaves. Two first uses in one leaf take different locks, the merge keeps
    both, and the leaf receives two datums. *)
Theorem nc_scope_byte_bucket_splits_leaf :
  mergeable nat one_leaf (RawByteRule nat (fun s => s)) [first_use_of 1; first_use_of 2] /\
  leaf nat one_leaf one_leaf (target nat (first_use_of 1)) =
    leaf nat one_leaf one_leaf (target nat (first_use_of 2)).
Proof.
  split.
  - intros i j a b Hij Ha Hb [d [Hda Hdb]].
    destruct i as [| [| i]]; simpl in Ha;
      [ | | rewrite nth_error_nil_none in Ha; discriminate ];
      destruct j as [| [| j]]; simpl in Hb;
      try (rewrite nth_error_nil_none in Hb; discriminate);
      try (apply Hij; reflexivity);
      injection Ha as Ha; injection Hb as Hb; subst a b;
      simpl in Hda, Hdb;
      destruct Hda as [Hda | []]; destruct Hdb as [Hdb | []]; subst d; discriminate.
  - reflexivity.
Qed.

(** The witness of the no-lock control on one leaf: two first uses of
    different scopes in leaf (0, 0) form a mergeable set. *)
Theorem nc_no_creation_lock_witness :
  mergeable nat one_leaf (NoCreationRule nat) [first_use_of 1; first_use_of 2] /\
  leaf nat one_leaf one_leaf 1 = leaf nat one_leaf one_leaf 2.
Proof.
  split.
  - apply nc_no_creation_lock_two_leaf_datums; reflexivity.
  - reflexivity.
Qed.

(** The DR-119 rule on the same leaf: the two first uses conflict on their
    shared bucket lock, so a merge keeps one of them. *)
Theorem bucket_rule_detects_the_shared_leaf :
  conflict nat one_leaf (BucketRule nat) (first_use_of 1) (first_use_of 2).
Proof.
  exists (BucketLock nat 0).
  split; left; reflexivity.
Qed.
