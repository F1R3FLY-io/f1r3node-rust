From Stdlib Require Import Bool List Lia.
Import ListNotations.

(** Each node carries the result of the local closed-literal check.  A binary
    node represents one collection entry and its remaining siblings; this
    first-child/next-sibling encoding covers finite n-ary value trees.  The
    executable, free-variable, unforgeable, remainder and malformed cases all
    have [locally_closed = false]. *)
Inductive value_tree : Type :=
| Leaf (locally_closed : bool)
| Pair (locally_closed : bool) (first rest : value_tree).

Fixpoint nodes (tree : value_tree) : nat :=
  match tree with
  | Leaf _ => 1
  | Pair _ first rest => 1 + nodes first + nodes rest
  end.

Fixpoint pure (tree : value_tree) : bool :=
  match tree with
  | Leaf local => local
  | Pair local first rest => local && pure first && pure rest
  end.

Fixpoint frontier_nodes (frontier : list value_tree) : nat :=
  match frontier with
  | [] => 0
  | tree :: rest => nodes tree + frontier_nodes rest
  end.

Lemma leaf_step_decreases : forall local rest,
  frontier_nodes rest < frontier_nodes (Leaf local :: rest).
Proof. intros. simpl. lia. Qed.

Lemma pair_step_decreases : forall local first siblings rest,
  frontier_nodes (first :: siblings :: rest) <
  frontier_nodes (Pair local first siblings :: rest).
Proof. intros. simpl. lia. Qed.

(** The fuel is the exact number of nodes left to inspect.  [frontier] is the
    heap continuation, so neither evaluation nor the proof relies on the
    native call stack growing with the depth of a registry value. *)
Fixpoint scan (fuel : nat) (frontier : list value_tree) : bool :=
  match frontier with
  | [] => true
  | tree :: rest =>
      match fuel with
      | 0 => false
      | S remaining =>
          match tree with
          | Leaf local => local && scan remaining rest
          | Pair local first siblings =>
              local && scan remaining (first :: siblings :: rest)
          end
      end
  end.

Lemma scan_exact : forall fuel frontier,
  frontier_nodes frontier <= fuel ->
  scan fuel frontier = forallb pure frontier.
Proof.
  induction fuel as [|fuel IH]; intros frontier bound.
  - destruct frontier as [|tree rest]; simpl in *; auto.
    destruct tree; simpl in bound; lia.
  - destruct frontier as [|tree rest]; simpl; auto.
    destruct tree as [local | local first siblings]; simpl in *.
    + rewrite IH by lia. reflexivity.
    + assert (frontier_nodes (first :: siblings :: rest) <= fuel) as child_bound
        by (simpl in *; lia).
      rewrite (IH _ child_bound).
      simpl.
      destruct local, (pure first), (pure siblings), (forallb pure rest); reflexivity.
Qed.

Definition admits (tree : value_tree) : bool := scan (nodes tree) [tree].

Theorem admission_exactly_closed : forall tree,
  admits tree = true <-> pure tree = true.
Proof.
  intro tree. unfold admits.
  rewrite scan_exact by (simpl; lia).
  simpl. rewrite andb_true_r. tauto.
Qed.

Theorem executable_node_cannot_be_admitted : forall first rest,
  admits (Pair false first rest) = false.
Proof.
  intros first rest.
  apply Bool.not_true_is_false. intro admitted.
  apply admission_exactly_closed in admitted.
  simpl in admitted. discriminate.
Qed.

Theorem accepted_value_has_finite_frontier : forall tree,
  admits tree = true -> frontier_nodes [tree] = nodes tree.
Proof. intros tree _. simpl. lia. Qed.

Print Assumptions scan_exact.
Print Assumptions leaf_step_decreases.
Print Assumptions pair_step_decreases.
Print Assumptions admission_exactly_closed.
Print Assumptions executable_node_cannot_be_admitted.
Print Assumptions accepted_value_has_finite_frontier.
