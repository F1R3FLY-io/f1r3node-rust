From Stdlib Require Import Lists.List.

Import ListNotations.

(** A stack contains an authored prefix and an ordered forest of continuations.
    The mutual representation models [CostStack.cells] and [parallel_tail]
    without imposing a fixed branch arity. *)
Inductive stack_tree : Type :=
| StackNode : list nat -> stack_forest -> stack_tree
with stack_forest : Type :=
| NoChildren : stack_forest
| NextChild : stack_tree -> stack_forest -> stack_forest.

Fixpoint flat_cells (tree : stack_tree) : list nat :=
  match tree with
  | StackNode cells children => cells ++ flat_forest children
  end
with flat_forest (children : stack_forest) : list nat :=
  match children with
  | NoChildren => []
  | NextChild child rest => flat_cells child ++ flat_forest rest
  end.

(** Canonicalizing each signature changes cell values, not the authored
    branch forest. The native inventory decoder must implement this structural
    map rather than replacing a tree by its flattened cells. *)
Fixpoint map_tree (f : nat -> nat) (tree : stack_tree) : stack_tree :=
  match tree with
  | StackNode cells children => StackNode (map f cells) (map_forest f children)
  end
with map_forest (f : nat -> nat) (children : stack_forest) : stack_forest :=
  match children with
  | NoChildren => NoChildren
  | NextChild child rest => NextChild (map_tree f child) (map_forest f rest)
  end.

Scheme stack_tree_induction := Induction for stack_tree Sort Prop
with stack_forest_induction := Induction for stack_forest Sort Prop.
Combined Scheme stack_induction from stack_tree_induction, stack_forest_induction.

Theorem map_tree_preserves_flat_order :
  (forall tree f, flat_cells (map_tree f tree) = map f (flat_cells tree)) /\
  (forall children f, flat_forest (map_forest f children) =
                      map f (flat_forest children)).
Proof.
  apply stack_induction.
  - intros cells children IH f. simpl. rewrite IH. rewrite map_app. reflexivity.
  - intros f. reflexivity.
  - intros child IHchild rest IHrest f.
    simpl.
    rewrite IHchild.
    rewrite IHrest.
    rewrite map_app.
    reflexivity.
Qed.

Record birth := {
  source_hash : nat;
  event_cells : list nat;
  authored_tree : stack_tree
}.

Definition well_formed (value : birth) : Prop :=
  event_cells value = flat_cells (authored_tree value).

(** The producer's native claim and rooted-state checks require tree
    identity in addition to the legacy source hash and transfer-event list. *)
Definition admitted (observed claimed : birth) (rooted : stack_tree) : Prop :=
  well_formed observed /\
  well_formed claimed /\
  source_hash observed = source_hash claimed /\
  event_cells observed = event_cells claimed /\
  authored_tree observed = authored_tree claimed /\
  authored_tree observed = rooted.

Theorem admitted_claim_is_the_rooted_tree : forall observed claimed rooted,
  admitted observed claimed rooted -> authored_tree claimed = rooted.
Proof.
  intros observed claimed rooted [_ [_ [_ [_ [same rooted_same]]]]].
  rewrite <- same.
  exact rooted_same.
Qed.

Theorem admitted_claim_has_exact_flat_events : forall observed claimed rooted,
  admitted observed claimed rooted ->
  event_cells claimed = flat_cells rooted.
Proof.
  intros observed claimed rooted admission.
  pose proof (admitted_claim_is_the_rooted_tree
                observed claimed rooted admission) as exact_tree.
  destruct admission as [_ [valid_claim _]].
  unfold well_formed in valid_claim.
  rewrite valid_claim, exact_tree.
  reflexivity.
Qed.

Definition split_branches : stack_tree :=
  StackNode [0]
    (NextChild (StackNode [1] NoChildren)
      (NextChild (StackNode [2] NoChildren) NoChildren)).

Definition regrouped_branches : stack_tree :=
  StackNode [0]
    (NextChild (StackNode [1; 2] NoChildren)
      (NextChild (StackNode [] NoChildren) NoChildren)).

Theorem flattening_loses_branch_identity :
  flat_cells split_branches = flat_cells regrouped_branches /\
  split_branches <> regrouped_branches.
Proof.
  split.
  - reflexivity.
  - intro same; discriminate same.
Qed.

Theorem flat_only_admission_is_unsound :
  exists observed claimed,
    well_formed observed /\ well_formed claimed /\
    source_hash observed = source_hash claimed /\
    event_cells observed = event_cells claimed /\
    authored_tree observed <> authored_tree claimed.
Proof.
  exists {| source_hash := 7; event_cells := [0; 1; 2];
            authored_tree := split_branches |}.
  exists {| source_hash := 7; event_cells := [0; 1; 2];
            authored_tree := regrouped_branches |}.
  repeat split; try reflexivity.
  intro same; discriminate same.
Qed.

Print Assumptions admitted_claim_is_the_rooted_tree.
Print Assumptions admitted_claim_has_exact_flat_events.
Print Assumptions flattening_loses_branch_identity.
Print Assumptions flat_only_admission_is_unsound.
Print Assumptions map_tree_preserves_flat_order.
