(* C9 (epic 8946, B1 Phase C; decision record DR-84): a split adds one
   causal-path segment instead of two.

   The deterministic scheduler orders RSpace operations by their causal
   paths, in the forward lexicographic order of Vec<(u64, u64)>. An
   operation of participant P at step s has the path P (s, 0). Before C9,
   child i of a split at step s had the participant path P (s, 1) (i, 0).
   After C9, child i has the participant path P (s, i + 1).

   A generated path is described by its units. The unit Op s is an
   operation at step s. The unit Child s i is child i of a split at step s.
   render_legacy gives the path of the legacy splitter and render_compact
   gives the path of the compacted splitter. fuse maps a legacy path to its
   compacted path.

   Results:
   - fuse_render: fuse maps the legacy path of every unit list to the
     compacted path of the same unit list.
   - fuse_order_isomorphism: two compacted paths compare like their legacy
     paths. The scheduler orders every frontier in the same way, so the
     schedule, the event log and the post-state root do not change.
   - fuse_injective: distinct legacy paths have distinct compacted paths.
   - fuse_halves_participant_depth: a participant path below k splits has
     k segments instead of 2k. An operation path below k splits has k + 1
     segments instead of 2k + 1. funding_flow_depth computes 513 instead of
     1,025 for k = 512.
   - naive_fusion_collides: the fusion of (s, 1) (i, 0) into (s, i) gives
     an operation and the first child of a split the same path. Each
     participant uses its step counter for its operations and its splits,
     so the Rust program never makes that pair. The chosen fusion does not
     depend on that invariant.

   Rust correspondence: ReductionContext::split and next_operation in
   rholang/src/rust/interpreter/deterministic_reduction.rs. path_compare is
   the Vec<(u64, u64)> order that CausalPath implements in
   rspace++/src/rspace/operation_context.rs. The extracted tests are
   compacted_paths_preserve_vec_order, funding_flow_depth_at_most_513
   (deterministic_reduction.rs) and compacted_schedule_equals_legacy_schedule
   (native_runtime/checked_operations/trace/replay/session/execution/tests.rs). *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Definition segment := (nat * nat)%type.
Definition path := list segment.

(* The order of (u64, u64): the step first, then the second component. *)
Definition seg_compare (x y : segment) : comparison :=
  match Nat.compare (fst x) (fst y) with
  | Eq => Nat.compare (snd x) (snd y)
  | other => other
  end.

(* The order of Vec<(u64, u64)>: lexicographic, and a proper prefix is
   smaller. *)
Fixpoint path_compare (xs ys : path) : comparison :=
  match xs, ys with
  | [], [] => Eq
  | [], _ :: _ => Lt
  | _ :: _, [] => Gt
  | x :: xs', y :: ys' =>
      match seg_compare x y with
      | Eq => path_compare xs' ys'
      | other => other
      end
  end.

Inductive path_unit := Op (step : nat) | Child (step index : nat).

Fixpoint render_legacy (units : list path_unit) : path :=
  match units with
  | [] => []
  | Op s :: rest => (s, 0) :: render_legacy rest
  | Child s i :: rest => (s, 1) :: (i, 0) :: render_legacy rest
  end.

Fixpoint render_compact (units : list path_unit) : path :=
  match units with
  | [] => []
  | Op s :: rest => (s, 0) :: render_compact rest
  | Child s i :: rest => (s, S i) :: render_compact rest
  end.

Fixpoint fuse (p : path) : path :=
  match p with
  | [] => []
  | (s, 1) :: (i, 0) :: rest => (s, S i) :: fuse rest
  | segment :: rest => segment :: fuse rest
  end.

Definition legacy_path (p : path) : Prop := exists units, p = render_legacy units.

Lemma fuse_render : forall units, fuse (render_legacy units) = render_compact units.
Proof.
  induction units as [|[s|s i] rest IH]; cbn; [reflexivity | |]; rewrite IH; reflexivity.
Qed.

Lemma seg_compare_eq : forall x y, seg_compare x y = Eq <-> x = y.
Proof.
  intros [s b] [t c]; unfold seg_compare; cbn [fst snd].
  destruct (Nat.compare s t) eqn:E.
  - apply Nat.compare_eq_iff in E; subst t.
    rewrite Nat.compare_eq_iff.
    split; [intros ->; reflexivity | intros H; injection H; auto].
  - split; [discriminate|].
    intros H; injection H as -> ->; rewrite Nat.compare_refl in E; discriminate.
  - split; [discriminate|].
    intros H; injection H as -> ->; rewrite Nat.compare_refl in E; discriminate.
Qed.

Lemma path_compare_eq : forall p q, path_compare p q = Eq <-> p = q.
Proof.
  induction p as [|x p IH]; intros [|y q]; cbn [path_compare].
  - split; reflexivity.
  - split; discriminate.
  - split; discriminate.
  - destruct (seg_compare x y) eqn:E.
    + apply seg_compare_eq in E; subst y.
      rewrite IH.
      split; [intros ->; reflexivity | intros H; injection H; auto].
    + split; [discriminate|].
      intros H; injection H as Head _; subst y.
      assert (seg_compare x x = Eq) as Same by (apply seg_compare_eq; reflexivity).
      congruence.
    + split; [discriminate|].
      intros H; injection H as Head _; subst y.
      assert (seg_compare x x = Eq) as Same by (apply seg_compare_eq; reflexivity).
      congruence.
Qed.

(* The two renderings compare alike for all unit lists. The proof needs no
   well-formedness premise: an operation unit and a child unit with the same
   step are decided at their first segment in both renderings. *)
Theorem render_order_isomorphism : forall us vs,
  path_compare (render_compact us) (render_compact vs) =
  path_compare (render_legacy us) (render_legacy vs).
Proof.
  induction us as [|u us IH]; intros [|v vs].
  - reflexivity.
  - destruct v; reflexivity.
  - destruct u; reflexivity.
  - destruct u as [s|s i], v as [t|t j];
      cbn [render_compact render_legacy path_compare]; unfold seg_compare;
      cbn [fst snd].
    + destruct (Nat.compare s t); [apply IH | reflexivity | reflexivity].
    + destruct (Nat.compare s t); reflexivity.
    + destruct (Nat.compare s t); reflexivity.
    + destruct (Nat.compare s t); [|reflexivity|reflexivity].
      cbn [Nat.compare path_compare]; unfold seg_compare; cbn [fst snd Nat.compare].
      destruct (Nat.compare i j); [apply IH | reflexivity | reflexivity].
Qed.

Theorem fuse_order_isomorphism : forall p q,
  legacy_path p -> legacy_path q ->
  path_compare (fuse p) (fuse q) = path_compare p q.
Proof.
  intros p q [us ->] [vs ->].
  rewrite !fuse_render.
  apply render_order_isomorphism.
Qed.

Theorem fuse_injective : forall p q,
  legacy_path p -> legacy_path q -> fuse p = fuse q -> p = q.
Proof.
  intros p q Hp Hq Fused.
  apply path_compare_eq.
  rewrite <- (fuse_order_isomorphism p q Hp Hq).
  apply path_compare_eq.
  exact Fused.
Qed.

Fixpoint children (units : list path_unit) : nat :=
  match units with
  | [] => 0
  | Op _ :: rest => children rest
  | Child _ _ :: rest => S (children rest)
  end.

Definition is_child (unit : path_unit) : Prop :=
  match unit with
  | Child _ _ => True
  | Op _ => False
  end.

Lemma length_render_compact : forall us, length (render_compact us) = length us.
Proof.
  induction us as [|[s|s i] us IH]; cbn; congruence.
Qed.

Lemma length_render_legacy : forall us,
  length (render_legacy us) = length us + children us.
Proof.
  induction us as [|[s|s i] us IH]; cbn; lia.
Qed.

Lemma children_app : forall us vs, children (us ++ vs) = children us + children vs.
Proof.
  induction us as [|[s|s i] us IH]; intros vs; cbn; [reflexivity | apply IH | rewrite IH; reflexivity].
Qed.

Lemma children_of_children : forall us, Forall is_child us -> children us = length us.
Proof.
  intros us All.
  induction All as [|[s|s i] us Child Rest IH]; cbn in *; [reflexivity | contradiction | lia].
Qed.

Theorem fuse_halves_participant_depth : forall us,
  Forall is_child us ->
  length (render_legacy us) = 2 * length (render_compact us) /\
  (forall s,
    length (render_legacy (us ++ [Op s])) = 2 * length (render_compact us) + 1 /\
    length (render_compact (us ++ [Op s])) = length (render_compact us) + 1).
Proof.
  intros us All.
  pose proof (children_of_children us All) as Count.
  rewrite length_render_compact.
  split.
  - rewrite length_render_legacy; lia.
  - intros s.
    rewrite length_render_legacy, length_render_compact, children_app, !length_app.
    cbn; lia.
Qed.

Example funding_flow_depth :
  length (render_compact (repeat (Child 0 0) 512 ++ [Op 0])) = 513 /\
  length (render_legacy (repeat (Child 0 0) 512 ++ [Op 0])) = 1025.
Proof.
  split; vm_compute; reflexivity.
Qed.

Fixpoint render_naive (units : list path_unit) : path :=
  match units with
  | [] => []
  | Op s :: rest => (s, 0) :: render_naive rest
  | Child s i :: rest => (s, i) :: render_naive rest
  end.

Theorem naive_fusion_collides :
  render_naive [Op 3] = render_naive [Child 3 0] /\
  render_legacy [Op 3] <> render_legacy [Child 3 0] /\
  path_compare (render_legacy [Op 3]) (render_legacy [Child 3 0]) = Lt /\
  path_compare (render_naive [Op 3]) (render_naive [Child 3 0]) = Eq.
Proof.
  split; [reflexivity|].
  split; [discriminate|].
  split; reflexivity.
Qed.
