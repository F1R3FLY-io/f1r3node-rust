From Stdlib Require Import List NArith.
From CostAccountedRho Require Import StackSafeSignatureSort.
From StackSafePDA Require Import StackSafePDA SorterSimulation.
Import ListNotations.
Set Implicit Arguments.

Section CostAccounting.
Context {Payload Value : Type}.

Inductive label :=
| Ordinary : Payload -> label
| Signed : Payload -> option N -> label
| Stack : Payload -> label
| Bind : Payload -> label
| Process : Payload -> label.

Variable algebra : label -> list Value -> Value.

Definition optional_child (child : option (@tree label)) : list (@tree label) :=
  match child with None => [] | Some subject => [subject] end.

Definition node (tag : label) (children : list (@tree label)) :=
  Node tag (forest_of_list children).

Lemma fold_children : forall children,
  fold_forest algebra (forest_of_list children) = map (fold_tree algebra) children.
Proof.
  induction children; simpl; [reflexivity|now rewrite IHchildren].
Qed.

Theorem ordered_children_reach_the_same_algebra : forall tag children,
  result algebra ([Visit (node tag children)], []) =
  Some [algebra tag (map (fold_tree algebra) children)].
Proof.
  intros. rewrite sorter_initial_result. unfold node. simpl. now rewrite fold_children.
Qed.

Theorem signed_body_signature_and_price_are_preserved : forall payload price body signature,
  result algebra ([Visit (node (Signed payload price)
      (optional_child body ++ optional_child signature))], []) =
  Some [algebra (Signed payload price)
      (map (fold_tree algebra) (optional_child body) ++
       map (fold_tree algebra) (optional_child signature))].
Proof.
  intros. rewrite ordered_children_reach_the_same_algebra, map_app. reflexivity.
Qed.

Theorem stack_cells_keep_source_order : forall payload cells,
  result algebra ([Visit (node (Stack payload) cells)], []) =
  Some [algebra (Stack payload) (map (fold_tree algebra) cells)].
Proof. intros. apply ordered_children_reach_the_same_algebra. Qed.

Theorem receive_signature_follows_patterns_and_source : forall payload patterns source signature,
  result algebra ([Visit (node (Bind payload)
      (patterns ++ [source] ++ optional_child signature))], []) =
  Some [algebra (Bind payload)
      (map (fold_tree algebra) patterns ++
       [fold_tree algebra source] ++
       map (fold_tree algebra) (optional_child signature))].
Proof.
  intros. rewrite ordered_children_reach_the_same_algebra, !map_app. reflexivity.
Qed.

Definition par_children
  (sends receives exprs news matches bundles connectives unforgeables conditionals
   signed_terms stacks : list (@tree label)) :=
  sends ++ receives ++ exprs ++ news ++ matches ++ bundles ++ connectives ++
  unforgeables ++ conditionals ++ signed_terms ++ stacks.

Theorem par_cost_families_follow_conditionals : forall payload
  sends receives exprs news matches bundles connectives unforgeables conditionals signed_terms stacks,
  result algebra ([Visit (node (Process payload)
      (par_children sends receives exprs news matches bundles connectives unforgeables
        conditionals signed_terms stacks))], []) =
  Some [algebra (Process payload)
    (map (fold_tree algebra) sends ++ map (fold_tree algebra) receives ++
     map (fold_tree algebra) exprs ++ map (fold_tree algebra) news ++
     map (fold_tree algebra) matches ++ map (fold_tree algebra) bundles ++
     map (fold_tree algebra) connectives ++ map (fold_tree algebra) unforgeables ++
     map (fold_tree algebra) conditionals ++ map (fold_tree algebra) signed_terms ++
     map (fold_tree algebra) stacks)].
Proof.
  intros. rewrite ordered_children_reach_the_same_algebra.
  unfold par_children. now rewrite !map_app.
Qed.
End CostAccounting.

Theorem signature_flatten_keeps_every_leaf_and_tie_order :
  forall (LeafValue Result : Type) (decode : nat -> LeafValue)
    (normalize : list LeafValue -> Result) signatures,
  option_map (fun leaves => normalize (map decode leaves))
    (flatten_machine (forest_size signatures) signatures []) =
  Some (normalize (map decode (flatten_forest signatures))).
Proof.
  intros. rewrite flatten_machine_steps_are_linear. reflexivity.
Qed.

Print Assumptions signed_body_signature_and_price_are_preserved.
Print Assumptions stack_cells_keep_source_order.
Print Assumptions receive_signature_follows_patterns_and_source.
Print Assumptions par_cost_families_follow_conditionals.
Print Assumptions signature_flatten_keeps_every_leaf_and_tie_order.
