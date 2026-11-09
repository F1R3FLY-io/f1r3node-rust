From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import StackSafePDA RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3SourceScalarReduction.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Scalar := RPB3SourceScalarReduction.

Fixpoint checked_source_scalar_tree (fuel : nat) (bytes : list nat)
    (program : Schema.generated_program) (node : Source.source_tree)
    : bool :=
  match fuel with
  | 0 => false
  | S smaller =>
      match node with
      | Source.SourceScalar _ _ =>
          match Scalar.source_scalar_raw bytes program node with
          | Some _ => true
          | None => false
          end
      | Source.SourceMessage _ field children =>
          Schema.payload_eqb (Schema.descriptor_payload field)
              Schema.Message &&
          match Schema.descriptor_child_type_id field with
          | Some type_id =>
              match Schema.lookup_projected_program type_id [] [] with
              | Some child_program =>
                  forallb
                    (checked_source_scalar_tree smaller bytes child_program)
                    children
              | None => false
              end
          | None => false
          end
      | Source.SourceAuxiliary _ _ => false
      end
  end.

Fixpoint source_tree_scalars_have_projection (fuel : nat)
    (bytes : list nat) (program : Schema.generated_program)
    (node : Source.source_tree) : Prop :=
  match fuel with
  | 0 => False
  | S smaller =>
      match node with
      | Source.SourceScalar _ _ =>
          exists raw value,
            Scalar.source_scalar_raw bytes program node = Some raw /\
            Scalar.source_scalar_semantic bytes program node = Some value
      | Source.SourceMessage _ field children =>
          exists type_id child_program,
            Schema.descriptor_child_type_id field = Some type_id /\
            Schema.lookup_projected_program type_id [] [] =
              Some child_program /\
            Forall (source_tree_scalars_have_projection smaller bytes
              child_program) children
      | Source.SourceAuxiliary _ _ => False
      end
  end.

Lemma checked_source_scalar_tree_has_semantic_projections :
  forall fuel bytes program node,
    checked_source_scalar_tree fuel bytes program node = true ->
    source_tree_scalars_have_projection fuel bytes program node.
Proof.
  induction fuel as [|smaller IH]; intros bytes program node checked.
  - destruct node; discriminate.
  - destruct node as [span field|span field children|span field].
    + cbn [checked_source_scalar_tree
        source_tree_scalars_have_projection] in checked |- *.
      destruct (Scalar.source_scalar_raw bytes program
        (Source.SourceScalar span field)) as [raw|]
        eqn:reduced; [|discriminate].
      destruct (Scalar.accepted_source_scalar_has_semantic_projection
        bytes program (Source.SourceScalar span field) raw reduced)
        as [value semantic].
      exists raw, value. now split.
    + cbn [checked_source_scalar_tree
        source_tree_scalars_have_projection] in checked |- *.
      apply andb_true_iff in checked as [_ checked].
      destruct (Schema.descriptor_child_type_id field)
        as [type_id|] eqn:child; [|discriminate].
      destruct (Schema.lookup_projected_program type_id [] [])
        as [child_program|] eqn:lookup; [|discriminate].
      exists type_id, child_program.
      split; [reflexivity|].
      split; [exact lookup|].
      rewrite Forall_forall.
      intros descendant member.
      rewrite forallb_forall in checked.
      exact (IH bytes child_program descendant
        (checked descendant member)).
    + discriminate.
Qed.

Theorem checked_source_scalar_tree_has_raw_reference_tree :
  forall fuel bytes program node,
    checked_source_scalar_tree fuel bytes program node = true ->
    exists converted,
      Source.raw_tree_of_source fuel bytes node = Some converted.
Proof.
  induction fuel as [|smaller IH]; intros bytes program node checked.
  - destruct node; discriminate.
  - destruct node as [span field|span field children|span field].
    + cbn [checked_source_scalar_tree] in checked.
      destruct (Scalar.source_scalar_raw bytes program
        (Source.SourceScalar span field)) as [raw|]
        eqn:scalar; [|discriminate].
      exists (Node raw FNil).
      eapply Scalar.accepted_source_scalar_reuses_raw_tree_leaf;
        exact scalar.
    + cbn [checked_source_scalar_tree] in checked.
      apply andb_true_iff in checked as [payload checked].
      destruct (Schema.descriptor_child_type_id field)
        as [type_id|] eqn:child; [|discriminate].
      destruct (Schema.lookup_projected_program type_id [] [])
        as [child_program|] eqn:lookup; [|discriminate].
      assert (converted_children : exists converted,
        Source.sequence_source_results
          (map (Source.raw_tree_of_source smaller bytes) children) =
            Some converted).
      { revert checked.
        induction children as [|first rest list_IH]; intro checked.
        - exists []. reflexivity.
        - simpl in checked.
          apply andb_true_iff in checked as [first_checked rest_checked].
          destruct (IH bytes child_program first first_checked)
            as [first_tree converted_first].
          destruct (list_IH rest_checked) as [rest_trees converted_rest].
          exists (first_tree :: rest_trees).
          simpl. now rewrite converted_first, converted_rest. }
      destruct converted_children as [converted_children converted].
      assert (raw : exists label,
        Source.raw_field_of_source bytes
          (Source.SourceMessage span field children) = Some label).
      { unfold Source.raw_field_of_source. simpl.
        rewrite payload, child. eexists. reflexivity. }
      destruct raw as [label raw].
      exists (Node label (forest_of_list converted_children)).
      cbn [Source.raw_tree_of_source].
      now rewrite raw, converted.
    + discriminate.
Qed.

Lemma checked_source_scalar_forest_has_raw_reference_trees :
  forall fuel bytes program nodes,
    forallb (checked_source_scalar_tree fuel bytes program) nodes = true ->
    exists converted,
      Source.sequence_source_results
        (map (Source.raw_tree_of_source fuel bytes) nodes) = Some converted.
Proof.
  intros fuel bytes program nodes.
  induction nodes as [|first rest IH]; intro checked.
  - exists []. reflexivity.
  - simpl in checked.
    apply andb_true_iff in checked as [first_checked rest_checked].
    destruct (checked_source_scalar_tree_has_raw_reference_tree
      fuel bytes program first first_checked)
      as [first_tree converted_first].
    destruct (IH rest_checked) as [rest_trees converted_rest].
    exists (first_tree :: rest_trees).
    simpl. now rewrite converted_first, converted_rest.
Qed.

Definition checked_source_raw_par_root (bytes : list nat)
    (program : Schema.generated_program) (nodes : list Source.source_tree)
    : option (@tree Schema.raw_field) :=
  if Schema.program_family program =? 1 then
    if forallb (checked_source_scalar_tree (S (length bytes)) bytes program)
        nodes then
      option_map (fun children =>
        Node (Schema.RawField 0 Schema.LengthDelimited Schema.Message
          bytes (length bytes) (Some 1) [] [])
          (forest_of_list children))
        (Source.sequence_source_results
          (map (Source.raw_tree_of_source (S (length bytes)) bytes) nodes))
    else None
  else None.

Theorem checked_source_raw_par_root_is_total_on_checked_source :
  forall bytes program nodes,
    Schema.program_family program = 1 ->
    forallb (checked_source_scalar_tree (S (length bytes)) bytes program)
      nodes = true ->
    exists root, checked_source_raw_par_root bytes program nodes =
      Some root.
Proof.
  intros bytes program nodes family checked.
  destruct (checked_source_scalar_forest_has_raw_reference_trees
    (S (length bytes)) bytes program nodes checked)
    as [converted converted_all].
  unfold checked_source_raw_par_root.
  rewrite family, Nat.eqb_refl, checked, converted_all.
  eexists. reflexivity.
Qed.

Definition source_reference_par_semantics (bytes : list nat)
    (program : Schema.generated_program) (nodes : list Source.source_tree)
    : option Schema.typed_summary :=
  match checked_source_raw_par_root bytes program nodes with
  | None => None
  | Some root =>
      let summary := Schema.typed_reference root in
      if Schema.summary_valid summary then Some summary else None
  end.

Definition source_reference_par_child (bytes : list nat)
    (program : Schema.generated_program) (nodes : list Source.source_tree)
    : option (bool * Schema.typed_summary) :=
  Source.source_checked_par_child_reducer
    source_reference_par_semantics bytes program nodes.

Definition source_reference_par_of_body (body : list nat)
    : option (bool * Schema.typed_summary) :=
  match Schema.lookup_projected_program 1 [] [] with
  | None => None
  | Some program =>
      match Source.decode_region (S (length body)) body program
          0 (length body) with
      | None => None
      | Some nodes => source_reference_par_child body program nodes
      end
  end.

Definition source_reference_new_injections (bytes : list nat)
    : option (list (Source.typed_map_return Schema.typed_summary)) :=
  Source.reduce_root_map_slot 20 3 bytes source_reference_par_of_body.

Theorem accepted_source_reference_map_member_has_exact_raw_leaf :
  forall bytes program nodes slot node returned,
    Source.decode_region (S (length bytes)) bytes program
      0 (length bytes) = Some nodes ->
    In node nodes ->
    Source.reduce_source_map_node bytes program slot
      source_reference_par_of_body node = Some returned ->
    exists raw,
      Source.raw_tree_of_source (S (length bytes)) bytes node =
        Some (Node raw FNil) /\
      Schema.raw_payload raw = Schema.StringMap /\
      Schema.raw_program_type_id raw =
        Schema.descriptor_child_type_id (Source.source_field node) /\
      Schema.raw_body raw = Source.source_body bytes node /\
      Schema.valid_raw_body raw = true.
Proof.
  intros bytes program nodes slot node returned decoded member accepted.
  destruct (Source.reduced_source_map_node_has_exact_typed_source
    Schema.typed_summary bytes program slot source_reference_par_of_body
    node returned accepted) as [typed [refined [_ _]]].
  unfold Source.refine_string_map_entry in refined.
  destruct (Source.decode_string_map_entry_source bytes program node)
    as [entry|] eqn:source; [|discriminate].
  destruct (Source.decoded_source_map_entry_has_exact_raw_leaf
    (S (length bytes)) bytes program nodes node entry
    decoded member source) as [raw [converted
      [payload [child [body valid]]]]].
  exists raw.
  repeat split; try assumption.
  unfold Source.decode_string_map_entry_source in source.
  destruct node as [span field|span field children|span field];
    simpl in source; try discriminate.
  pose proof (Source.decoded_region_nodes_match_generated_source
    (S (length bytes)) bytes program 0 (length bytes) nodes decoded)
    as generated.
  rewrite Forall_forall in generated.
  destruct (generated (Source.SourceAuxiliary span field) member)
    as [actual _].
  simpl in actual.
  rewrite actual in source.
  destruct (Schema.descriptor_payload field) eqn:field_payload;
    simpl in source; try discriminate.
  destruct (Schema.descriptor_child_type_id field)
    as [type_id|] eqn:field_child; [|discriminate].
  destruct (Source.Wire.worklist_region bytes
    (Source.Wire.span_payload_start span)
    (Source.Wire.span_end span) true) as [spans| |];
    try discriminate.
  destruct (Source.parse_string_map_entry_spans spans) as [pair|];
    [|discriminate].
  inversion source; subst entry.
  simpl. now rewrite field_child.
Qed.

Lemma checked_source_raw_par_root_preserves_original_body :
  forall bytes program nodes root,
    checked_source_raw_par_root bytes program nodes = Some root ->
    Schema.summary_body (Schema.typed_reference root) = bytes.
Proof.
  intros bytes program nodes root converted.
  unfold checked_source_raw_par_root in converted.
  destruct (Schema.program_family program =? 1); [|discriminate].
  destruct (forallb
    (checked_source_scalar_tree (S (length bytes)) bytes program) nodes);
    [|discriminate].
  destruct (Source.sequence_source_results
    (map (Source.raw_tree_of_source (S (length bytes)) bytes) nodes))
    as [children|]; [|discriminate].
  inversion converted; subst root.
  reflexivity.
Qed.

Theorem source_reference_par_semantics_is_exact_typed_reference :
  forall bytes program nodes summary,
    source_reference_par_semantics bytes program nodes = Some summary ->
    exists root,
      checked_source_raw_par_root bytes program nodes = Some root /\
      Schema.typed_reference root = summary /\
      Schema.summary_body summary = bytes /\
      Schema.summary_valid summary = true /\
      Schema.worklist_outcome root = Schema.AcceptedSummary summary.
Proof.
  intros bytes program nodes summary accepted.
  unfold source_reference_par_semantics in accepted.
  destruct (checked_source_raw_par_root bytes program nodes)
    as [root|] eqn:converted; [|discriminate].
  destruct (Schema.summary_valid (Schema.typed_reference root))
    eqn:valid; [|discriminate].
  inversion accepted; subst summary.
  exists root.
  repeat split; try assumption; try reflexivity.
  - exact (checked_source_raw_par_root_preserves_original_body
      bytes program nodes root converted).
  - unfold Schema.worklist_outcome.
    rewrite Schema.explicit_worklist_equals_independent_typed_reference.
    unfold Schema.classify_summary. now rewrite valid.
Qed.

Theorem source_reference_par_semantics_has_projected_scalar_tree :
  forall bytes program nodes summary,
    source_reference_par_semantics bytes program nodes = Some summary ->
    Forall (source_tree_scalars_have_projection (S (length bytes))
      bytes program) nodes.
Proof.
  intros bytes program nodes summary accepted.
  unfold source_reference_par_semantics in accepted.
  destruct (checked_source_raw_par_root bytes program nodes)
    as [root|] eqn:converted; [|discriminate].
  unfold checked_source_raw_par_root in converted.
  destruct (Schema.program_family program =? 1); [|discriminate].
  destruct (forallb
    (checked_source_scalar_tree (S (length bytes)) bytes program) nodes)
    eqn:scalars; [|discriminate].
  rewrite Forall_forall.
  intros node member.
  rewrite forallb_forall in scalars.
  exact (checked_source_scalar_tree_has_semantic_projections
    (S (length bytes)) bytes program node (scalars node member)).
Qed.

Theorem source_reference_par_child_has_original_default_and_reference :
  forall bytes program nodes semantic_default summary,
    source_reference_par_child bytes program nodes =
      Some (semantic_default, summary) ->
    Schema.program_family program = 1 /\
    Source.source_par_semantic_default bytes nodes =
      Some semantic_default /\
    exists root,
      checked_source_raw_par_root bytes program nodes = Some root /\
      Schema.typed_reference root = summary /\
      Schema.summary_body summary = bytes /\
      Schema.summary_valid summary = true /\
      Schema.worklist_outcome root = Schema.AcceptedSummary summary.
Proof.
  intros bytes program nodes semantic_default summary accepted.
  pose proof (Source.source_checked_par_child_result_has_source_summary
    Schema.typed_summary source_reference_par_semantics bytes program
    nodes semantic_default summary accepted)
    as [family [source semantic]].
  destruct (source_reference_par_semantics_is_exact_typed_reference
    bytes program nodes summary semantic) as [root reference].
  repeat split; try assumption.
  now exists root.
Qed.

Theorem source_reference_par_body_has_decoded_reference :
  forall body semantic_default summary,
    source_reference_par_of_body body =
      Some (semantic_default, summary) ->
    exists program nodes root,
      Schema.lookup_projected_program 1 [] [] = Some program /\
      Source.decode_region (S (length body)) body program
        0 (length body) = Some nodes /\
      Forall (source_tree_scalars_have_projection (S (length body))
        body program) nodes /\
      Source.source_par_semantic_default body nodes =
        Some semantic_default /\
      checked_source_raw_par_root body program nodes = Some root /\
      Schema.typed_reference root = summary /\
      Schema.summary_body summary = body /\
      Schema.summary_valid summary = true /\
      Schema.worklist_outcome root = Schema.AcceptedSummary summary.
Proof.
  intros body semantic_default summary accepted.
  unfold source_reference_par_of_body in accepted.
  destruct (Schema.lookup_projected_program 1 [] [])
    as [program|] eqn:looked_up; [|discriminate].
  destruct (Source.decode_region (S (length body)) body program
      0 (length body)) as [nodes|] eqn:decoded; [|discriminate].
  destruct (source_reference_par_child_has_original_default_and_reference
    body program nodes semantic_default summary accepted)
    as [_ [source [root [converted [typed [original [valid worklist]]]]]]].
  pose proof (Source.source_checked_par_child_result_has_source_summary
    Schema.typed_summary source_reference_par_semantics body program
    nodes semantic_default summary accepted)
    as [_ [_ semantics]].
  pose proof (source_reference_par_semantics_has_projected_scalar_tree
    body program nodes summary semantics) as projections.
  exists program, nodes, root.
  repeat split; try assumption.
Qed.

Lemma converted_source_sequence_has_each_raw_tree :
  forall bytes fuel nodes converted node,
    Source.sequence_source_results
      (map (Source.raw_tree_of_source fuel bytes) nodes) =
        Some converted ->
    In node nodes ->
    exists raw_tree,
      Source.raw_tree_of_source fuel bytes node = Some raw_tree.
Proof.
  intros bytes fuel nodes.
  induction nodes as [|first rest IH];
    intros converted node converted_all member;
    simpl in converted_all, member; [contradiction|].
  destruct (Source.raw_tree_of_source fuel bytes first)
    as [first_tree|] eqn:converted_first; [|discriminate].
  destruct (Source.sequence_source_results
    (map (Source.raw_tree_of_source fuel bytes) rest))
    as [rest_trees|] eqn:converted_rest; [|discriminate].
  destruct member as [same|later].
  - subst node. now exists first_tree.
  - eapply IH; [reflexivity|exact later].
Qed.

Theorem source_reference_par_body_converted_fields_pass_reference_body_guard :
  forall body semantic_default summary,
    source_reference_par_of_body body =
      Some (semantic_default, summary) ->
    exists program nodes,
      Schema.lookup_projected_program 1 [] [] = Some program /\
      Source.decode_region (S (length body)) body program
        0 (length body) = Some nodes /\
      Forall (fun node => exists raw,
        Source.raw_field_of_source body node = Some raw /\
        Schema.valid_raw_body raw = true) nodes.
Proof.
  intros body semantic_default summary accepted.
  destruct (source_reference_par_body_has_decoded_reference
    body semantic_default summary accepted)
    as [program [nodes [root [looked_up [decoded
      [_ [_ [converted _]]]]]]]].
  unfold checked_source_raw_par_root in converted.
  destruct (Schema.program_family program =? 1); [|discriminate].
  destruct (forallb
    (checked_source_scalar_tree (S (length body)) body program) nodes);
    [|discriminate].
  destruct (Source.sequence_source_results
    (map (Source.raw_tree_of_source (S (length body)) body) nodes))
    as [converted_nodes|] eqn:converted_all; [|discriminate].
  exists program, nodes.
  split; [exact looked_up|].
  split; [exact decoded|].
  apply Forall_forall.
  intros node member.
  destruct (converted_source_sequence_has_each_raw_tree
    body (S (length body)) nodes converted_nodes node
    converted_all member) as [raw_tree converted_node].
  destruct (Source.raw_tree_of_source_preserves_root_label
    (S (length body)) body node raw_tree converted_node)
    as [raw [children [_ converted_field]]].
  exists raw.
  split; [exact converted_field|].
  eapply Source.decoded_root_raw_field_passes_reference_body_guard;
    [exact decoded|exact member|exact converted_field].
Qed.

Theorem source_reference_par_body_decoded_descendants_pass_reference_body_guard :
  forall body semantic_default summary,
    source_reference_par_of_body body =
      Some (semantic_default, summary) ->
    exists program nodes,
      Schema.lookup_projected_program 1 [] [] = Some program /\
      Source.decode_region (S (length body)) body program
        0 (length body) = Some nodes /\
      Forall (fun node => forall descendant raw,
        Source.source_tree_descendant node descendant ->
        Source.raw_field_of_source body descendant = Some raw ->
        Schema.valid_raw_body raw = true) nodes.
Proof.
  intros body semantic_default summary accepted.
  destruct (source_reference_par_body_has_decoded_reference
    body semantic_default summary accepted)
    as [program [nodes [root [looked_up [decoded _]]]]].
  exists program, nodes.
  split; [exact looked_up|].
  split; [exact decoded|].
  apply Forall_forall.
  intros node member descendant raw reached converted.
  eapply Source.decoded_root_descendant_raw_field_passes_reference_body_guard;
    [exact decoded|exact member|exact reached|exact converted].
Qed.

Theorem source_reference_new_injections_preserves_typed_order :
  forall bytes returned,
    source_reference_new_injections bytes = Some returned ->
    exists program nodes,
      Schema.lookup_projected_program 20 [] [] = Some program /\
      Source.decode_region (S (length bytes)) bytes program
        0 (length bytes) = Some nodes /\
      Forall2 (fun node result => exists typed,
        Source.refine_string_map_entry bytes program node = Some typed /\
        Schema.descriptor_slot (Source.typed_map_outer_field typed) = 3 /\
        Source.reduce_typed_map_entry source_reference_par_of_body typed =
          Some result)
        (filter (Source.source_map_node_in_slot program 3) nodes) returned /\
      Source.ordered_map_returns_after None returned.
Proof.
  intros bytes returned accepted.
  exact (Source.root_map_slot_reduction_preserves_source_and_order
    Schema.typed_summary 20 3 bytes source_reference_par_of_body
    returned accepted).
Qed.

Lemma forall2_return_member_has_source :
  forall (A B : Type) (relation : A -> B -> Prop) left right result,
    Forall2 relation left right ->
    In result right ->
    exists node, In node left /\ relation node result.
Proof.
  intros A B relation left right result paired.
  induction paired as [|node returned rest_nodes rest_results
      related paired IH]; intro present.
  - contradiction.
  - destruct present as [same|later].
    + subst result. exists node. split; [left; reflexivity|exact related].
    + destruct (IH later) as [found [member relation_found]].
      exists found. split; [right; exact member|exact relation_found].
Qed.

Theorem returned_new_injection_par_has_source_reference_result :
  forall bytes returned result summary,
    source_reference_new_injections bytes = Some returned ->
    In result returned ->
    Source.returned_map_par Schema.typed_summary result = Some summary ->
    exists program nodes node typed span body child_program child_nodes root,
      Schema.lookup_projected_program 20 [] [] = Some program /\
      Source.decode_region (S (length bytes)) bytes program
        0 (length bytes) = Some nodes /\
      In node (filter (Source.source_map_node_in_slot program 3) nodes) /\
      Source.refine_string_map_entry bytes program node = Some typed /\
      Source.typed_map_value_source typed = Some (span, body) /\
      source_reference_par_of_body body = Some (false, summary) /\
      Schema.lookup_projected_program 1 [] [] = Some child_program /\
      Source.decode_region (S (length body)) body child_program
        0 (length body) = Some child_nodes /\
      Forall (source_tree_scalars_have_projection (S (length body))
        body child_program) child_nodes /\
      Source.source_par_semantic_default body child_nodes = Some false /\
      checked_source_raw_par_root body child_program child_nodes = Some root /\
      Schema.typed_reference root = summary /\
      Schema.summary_body summary = body /\
      Schema.summary_valid summary = true /\
      Schema.worklist_outcome root = Schema.AcceptedSummary summary.
Proof.
  intros bytes returned result summary accepted member present.
  destruct (source_reference_new_injections_preserves_typed_order
    bytes returned accepted)
    as [program [nodes [looked_up [decoded [paired ordered]]]]].
  destruct (forall2_return_member_has_source
    Source.source_tree
    (Source.typed_map_return Schema.typed_summary)
    (fun node result => exists typed,
      Source.refine_string_map_entry bytes program node = Some typed /\
      Schema.descriptor_slot (Source.typed_map_outer_field typed) = 3 /\
      Source.reduce_typed_map_entry source_reference_par_of_body typed =
        Some result)
    (filter (Source.source_map_node_in_slot program 3) nodes)
    returned result paired member)
    as [node [selected [typed [refined [_ reduced]]]]].
  pose proof (Source.reduced_typed_map_return_matches_source
    Schema.typed_summary source_reference_par_of_body typed result reduced)
    as [_ value_source].
  destruct (Source.typed_map_value_source typed) as [[span body]|]
    eqn:source_value; simpl in value_source.
  - destruct value_source as [value [child [same_value same_span]]].
    rewrite present in same_value.
    inversion same_value; subst value.
    destruct (source_reference_par_body_has_decoded_reference
      body false summary child)
      as [child_program [child_nodes [root facts]]].
    exists program, nodes, node, typed, span, body,
      child_program, child_nodes, root.
    tauto.
  - destruct value_source as [absent _].
    rewrite present in absent. discriminate.
Qed.

Theorem returned_new_injection_omitted_par_has_absent_source :
  forall bytes returned result,
    source_reference_new_injections bytes = Some returned ->
    In result returned ->
    Source.returned_map_par Schema.typed_summary result = None ->
    exists program nodes node typed,
      Schema.lookup_projected_program 20 [] [] = Some program /\
      Source.decode_region (S (length bytes)) bytes program
        0 (length bytes) = Some nodes /\
      In node (filter (Source.source_map_node_in_slot program 3) nodes) /\
      Source.refine_string_map_entry bytes program node = Some typed /\
      Source.typed_map_value_source typed = None /\
      Source.returned_map_par_span Schema.typed_summary result = None.
Proof.
  intros bytes returned result accepted member omitted.
  destruct (source_reference_new_injections_preserves_typed_order
    bytes returned accepted)
    as [program [nodes [looked_up [decoded [paired ordered]]]]].
  destruct (forall2_return_member_has_source
    Source.source_tree
    (Source.typed_map_return Schema.typed_summary)
    (fun node result => exists typed,
      Source.refine_string_map_entry bytes program node = Some typed /\
      Schema.descriptor_slot (Source.typed_map_outer_field typed) = 3 /\
      Source.reduce_typed_map_entry source_reference_par_of_body typed =
        Some result)
    (filter (Source.source_map_node_in_slot program 3) nodes)
    returned result paired member)
    as [node [selected [typed [refined [_ reduced]]]]].
  pose proof (Source.reduced_typed_map_return_matches_source
    Schema.typed_summary source_reference_par_of_body typed result reduced)
    as [_ value_source].
  destruct (Source.typed_map_value_source typed) as [[span body]|]
    eqn:source_value; simpl in value_source.
  - destruct value_source as [value [child [present _]]].
    rewrite omitted in present. discriminate.
  - destruct value_source as [absent missing_span].
    exists program, nodes, node, typed.
    repeat split; try assumption.
Qed.

Print Assumptions checked_source_raw_par_root_preserves_original_body.
Print Assumptions checked_source_scalar_tree_has_semantic_projections.
Print Assumptions checked_source_scalar_tree_has_raw_reference_tree.
Print Assumptions checked_source_scalar_forest_has_raw_reference_trees.
Print Assumptions checked_source_raw_par_root_is_total_on_checked_source.
Print Assumptions accepted_source_reference_map_member_has_exact_raw_leaf.
Print Assumptions source_reference_par_semantics_is_exact_typed_reference.
Print Assumptions source_reference_par_semantics_has_projected_scalar_tree.
Print Assumptions source_reference_par_child_has_original_default_and_reference.
Print Assumptions source_reference_par_body_has_decoded_reference.
Print Assumptions converted_source_sequence_has_each_raw_tree.
Print Assumptions source_reference_par_body_converted_fields_pass_reference_body_guard.
Print Assumptions source_reference_par_body_decoded_descendants_pass_reference_body_guard.
Print Assumptions source_reference_new_injections_preserves_typed_order.
Print Assumptions returned_new_injection_par_has_source_reference_result.
Print Assumptions returned_new_injection_omitted_par_has_absent_source.
