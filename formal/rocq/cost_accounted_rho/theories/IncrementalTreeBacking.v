(* C13 (epic 8946, B1 Phase A; decision record DR-77): tree growth charges
   the increment of the tree backing.

   A host-work budget only accumulates usage: a reservation is never
   released. The runtime budget keeps several B-trees (registries, the
   authority frontier, events) that grow one insert or one batch at a time.

   tree_bytes n bounds the bytes of every node of a B-tree with n entries
   (one node per five entries, NativeCheckpointBacking.checkpoint_node_count).
   The legacy charge reserved tree_bytes (n + 1) on every insert, so n inserts
   reserved sum_k tree_bytes k. The C13 charge reserves the increment
   tree_bytes (n + 1) - tree_bytes n.

   Results:
   - incremental_charges_telescope: for any sequence of batches, the C13
     charges add up to project (final) - project (initial), for every
     monotone projection (bytes or operations).
   - incremental_bytes_from_empty and incremental_operations_from_empty:
     from an empty tree, the charges add up to the backing of the final tree.
   - incremental_prefix_covers_nodes: after every prefix of single inserts,
     the cumulative C13 reservation covers every node of any B-tree with that
     many entries. For an insert-only tree, inserts never free nodes, so it
     also covers every node that the inserts allocated.
   - charges_cover_retained_with_removals: for any sequence of inserts and
     removals, every prefix has charged at least the backing of the current
     tree. SearchStateBytes counts retained logical bytes, so this is the
     soundness condition for trees that also remove entries.
   - legacy_cumulative_quadratic and legacy_example: the legacy charge grows
     quadratically, while the C13 charge stays linear.

   Rust correspondence: shared/src/rust/collection_backing.rs (tree_backing,
   tree_growth); rholang/src/rust/interpreter/accounting/mod.rs
   (reserve_registry_insert, reserve_tree_birth, reserve_tree_batch);
   rspace++/src/rspace/replay_rspace/native_candidate/metered.rs
   (prepare_metered_produce_counter). The extracted property tests are in the tree_growth_tests
   module of accounting/mod.rs. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
From CostAccountedRho Require Import NativeCheckpointBacking.
Import ListNotations.

Lemma tree_nodes_bound_monotone : forall a b,
  a <= b -> tree_nodes_bound a <= tree_nodes_bound b.
Proof.
  intros [| a] [| b] ab; cbn [tree_nodes_bound]; try lia.
  pose proof (Nat.Div0.div_le_mono a b 5 ltac:(lia)). lia.
Qed.

Lemma five_nodes_cover_entries : forall entries, entries <= 5 * tree_nodes_bound entries.
Proof.
  intros [| entries]; cbn [tree_nodes_bound]; [lia |].
  pose proof (Nat.div_mod entries 5 ltac:(lia)).
  pose proof (Nat.mod_upper_bound entries 5 ltac:(lia)).
  lia.
Qed.

Section Backing.
  Variable node_bytes : nat.

  Definition tree_bytes (entries : nat) : nat := tree_nodes_bound entries * node_bytes.

  Definition tree_operations (entries : nat) : nat := 2 * (entries + tree_nodes_bound entries).

  Lemma tree_bytes_monotone : forall a b, a <= b -> tree_bytes a <= tree_bytes b.
  Proof.
    intros a b ab. unfold tree_bytes.
    apply Nat.mul_le_mono_r. apply tree_nodes_bound_monotone. exact ab.
  Qed.

  Lemma tree_operations_monotone : forall a b, a <= b -> tree_operations a <= tree_operations b.
  Proof.
    intros a b ab. unfold tree_operations.
    pose proof (tree_nodes_bound_monotone a b ab). lia.
  Qed.

  (* The C13 charge of a batch of [additional] entries added to a tree of
     [entries] entries. *)
  Definition growth (project : nat -> nat) (entries additional : nat) : nat :=
    project (entries + additional) - project entries.

  Fixpoint batch_total (project : nat -> nat) (entries : nat) (batches : list nat) : nat :=
    match batches with
    | [] => 0
    | additional :: rest =>
        growth project entries additional + batch_total project (entries + additional) rest
    end.

  Theorem incremental_charges_telescope : forall project,
    (forall a b, a <= b -> project a <= project b) ->
    forall batches entries,
      batch_total project entries batches =
      project (entries + fold_right Nat.add 0 batches) - project entries.
  Proof.
    intros project monotone batches.
    induction batches as [| additional rest IH]; intros entries; simpl.
    - rewrite Nat.add_0_r. lia.
    - rewrite IH. unfold growth.
      pose proof (monotone entries (entries + additional) ltac:(lia)).
      pose proof (monotone (entries + additional)
                    (entries + additional + fold_right Nat.add 0 rest) ltac:(lia)).
      replace (entries + (additional + fold_right Nat.add 0 rest))
        with (entries + additional + fold_right Nat.add 0 rest) by lia.
      lia.
  Qed.

  Corollary incremental_bytes_from_empty : forall batches,
    batch_total tree_bytes 0 batches = tree_bytes (fold_right Nat.add 0 batches).
  Proof.
    intros batches.
    rewrite (incremental_charges_telescope tree_bytes tree_bytes_monotone batches 0).
    unfold tree_bytes at 2. simpl. lia.
  Qed.

  Corollary incremental_operations_from_empty : forall batches,
    batch_total tree_operations 0 batches = tree_operations (fold_right Nat.add 0 batches).
  Proof.
    intros batches.
    rewrite (incremental_charges_telescope tree_operations tree_operations_monotone batches 0).
    unfold tree_operations at 2. simpl. lia.
  Qed.

  Lemma sum_repeat_one : forall count, fold_right Nat.add 0 (repeat 1 count) = count.
  Proof. induction count as [| count IH]; simpl; [reflexivity | rewrite IH; reflexivity]. Qed.

  (* After [entries] single inserts, the cumulative C13 reservation covers every
     node of any B-tree with [entries] entries whose root holds at least one
     entry and whose other nodes hold at least five. *)
  Theorem incremental_prefix_covers_nodes : forall root occupancies,
    1 <= root -> Forall (fun count => 5 <= count) occupancies ->
    S (length occupancies) * node_bytes <=
      batch_total tree_bytes 0 (repeat 1 (root + fold_right Nat.add 0 occupancies)).
  Proof.
    intros root occupancies root_live occupied.
    rewrite incremental_bytes_from_empty, sum_repeat_one.
    unfold tree_bytes.
    apply Nat.mul_le_mono_r.
    exact (checkpoint_node_count root occupancies root_live occupied).
  Qed.

  (* Trees with removals. SearchStateBytes counts canonical logical bytes
     retained (docs/casper/theory/host-work-budget.md), not allocator
     activity. An insert at size n charges growth n 1; a removal charges
     nothing and refunds nothing. *)
  Inductive tree_op := Insert | Remove.

  Fixpoint run_ops (size charge : nat) (ops : list tree_op) : nat * nat :=
    match ops with
    | [] => (size, charge)
    | Insert :: rest => run_ops (S size) (charge + growth tree_bytes size 1) rest
    | Remove :: rest => run_ops (pred size) charge rest
    end.

  Lemma run_ops_invariant : forall ops size maximum charge,
    size <= maximum -> tree_bytes maximum <= charge ->
    exists maximum',
      fst (run_ops size charge ops) <= maximum' /\
      tree_bytes maximum' <= snd (run_ops size charge ops).
  Proof.
    induction ops as [| op rest IH]; intros size maximum charge below covered; simpl.
    - exists maximum. split; assumption.
    - destruct op.
      + unfold growth. rewrite Nat.add_1_r.
        destruct (Nat.le_gt_cases (S size) maximum) as [inside | beyond].
        * apply (IH (S size) maximum); [exact inside |]. lia.
        * apply (IH (S size) (S size)); [lia |].
          assert (size = maximum) by lia. subst maximum.
          pose proof (tree_bytes_monotone size (S size) ltac:(lia)). lia.
      + apply (IH (pred size) maximum); [lia | exact covered].
  Qed.

  (* Every prefix of inserts and removals from an empty tree has charged at
     least the backing of the current tree. *)
  Theorem charges_cover_retained_with_removals : forall ops,
    tree_bytes (fst (run_ops 0 0 ops)) <= snd (run_ops 0 0 ops).
  Proof.
    intros ops.
    destruct (run_ops_invariant ops 0 0 0 ltac:(lia) ltac:(unfold tree_bytes; simpl; lia))
      as [maximum [below covered]].
    pose proof (tree_bytes_monotone _ _ below). lia.
  Qed.

  (* The legacy charge reserved the whole tree on every single insert. *)
  Fixpoint legacy_nodes (inserts : nat) : nat :=
    match inserts with
    | 0 => 0
    | S rest => tree_nodes_bound (S rest) + legacy_nodes rest
    end.

  Definition legacy_bytes (inserts : nat) : nat := legacy_nodes inserts * node_bytes.

  Theorem legacy_cumulative_quadratic : forall inserts,
    inserts * (inserts + 1) <= 10 * legacy_nodes inserts.
  Proof.
    induction inserts as [| inserts IH]; cbn [legacy_nodes]; [lia |].
    pose proof (five_nodes_cover_entries (S inserts)). nia.
  Qed.

  Theorem incremental_nodes_linear : forall inserts,
    tree_nodes_bound inserts <= 1 + inserts / 5.
  Proof.
    intros [| inserts]; cbn [tree_nodes_bound]; [lia |].
    pose proof (Nat.Div0.div_le_mono inserts (S inserts) 5 ltac:(lia)).
    lia.
  Qed.
End Backing.

(* For 100 inserts the legacy charge reserved 1,050 nodes; the tree needs
   20. (The Rust test checks 1,000 inserts: 100,500 nodes against 200.) *)
Example legacy_example :
  legacy_nodes 100 = 1050 /\ tree_nodes_bound 100 = 20.
Proof. split; vm_compute; reflexivity. Qed.

Print Assumptions incremental_charges_telescope.
Print Assumptions incremental_bytes_from_empty.
Print Assumptions incremental_operations_from_empty.
Print Assumptions incremental_prefix_covers_nodes.
Print Assumptions legacy_cumulative_quadratic.
Print Assumptions incremental_nodes_linear.
Print Assumptions charges_cover_retained_with_removals.
Print Assumptions legacy_example.
