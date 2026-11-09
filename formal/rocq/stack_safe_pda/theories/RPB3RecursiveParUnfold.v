From Stdlib Require Import List Arith.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3SemanticReducer EPM1ACTree03
  RPB3RecursiveAuxiliarySourceReference.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Semantic := RPB3SemanticReducer.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.

(** The source-reference family consults its nested-Par argument only by
    applying it, so two pointwise-equal arguments produce equal references.
    That is the pointwise premise [Fix_eq] requires, so the well-founded [Fix]
    unfolds without any assumption of function extensionality. *)

(** [Source.source_tree] nests its children under [list], so the generated
    [source_tree_ind] has no hypothesis for them.  This principle supplies one
    per child, through the same nested [fix] shape the reference itself
    recurses with. *)
Fixpoint source_tree_nested_ind
    (property : Source.source_tree -> Prop)
    (on_scalar : forall span field,
      property (Source.SourceScalar span field))
    (on_message : forall span field children,
      Forall property children ->
      property (Source.SourceMessage span field children))
    (on_auxiliary : forall span field,
      property (Source.SourceAuxiliary span field))
    (node : Source.source_tree) {struct node} : property node :=
  match node return property node with
  | Source.SourceScalar span field => on_scalar span field
  | Source.SourceMessage span field children =>
      on_message span field children
        ((fix visit_members (remaining : list Source.source_tree)
            : Forall property remaining :=
            match remaining return Forall property remaining with
            | [] => Forall_nil property
            | first :: rest =>
                Forall_cons first
                  (source_tree_nested_ind property on_scalar on_message
                    on_auxiliary first)
                  (visit_members rest)
            end) children)
  | Source.SourceAuxiliary span field => on_auxiliary span field
  end.

Lemma reduce_map_par_ext :
  forall (A : Type) (par_left par_right : list nat -> option (bool * A)),
    (forall body, par_left body = par_right body) ->
    forall source,
      Source.reduce_map_par par_left source =
      Source.reduce_map_par par_right source.
Proof.
  intros A par_left par_right same source.
  destruct source as [[span body]|]; unfold Source.reduce_map_par.
  - now rewrite same.
  - reflexivity.
Qed.

Lemma reduce_typed_map_entry_ext :
  forall (A : Type) (par_left par_right : list nat -> option (bool * A)),
    (forall body, par_left body = par_right body) ->
    forall entry,
      Source.reduce_typed_map_entry par_left entry =
      Source.reduce_typed_map_entry par_right entry.
Proof.
  intros A par_left par_right same entry.
  unfold Source.reduce_typed_map_entry.
  now rewrite (reduce_map_par_ext A par_left par_right same).
Qed.

Lemma reduce_source_map_node_ext :
  forall (A : Type) bytes program slot
      (par_left par_right : list nat -> option (bool * A)),
    (forall body, par_left body = par_right body) ->
    forall node,
      Source.reduce_source_map_node bytes program slot par_left node =
      Source.reduce_source_map_node bytes program slot par_right node.
Proof.
  intros A bytes program slot par_left par_right same node.
  unfold Source.reduce_source_map_node.
  destruct (Source.refine_string_map_entry bytes program node)
    as [typed|]; [|reflexivity].
  destruct (Schema.descriptor_slot (Source.typed_map_outer_field typed)
    =? slot); [|reflexivity].
  rewrite (reduce_typed_map_entry_ext A par_left par_right same typed).
  reflexivity.
Qed.

Lemma recursive_auxiliary_reference_ext :
  forall (par_left par_right :
      list nat -> option (bool * Semantic.semantic_summary)),
    (forall body, par_left body = par_right body) ->
    forall hash_fn bytes program slot node,
      Recursive.recursive_auxiliary_reference par_left hash_fn bytes
        program slot node =
      Recursive.recursive_auxiliary_reference par_right hash_fn bytes
        program slot node.
Proof.
  intros par_left par_right same hash_fn bytes program slot node.
  destruct node as [span field|span field children|span field];
    [reflexivity|reflexivity|].
  unfold Recursive.recursive_auxiliary_reference.
  destruct (Recursive.checked_generated_raw bytes program
    (Source.SourceAuxiliary span field)) as [raw|]; [|reflexivity].
  destruct (Schema.descriptor_slot field =? slot); [|reflexivity].
  destruct (Schema.descriptor_payload field).
  - reflexivity.
  - reflexivity.
  - rewrite (reduce_source_map_node_ext Semantic.semantic_summary bytes
      program slot par_left par_right same
      (Source.SourceAuxiliary span field)).
    reflexivity.
  - destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)))
      as [[snapshot trie]|]; [|reflexivity].
    rewrite (map_ext par_left par_right same).
    reflexivity.
Qed.

Lemma recursive_source_children_agree_on_members :
  forall (par_left par_right :
      list nat -> option (bool * Semantic.semantic_summary))
      hash_fn bytes nodes,
    Forall (fun node =>
      forall program,
        Recursive.recursive_source_reference par_left hash_fn bytes
          program node =
        Recursive.recursive_source_reference par_right hash_fn bytes
          program node) nodes ->
    forall program,
      Recursive.recursive_source_children par_left hash_fn bytes
        program nodes =
      Recursive.recursive_source_children par_right hash_fn bytes
        program nodes.
Proof.
  intros par_left par_right hash_fn bytes nodes members.
  induction members as [|first rest first_agrees _ IH]; intro program.
  - reflexivity.
  - cbn [Recursive.recursive_source_children].
    rewrite first_agrees, IH.
    reflexivity.
Qed.

Lemma recursive_source_reference_ext :
  forall (par_left par_right :
      list nat -> option (bool * Semantic.semantic_summary)),
    (forall body, par_left body = par_right body) ->
    forall hash_fn bytes node program,
      Recursive.recursive_source_reference par_left hash_fn bytes
        program node =
      Recursive.recursive_source_reference par_right hash_fn bytes
        program node.
Proof.
  intros par_left par_right same hash_fn bytes node.
  induction node as [span field|span field children members|span field]
    using source_tree_nested_ind; intro program.
  - reflexivity.
  - cbn [Recursive.recursive_source_reference].
    destruct (Recursive.checked_generated_raw bytes program
      (Source.SourceMessage span field children))
      as [raw|]; [|reflexivity].
    destruct (Schema.descriptor_child_type_id field)
      as [type_id|]; [|reflexivity].
    destruct (Schema.lookup_projected_program type_id [] [])
      as [child_program|]; [|reflexivity].
    rewrite !Recursive.local_recursive_children_are_global_children.
    rewrite (recursive_source_children_agree_on_members par_left par_right
      hash_fn bytes children members child_program).
    reflexivity.
  - cbn [Recursive.recursive_source_reference].
    apply recursive_auxiliary_reference_ext.
    exact same.
Qed.

Lemma recursive_source_children_ext :
  forall (par_left par_right :
      list nat -> option (bool * Semantic.semantic_summary)),
    (forall body, par_left body = par_right body) ->
    forall hash_fn bytes program nodes,
      Recursive.recursive_source_children par_left hash_fn bytes
        program nodes =
      Recursive.recursive_source_children par_right hash_fn bytes
        program nodes.
Proof.
  intros par_left par_right same hash_fn bytes program nodes.
  apply recursive_source_children_agree_on_members.
  apply Forall_forall.
  intros node _ node_program.
  exact (recursive_source_reference_ext par_left par_right same
    hash_fn bytes node node_program).
Qed.

Lemma guarded_recursive_par_child_ext :
  forall parent
      (recurse_left recurse_right :
        forall child, Recursive.shorter_body child parent ->
          option (bool * Semantic.semantic_summary)),
    (forall child shorter,
      recurse_left child shorter = recurse_right child shorter) ->
    forall child,
      Recursive.guarded_recursive_par_child parent recurse_left child =
      Recursive.guarded_recursive_par_child parent recurse_right child.
Proof.
  intros parent recurse_left recurse_right same child.
  unfold Recursive.guarded_recursive_par_child.
  destruct (lt_dec (length child) (length parent)) as [shorter|too_long].
  - exact (same child shorter).
  - reflexivity.
Qed.

Lemma recursive_par_step_ext :
  forall hash_fn body
      (recurse_left recurse_right :
        forall child, Recursive.shorter_body child body ->
          option (bool * Semantic.semantic_summary)),
    (forall child shorter,
      recurse_left child shorter = recurse_right child shorter) ->
    Recursive.recursive_par_step hash_fn body recurse_left =
    Recursive.recursive_par_step hash_fn body recurse_right.
Proof.
  intros hash_fn body recurse_left recurse_right same.
  unfold Recursive.recursive_par_step.
  destruct (Schema.lookup_projected_program 1 [] [])
    as [program|]; [|reflexivity].
  destruct (Source.decode_root 1 body) as [nodes|]; [|reflexivity].
  rewrite (recursive_source_children_ext
    (Recursive.guarded_recursive_par_child body recurse_left)
    (Recursive.guarded_recursive_par_child body recurse_right)
    (guarded_recursive_par_child_ext body recurse_left recurse_right same)
    hash_fn body program nodes).
  reflexivity.
Qed.

Lemma recursive_par_reference_unfold :
  forall hash_fn body,
    Recursive.recursive_par_reference hash_fn body =
    Recursive.recursive_par_step hash_fn body
      (fun child _ => Recursive.recursive_par_reference hash_fn child).
Proof.
  intros hash_fn body.
  unfold Recursive.recursive_par_reference.
  exact (Fix_eq Recursive.shorter_body_is_well_founded
    (fun _ => option (bool * Semantic.semantic_summary))
    (Recursive.recursive_par_step hash_fn)
    (recursive_par_step_ext hash_fn) body).
Qed.

Print Assumptions source_tree_nested_ind.
Print Assumptions reduce_map_par_ext.
Print Assumptions reduce_typed_map_entry_ext.
Print Assumptions reduce_source_map_node_ext.
Print Assumptions recursive_auxiliary_reference_ext.
Print Assumptions recursive_source_children_agree_on_members.
Print Assumptions recursive_source_reference_ext.
Print Assumptions recursive_source_children_ext.
Print Assumptions guarded_recursive_par_child_ext.
Print Assumptions recursive_par_step_ext.
Print Assumptions recursive_par_reference_unfold.
