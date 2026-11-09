From Stdlib Require Import List Bool Arith Lia NArith.
From StackSafePDA Require Import StackSafePDA EPM1 EPathMap
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3SourceReferenceIntegration EPM1ACTree03.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Reference := RPB3SourceReferenceIntegration.
Module Act := EPM1ACTree03.

Definition descriptor_eq_dec (left right : Schema.field_descriptor)
    : {left = right} + {left <> right}.
Proof. repeat decide equality; apply Nat.eq_dec. Defined.

Definition typed_snapshot_value (body : list nat)
    : option (bool * Schema.typed_summary) :=
  Reference.source_reference_par_of_body body.

Definition typed_snapshot_values (snapshot : EPM1.epm1_snapshot)
    : option (list (bool * Schema.typed_summary)) :=
  Source.sequence_source_results
    (map typed_snapshot_value (EPM1.value_bodies snapshot)).

Inductive auxiliary_result : Type :=
| StringMapResult
    (raw : Schema.raw_field)
    (entry : Source.typed_map_return Schema.typed_summary)
| PathMapResult
    (raw : Schema.raw_field)
    (snapshot : EPM1.epm1_snapshot)
    (trie : option Act.P.ByteTrie)
    (values : list (bool * Schema.typed_summary)).

Definition auxiliary_result_raw (result : auxiliary_result)
    : Schema.raw_field :=
  match result with
  | StringMapResult raw _ | PathMapResult raw _ _ _ => raw
  end.

Definition typed_auxiliary_reference (hash_fn : list nat -> nat)
    (bytes : list nat) (program : Schema.generated_program)
    (slot : nat) (node : Source.source_tree)
    : option auxiliary_result :=
  match node with
  | Source.SourceAuxiliary span carried =>
      match Source.source_descriptor program span with
      | Some selected =>
          if descriptor_eq_dec carried selected then
            if Schema.descriptor_slot selected =? slot then
              match Source.raw_field_of_source bytes node with
              | Some raw =>
                  if Schema.valid_raw_body raw then
                    match Schema.descriptor_payload selected with
                    | Schema.StringMap =>
                        option_map (StringMapResult raw)
                          (Source.reduce_source_map_node bytes program slot
                            Reference.source_reference_par_of_body node)
                    | Schema.PathMap =>
                        match Act.decode_epm1_actree03 hash_fn
                            (Source.source_body bytes node) with
                        | Some (snapshot, trie) =>
                            option_map (PathMapResult raw snapshot trie)
                              (typed_snapshot_values snapshot)
                        | None => None
                        end
                    | Schema.Scalar | Schema.Message => None
                    end
                  else None
              | None => None
              end
            else None
          else None
      | None => None
      end
  | _ => None
  end.

Definition auxiliary_node_in_slot (slot : nat)
    (node : Source.source_tree) : bool :=
  match node with
  | Source.SourceAuxiliary _ field =>
      Schema.descriptor_slot field =? slot
  | _ => false
  end.

Definition typed_auxiliary_root (hash_fn : list nat -> nat)
    (type_id slot : nat) (bytes : list nat)
    : option auxiliary_result :=
  match Schema.lookup_projected_program type_id [] [],
        Source.decode_root type_id bytes with
  | Some program, Some nodes =>
      match find (auxiliary_node_in_slot slot) nodes with
      | Some node => typed_auxiliary_reference hash_fn bytes program slot node
      | None => None
      end
  | _, _ => None
  end.

Theorem typed_snapshot_value_has_original_reference :
  forall body semantic_default summary,
    typed_snapshot_value body = Some (semantic_default, summary) ->
    Reference.source_reference_par_of_body body =
      Some (semantic_default, summary) /\
    Schema.summary_body summary = body /\
    Schema.summary_valid summary = true /\
    exists root,
      Schema.typed_reference root = summary /\
      Schema.worklist_outcome root = Schema.AcceptedSummary summary.
Proof.
  intros body semantic_default summary accepted.
  unfold typed_snapshot_value in accepted.
  destruct (Reference.source_reference_par_body_has_decoded_reference
    body semantic_default summary accepted)
    as [program [nodes [root [_ [_ [_ [_ [_
      [typed [original [valid worklist]]]]]]]]]]].
  repeat split; try assumption.
  exists root. now split.
Qed.

Theorem typed_snapshot_values_have_positional_references :
  forall bodies results,
    Source.sequence_source_results (map typed_snapshot_value bodies) =
      Some results ->
    Forall2 (fun body result =>
      Reference.source_reference_par_of_body body = Some result /\
      Schema.summary_body (snd result) = body /\
      Schema.summary_valid (snd result) = true) bodies results.
Proof.
  induction bodies as [|body rest IH]; intros results accepted.
  - simpl in accepted. inversion accepted. constructor.
  - simpl in accepted.
    destruct (typed_snapshot_value body)
      as [[semantic summary]|] eqn:head;
      [|discriminate].
    destruct (Source.sequence_source_results
      (map typed_snapshot_value rest)) as [tail|] eqn:continued;
      [|discriminate].
    inversion accepted; subst results.
    constructor.
    + destruct (typed_snapshot_value_has_original_reference
        body semantic summary head)
        as [decoded [original [valid _]]].
      repeat split; assumption.
    + exact (IH tail eq_refl).
Qed.

Theorem accepted_auxiliary_has_exact_generated_raw_leaf :
  forall hash_fn bytes program slot node result,
    typed_auxiliary_reference hash_fn bytes program slot node =
      Some result ->
    exists span field raw,
      node = Source.SourceAuxiliary span field /\
      Source.source_descriptor program span = Some field /\
      Schema.descriptor_slot field = slot /\
      Source.raw_field_of_source bytes node = Some raw /\
      Schema.valid_raw_body raw = true /\
      auxiliary_result_raw result = raw /\
      Source.raw_tree_of_source (S (length bytes)) bytes node =
        Some (Node raw FNil) /\
      Schema.raw_body raw = Source.source_body bytes node.
Proof.
  intros hash_fn bytes program slot node result accepted.
  unfold typed_auxiliary_reference in accepted.
  destruct node as [span field|span field children|span field];
    try discriminate.
  destruct (Source.source_descriptor program span)
    as [selected|] eqn:generated; [|discriminate].
  destruct (descriptor_eq_dec field selected) as [same|different];
    [subst selected|discriminate].
  destruct (Schema.descriptor_slot field =? slot) eqn:selected_slot;
    [|discriminate].
  apply Nat.eqb_eq in selected_slot.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceAuxiliary span field)) as [raw|]
    eqn:converted; [|discriminate].
  destruct (Schema.valid_raw_body raw) eqn:valid;
    [|discriminate].
  destruct (Schema.descriptor_payload field);
    try discriminate.
  - destruct (Source.reduce_source_map_node bytes program slot
      Reference.source_reference_par_of_body
      (Source.SourceAuxiliary span field)) as [entry|];
      [|discriminate].
    inversion accepted; subst result.
    exists span, field, raw.
    repeat split; try assumption; try reflexivity.
    + cbn [Source.raw_tree_of_source]. now rewrite converted.
    + exact (proj1 (proj2 (proj2 (proj2
        (Source.raw_field_of_source_preserves_wire_and_body
          bytes (Source.SourceAuxiliary span field) raw converted))))).
  - destruct (Act.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)))
      as [[snapshot trie]|] eqn:physical; [|discriminate].
    destruct (typed_snapshot_values snapshot) as [values|]
      eqn:typed; [|discriminate].
    inversion accepted; subst result.
    exists span, field, raw.
    repeat split; try assumption; try reflexivity.
    + cbn [Source.raw_tree_of_source]. now rewrite converted.
    + exact (proj1 (proj2 (proj2 (proj2
        (Source.raw_field_of_source_preserves_wire_and_body
          bytes (Source.SourceAuxiliary span field) raw converted))))).
Qed.

Theorem accepted_pathmap_has_physical_and_positional_reference :
  forall hash_fn bytes program slot node raw snapshot trie values,
    typed_auxiliary_reference hash_fn bytes program slot node =
      Some (PathMapResult raw snapshot trie values) ->
    Act.decode_epm1_actree03 hash_fn (Source.source_body bytes node) =
      Some (snapshot, trie) /\
    EPM1.decode_epm1 (Source.source_body bytes node) = Some snapshot /\
    Forall2 (fun body result =>
      Reference.source_reference_par_of_body body = Some result /\
      Schema.summary_body (snd result) = body /\
      Schema.summary_valid (snd result) = true)
      (EPM1.value_bodies snapshot) values.
Proof.
  intros hash_fn bytes program slot node raw snapshot trie values accepted.
  unfold typed_auxiliary_reference in accepted.
  destruct node as [span field|span field children|span field];
    try discriminate.
  destruct (Source.source_descriptor program span) as [selected|];
    [|discriminate].
  destruct (descriptor_eq_dec field selected); [|discriminate].
  destruct (Schema.descriptor_slot selected =? slot); [|discriminate].
  destruct (Source.raw_field_of_source bytes
    (Source.SourceAuxiliary span field)) as [candidate_raw|];
    [|discriminate].
  destruct (Schema.valid_raw_body candidate_raw); [|discriminate].
  destruct (Schema.descriptor_payload selected); try discriminate.
  - destruct (Source.reduce_source_map_node bytes program slot
      Reference.source_reference_par_of_body
      (Source.SourceAuxiliary span field)); discriminate.
  - destruct (Act.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)))
      as [[read decoded_trie]|] eqn:physical; [|discriminate].
    destruct (typed_snapshot_values read) as [typed_values|]
      eqn:typed; [|discriminate].
    inversion accepted; subst.
    split; [reflexivity|].
    split.
    + exact (Act.decode_epm1_actree03_refines_epm1
        hash_fn (Source.source_body bytes
          (Source.SourceAuxiliary span selected))
        snapshot trie physical).
    + unfold typed_snapshot_values in typed.
      exact (typed_snapshot_values_have_positional_references
        (EPM1.value_bodies snapshot) values typed).
Qed.

Theorem accepted_string_map_has_typed_entry_source :
  forall hash_fn bytes program slot node raw entry,
    typed_auxiliary_reference hash_fn bytes program slot node =
      Some (StringMapResult raw entry) ->
    exists typed,
      Source.refine_string_map_entry bytes program node = Some typed /\
      Schema.descriptor_slot (Source.typed_map_outer_field typed) = slot /\
      Source.reduce_typed_map_entry
        Reference.source_reference_par_of_body typed = Some entry.
Proof.
  intros hash_fn bytes program slot node raw entry accepted.
  unfold typed_auxiliary_reference in accepted.
  destruct node as [span field|span field children|span field];
    try discriminate.
  destruct (Source.source_descriptor program span) as [selected|];
    [|discriminate].
  destruct (descriptor_eq_dec field selected); [|discriminate].
  destruct (Schema.descriptor_slot selected =? slot); [|discriminate].
  destruct (Source.raw_field_of_source bytes
    (Source.SourceAuxiliary span field)) as [candidate_raw|];
    [|discriminate].
  destruct (Schema.valid_raw_body candidate_raw); [|discriminate].
  destruct (Schema.descriptor_payload selected); try discriminate.
  - destruct (Source.reduce_source_map_node bytes program slot
      Reference.source_reference_par_of_body
      (Source.SourceAuxiliary span field)) as [returned|]
      eqn:reduced; [|discriminate].
    inversion accepted; subst returned.
    exact (Source.reduced_source_map_node_has_exact_typed_source
      Schema.typed_summary bytes program slot
      Reference.source_reference_par_of_body
      (Source.SourceAuxiliary span field) entry reduced).
  - destruct (Act.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)))
      as [[snapshot trie]|]; [|discriminate].
    destruct (typed_snapshot_values snapshot); discriminate.
Qed.

Theorem pathmap_physical_decode_failure_refuses_auxiliary :
  forall hash_fn bytes program slot span field,
    Source.source_descriptor program span = Some field ->
    Schema.descriptor_slot field = slot ->
    Schema.descriptor_payload field = Schema.PathMap ->
    Act.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)) = None ->
    typed_auxiliary_reference hash_fn bytes program slot
      (Source.SourceAuxiliary span field) = None.
Proof.
  intros hash_fn bytes program slot span field
    generated selected payload physical.
  unfold typed_auxiliary_reference.
  rewrite generated.
  destruct (descriptor_eq_dec field field) as [_|different];
    [|contradiction].
  rewrite selected, Nat.eqb_refl.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceAuxiliary span field)) as [raw|];
    [|reflexivity].
  destruct (Schema.valid_raw_body raw); [|reflexivity].
  now rewrite payload, physical.
Qed.

Theorem accepted_pathmap_map_has_exact_zero_marker_count :
  forall hash_fn bytes program slot node raw snapshot trie values,
    typed_auxiliary_reference hash_fn bytes program slot node =
      Some (PathMapResult raw snapshot (Some trie) values) ->
    EPM1.snapshot_mode snapshot = MapMode ->
    Forall (fun marker => marker = 0%N) (Act.act_value_markers trie) /\
    length (Act.act_value_markers trie) = length values.
Proof.
  intros hash_fn bytes program slot node raw snapshot trie values
    accepted mode.
  destruct (accepted_pathmap_has_physical_and_positional_reference
    hash_fn bytes program slot node raw snapshot (Some trie) values
    accepted) as [physical [_ positional]].
  destruct (Act.decoded_map_has_exact_zero_markers hash_fn
    (Source.source_body bytes node) snapshot trie physical mode)
    as [zeros count].
  split; [exact zeros|].
  rewrite count.
  now apply Forall2_length in positional.
Qed.

Lemma find_auxiliary_is_member :
  forall slot nodes node,
    find (auxiliary_node_in_slot slot) nodes = Some node ->
    In node nodes.
Proof.
  intros slot nodes.
  induction nodes as [|head tail IH]; intros node selected;
    simpl in selected; [discriminate|].
  destruct (auxiliary_node_in_slot slot head).
  - inversion selected; subst. now left.
  - right. exact (IH node selected).
Qed.

Theorem accepted_auxiliary_root_has_decoded_generated_source :
  forall hash_fn type_id slot bytes result,
    typed_auxiliary_root hash_fn type_id slot bytes = Some result ->
    exists program nodes node,
      Schema.lookup_projected_program type_id [] [] = Some program /\
      Source.decode_region (S (length bytes)) bytes program
        0 (length bytes) = Some nodes /\
      In node nodes /\
      Source.source_node_matches_generated_descriptor program node /\
      typed_auxiliary_reference hash_fn bytes program slot node =
        Some result.
Proof.
  intros hash_fn type_id slot bytes result accepted.
  unfold typed_auxiliary_root in accepted.
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|] eqn:looked_up; [|discriminate].
  destruct (Source.decode_root type_id bytes) as [nodes|]
    eqn:decoded; [|discriminate].
  destruct (find (auxiliary_node_in_slot slot) nodes) as [node|]
    eqn:selected; [|discriminate].
  exists program, nodes, node.
  assert (decoded_region :
    Source.decode_region (S (length bytes)) bytes program
      0 (length bytes) = Some nodes).
  { unfold Source.decode_root in decoded.
    now rewrite looked_up in decoded. }
  split; [reflexivity|].
  split; [exact decoded_region|].
  split.
  - exact (find_auxiliary_is_member slot nodes node selected).
  - split.
    + pose proof (Source.decoded_region_nodes_match_generated_source
      (S (length bytes)) bytes program 0 (length bytes) nodes
      decoded_region) as generated.
      rewrite Forall_forall in generated.
      exact (generated node
        (find_auxiliary_is_member slot nodes node selected)).
    + exact accepted.
Qed.

Theorem accepted_auxiliary_root_owns_exact_source_leaf :
  forall hash_fn type_id slot bytes result,
    typed_auxiliary_root hash_fn type_id slot bytes = Some result ->
    exists program nodes node raw,
      Schema.lookup_projected_program type_id [] [] = Some program /\
      Source.decode_region (S (length bytes)) bytes program
        0 (length bytes) = Some nodes /\
      In node nodes /\
      Source.raw_tree_of_source (S (length bytes)) bytes node =
        Some (Node raw FNil) /\
      Schema.valid_raw_body raw = true /\
      Schema.raw_body raw = Source.source_body bytes node /\
      auxiliary_result_raw result = raw.
Proof.
  intros hash_fn type_id slot bytes result accepted.
  destruct (accepted_auxiliary_root_has_decoded_generated_source
    hash_fn type_id slot bytes result accepted)
    as [program [nodes [node [looked_up [decoded
      [member [_ referred]]]]]]].
  destruct (accepted_auxiliary_has_exact_generated_raw_leaf
    hash_fn bytes program slot node result referred)
    as [span [field [raw [_ [_ [_ [_ [valid
      [same_raw [leaf body]]]]]]]]]].
  exists program, nodes, node, raw.
  repeat split; assumption.
Qed.

Definition empty_actree03_arena : list nat :=
  Act.act_magic ++ [16; 0; 0; 0; 0; 0; 0; 0] ++ [0] ++ repeat 0 8.

Definition empty_pathmap_snapshot : EPM1.epm1_snapshot :=
  EPM1.Epm1Snapshot MapMode empty_actree03_arena [].

Definition empty_pathmap_wire : list nat :=
  [74; 33] ++ EPM1.encode_epm1 empty_pathmap_snapshot.

Definition invalid_actree03_wire : list nat :=
  [74; 33] ++ EPM1.encode_epm1
    (EPM1.Epm1Snapshot MapMode (repeat 0 25) []).

Definition default_value_actree03_arena : list nat :=
  Act.act_magic ++ [16; 0; 0; 0; 0; 0; 0; 0] ++ [64; 0] ++ repeat 0 8.

Definition default_value_pathmap_snapshot : EPM1.epm1_snapshot :=
  EPM1.Epm1Snapshot MapMode default_value_actree03_arena [[]].

Definition default_value_pathmap_wire : list nat :=
  [74; 35] ++ EPM1.encode_epm1 default_value_pathmap_snapshot.

Example default_par_snapshot_value_preserves_its_semantic_default :
  exists summary,
    Reference.source_reference_par_of_body [] = Some (true, summary) /\
    typed_snapshot_value [] = Some (true, summary) /\
    Schema.summary_body summary = [].
Proof. vm_compute. repeat eexists. Qed.

Example empty_pathmap_has_typed_auxiliary_reference :
  forall hash_fn, exists raw trie values,
    typed_auxiliary_root hash_fn 31 3 empty_pathmap_wire =
      Some (PathMapResult raw empty_pathmap_snapshot (Some trie) values).
Proof. intro hash_fn. vm_compute. repeat eexists. Qed.

Example default_map_value_has_typed_auxiliary_reference :
  forall hash_fn, exists raw trie summary,
    typed_auxiliary_root hash_fn 31 3 default_value_pathmap_wire =
      Some (PathMapResult raw default_value_pathmap_snapshot
        (Some trie) [(true, summary)]) /\
    Schema.summary_body summary = [].
Proof. intro hash_fn. vm_compute. repeat eexists. Qed.

Example malformed_actree03_refuses_typed_auxiliary_reference :
  forall hash_fn,
    EPM1.decode_epm1
      (EPM1.encode_epm1
        (EPM1.Epm1Snapshot MapMode (repeat 0 25) [])) =
        Some (EPM1.Epm1Snapshot MapMode (repeat 0 25) []) /\
    typed_auxiliary_root hash_fn 31 3 invalid_actree03_wire = None.
Proof. intro hash_fn. vm_compute. now split. Qed.

Example omitted_new_map_value_has_typed_auxiliary_reference :
  forall hash_fn, exists raw entry,
    typed_auxiliary_root hash_fn 20 3
      [34; 3; 10; 1; 97] = Some (StringMapResult raw entry).
Proof. intro hash_fn. vm_compute. repeat eexists. Qed.

Print Assumptions typed_snapshot_value_has_original_reference.
Print Assumptions typed_snapshot_values_have_positional_references.
Print Assumptions accepted_auxiliary_has_exact_generated_raw_leaf.
Print Assumptions accepted_string_map_has_typed_entry_source.
Print Assumptions accepted_pathmap_has_physical_and_positional_reference.
Print Assumptions accepted_pathmap_map_has_exact_zero_marker_count.
Print Assumptions pathmap_physical_decode_failure_refuses_auxiliary.
Print Assumptions accepted_auxiliary_root_has_decoded_generated_source.
Print Assumptions accepted_auxiliary_root_owns_exact_source_leaf.
