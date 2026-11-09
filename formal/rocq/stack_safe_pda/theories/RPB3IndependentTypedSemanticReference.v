From Stdlib Require Import List Bool Arith NArith Lia Wf_nat.
From StackSafePDA Require Import StackSafePDA EPM1 EPathMap RPB3WireCursor
  RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3SourceScalarReduction RPB3SemanticReducer EPM1ACTree03
  RPB3AuxiliaryTypedReference RPB3RecursiveAuxiliarySourceReference.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Scalar := RPB3SourceScalarReduction.
Module Semantic := RPB3SemanticReducer.
Module Auxiliary := RPB3AuxiliaryTypedReference.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.

Definition typed_entry_of_pair_entry
    (entry : Source.typed_map_return
      (Schema.typed_summary * Semantic.semantic_summary))
    : Source.typed_map_return Schema.typed_summary :=
  Source.TypedMapReturn Schema.typed_summary
    (Source.returned_map_key _ entry)
    (Source.returned_map_key_span _ entry)
    (option_map fst (Source.returned_map_par _ entry))
    (Source.returned_map_par_span _ entry).

Definition semantic_entry_of_pair_entry
    (entry : Source.typed_map_return
      (Schema.typed_summary * Semantic.semantic_summary))
    : Source.typed_map_return Semantic.semantic_summary :=
  Source.TypedMapReturn Semantic.semantic_summary
    (Source.returned_map_key _ entry)
    (Source.returned_map_key_span _ entry)
    (option_map snd (Source.returned_map_par _ entry))
    (Source.returned_map_par_span _ entry).

Definition typed_value_of_pair
    (value : bool * (Schema.typed_summary * Semantic.semantic_summary))
    : bool * Schema.typed_summary :=
  (fst value, fst (snd value)).

Definition semantic_value_of_pair
    (value : bool * (Schema.typed_summary * Semantic.semantic_summary))
    : bool * Semantic.semantic_summary :=
  (fst value, snd (snd value)).

Inductive typed_semantic_result : Type :=
| TypedSemanticScalar
    (raw : Schema.raw_field) (typed : Schema.typed_summary)
    (semantic : Semantic.semantic_summary)
| TypedSemanticMessage
    (raw : Schema.raw_field) (typed : Schema.typed_summary)
    (semantic : Semantic.semantic_summary)
    (children : list typed_semantic_result)
| TypedSemanticStringMap
    (raw : Schema.raw_field)
    (entry : Source.typed_map_return
      (Schema.typed_summary * Semantic.semantic_summary))
    (typed : Schema.typed_summary)
    (semantic : Semantic.semantic_summary)
| TypedSemanticPathMap
    (raw : Schema.raw_field) (snapshot : EPM1.epm1_snapshot)
    (trie : option EPM1ACTree03.P.ByteTrie)
    (values : list
      (bool * (Schema.typed_summary * Semantic.semantic_summary)))
    (typed : Schema.typed_summary)
    (semantic : Semantic.semantic_summary).

Definition result_typed (result : typed_semantic_result)
    : Schema.typed_summary :=
  match result with
  | TypedSemanticScalar _ typed _
  | TypedSemanticMessage _ typed _ _
  | TypedSemanticStringMap _ _ typed _
  | TypedSemanticPathMap _ _ _ _ typed _ => typed
  end.

Definition result_semantic (result : typed_semantic_result)
    : Semantic.semantic_summary :=
  match result with
  | TypedSemanticScalar _ _ semantic
  | TypedSemanticMessage _ _ semantic _
  | TypedSemanticStringMap _ _ _ semantic
  | TypedSemanticPathMap _ _ _ _ _ semantic => semantic
  end.

Fixpoint semantic_projection (result : typed_semantic_result)
    : Recursive.recursive_mixed_result :=
  match result with
  | TypedSemanticScalar raw _ semantic =>
      Recursive.RecursiveScalarResult raw semantic
  | TypedSemanticMessage raw _ semantic children =>
      let fix project_children (remaining : list typed_semantic_result)
          : list Recursive.recursive_mixed_result :=
        match remaining with
        | [] => []
        | first :: rest =>
            semantic_projection first :: project_children rest
        end in
      Recursive.RecursiveMessageResult raw semantic
        (project_children children)
  | TypedSemanticStringMap raw entry _ semantic =>
      Recursive.RecursiveAuxiliaryResult
        (Recursive.RecursiveStringMapResult raw
          (semantic_entry_of_pair_entry entry)) semantic
  | TypedSemanticPathMap raw snapshot trie values _ semantic =>
      Recursive.RecursiveAuxiliaryResult
        (Recursive.RecursivePathMapResult raw snapshot trie
          (map semantic_value_of_pair values))
        semantic
  end.

Definition par_semantic_projection
    (result : option
      (bool * (Schema.typed_summary * Semantic.semantic_summary)))
    : option (bool * Semantic.semantic_summary) :=
  option_map (fun value => (fst value, snd (snd value))) result.

Fixpoint new_map_typed_entries (children : list typed_semantic_result)
    : list (Source.typed_map_return Schema.typed_summary) :=
  match children with
  | [] => []
  | TypedSemanticStringMap _ entry _ _ :: rest =>
      typed_entry_of_pair_entry entry :: new_map_typed_entries rest
  | _ :: rest => new_map_typed_entries rest
  end.

Definition checked_generated_raw (bytes : list nat)
    (program : Schema.generated_program) (node : Source.source_tree)
    : option Schema.raw_field :=
  match Source.source_descriptor program (Source.source_span node),
        Source.raw_field_of_source bytes node with
  | Some selected, Some raw =>
      if Auxiliary.descriptor_eq_dec (Source.source_field node) selected
      then if Schema.valid_raw_body raw then Some raw else None
      else None
  | _, _ => None
  end.

Definition checked_generated_scalar_raw (bytes : list nat)
    (program : Schema.generated_program) (node : Source.source_tree)
    : option Schema.raw_field :=
  match checked_generated_raw bytes program node,
        Scalar.source_scalar_raw bytes program node with
  | Some _, Some raw => Some raw
  | _, _ => None
  end.

Definition auxiliary_typed_summary (raw : Schema.raw_field)
    : Schema.typed_summary :=
  let type_id :=
    match Schema.raw_program_type_id raw with
    | Some selected => selected
    | None => 0
    end in
  Schema.TypedSummary (Schema.raw_tag raw) (Schema.raw_wire raw)
    (Schema.raw_payload raw) type_id (Schema.raw_body raw)
    true true (Schema.ScorePlan type_id true []) [] [].

Definition auxiliary_semantic_summary
    (raw : Schema.raw_field) (score : Semantic.canonical_score)
    : Semantic.semantic_summary :=
  Semantic.SemanticSummary (Schema.raw_tag raw) (Schema.raw_wire raw)
    (Schema.raw_payload raw)
    (match Schema.raw_program_type_id raw with
     | Some selected => selected
     | None => 0
     end)
    (Schema.raw_body raw) false (Some true)
    (Semantic.ExactScore score) false false.

Definition empty_par_score : Semantic.canonical_score :=
  Semantic.score_constructor 999 [Semantic.ScoreInteger 0%N].

Definition string_map_semantic_summary (raw : Schema.raw_field)
    (entry : Source.typed_map_return Semantic.semantic_summary)
    : option Semantic.semantic_summary :=
  let value_score :=
    match Source.returned_map_par Semantic.semantic_summary entry with
    | None => Some empty_par_score
    | Some value =>
        match Semantic.sem_score_evidence value with
        | Semantic.ExactScore score => Some score
        | Semantic.ScoreNotDerived => None
        end
    end in
  option_map (fun score =>
    auxiliary_semantic_summary raw
      (Semantic.ScoreNode (Semantic.score_forest_of_list
        [Semantic.ScoreString
          (Source.returned_map_key Semantic.semantic_summary entry);
         score]))) value_score.

Lemma source_string_map_projection_commutes :
  forall paired_child semantic_child bytes program slot node,
    (forall body,
      par_semantic_projection (paired_child body) =
        semantic_child body) ->
    option_map semantic_entry_of_pair_entry
      (Source.reduce_source_map_node bytes program slot
        paired_child node) =
    Source.reduce_source_map_node bytes program slot
      semantic_child node.
Proof.
  intros paired_child semantic_child bytes program slot node agree.
  unfold Source.reduce_source_map_node.
  destruct (Source.refine_string_map_entry bytes program node)
    as [entry|]; [|reflexivity].
  destruct (Schema.descriptor_slot
    (Source.typed_map_outer_field entry) =? slot);
    [|reflexivity].
  unfold Source.reduce_typed_map_entry.
  destruct (Source.reduce_map_key
    (Source.typed_map_key_source entry))
    as [[key key_span]|]; [|reflexivity].
  destruct (Source.typed_map_value_source entry)
    as [[value_span body]|] eqn:value_source.
  - unfold Source.reduce_map_par.
    rewrite <- (agree body).
    destruct (paired_child body)
      as [[semantic_default [typed semantic]]|];
      [destruct semantic_default|]; reflexivity.
  - reflexivity.
Qed.

Lemma source_pathmap_projection_commutes :
  forall
    (paired_child : list nat -> option
      (bool * (Schema.typed_summary * Semantic.semantic_summary)))
    (semantic_child : list nat -> option
      (bool * Semantic.semantic_summary))
    (bodies : list (list nat)),
    (forall body,
      par_semantic_projection (paired_child body) =
        semantic_child body) ->
    option_map (map semantic_value_of_pair)
      (Source.sequence_source_results (map paired_child bodies)) =
    Source.sequence_source_results (map semantic_child bodies).
Proof.
  intros paired_child semantic_child bodies agree.
  induction bodies as [|first rest IH]; [reflexivity|].
  simpl.
  rewrite <- (agree first).
  destruct (paired_child first)
    as [[semantic_default [typed semantic]]|] eqn:head;
    simpl; [|reflexivity].
  rewrite <- IH.
  destruct (Source.sequence_source_results (map paired_child rest));
    reflexivity.
Qed.

Definition auxiliary_reference
    (par_child : list nat -> option
      (bool * (Schema.typed_summary * Semantic.semantic_summary)))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (program : Schema.generated_program) (slot : nat)
    (node : Source.source_tree) : option typed_semantic_result :=
  match node, checked_generated_raw bytes program node with
  | Source.SourceAuxiliary _ field, Some raw =>
      if Schema.descriptor_slot field =? slot then
        match Schema.descriptor_payload field with
        | Schema.StringMap =>
            match Source.reduce_source_map_node bytes program slot
                par_child node with
            | None => None
            | Some paired_entry =>
                let semantic_entry :=
                  semantic_entry_of_pair_entry paired_entry in
                option_map (TypedSemanticStringMap raw paired_entry
                  (auxiliary_typed_summary raw))
                  (string_map_semantic_summary raw semantic_entry)
            end
        | Schema.PathMap =>
            match EPM1ACTree03.decode_epm1_actree03 hash_fn
                (Source.source_body bytes node) with
            | None => None
            | Some (snapshot, trie) =>
                option_map (fun values =>
                  TypedSemanticPathMap raw snapshot trie values
                    (auxiliary_typed_summary raw)
                    (auxiliary_semantic_summary raw
                      (Semantic.ScoreBytes (Schema.raw_body raw))))
                  (Source.sequence_source_results
                    (map par_child (EPM1.value_bodies snapshot)))
            end
        | Schema.Scalar | Schema.Message => None
        end
      else None
  | _, _ => None
  end.

Fixpoint ordered_new_map_entries
    (prior : option (list nat))
    (entries : list (Source.typed_map_return Schema.typed_summary))
    : bool :=
  match entries with
  | [] => true
  | entry :: rest =>
      Source.map_key_follows prior
        (Source.returned_map_key Schema.typed_summary entry) &&
      ordered_new_map_entries
        (Some (Source.returned_map_key Schema.typed_summary entry)) rest
  end.

Lemma ordered_new_map_entries_project_to_recursive_order :
  forall prior children,
    ordered_new_map_entries prior (new_map_typed_entries children) =
      Recursive.ordered_recursive_new_map_returnsb prior
        (Recursive.recursive_new_map_returns
          (map semantic_projection children)).
Proof.
  intros prior children.
  revert prior.
  induction children as [|first rest IH]; intro prior;
    simpl; [reflexivity|].
  destruct first; simpl; try apply IH.
  rewrite IH.
  reflexivity.
Qed.

Definition summary_alignment (typed : Schema.typed_summary)
    (semantic : Semantic.semantic_summary) : Prop :=
  Schema.summary_valid typed = true /\
  Semantic.sem_definitely_refused semantic = false /\
  Schema.summary_tag typed = Semantic.sem_tag semantic /\
  Schema.summary_wire typed = Semantic.sem_wire semantic /\
  Schema.summary_payload typed = Semantic.sem_payload semantic /\
  Schema.summary_program_type_id typed = Semantic.sem_type_id semantic /\
  Schema.summary_body typed = Semantic.sem_body semantic.

Definition scan_alignment (typed semantic : Schema.scan_state) : Prop :=
  Schema.scan_valid typed = Schema.scan_valid semantic /\
  Schema.scan_last_slot typed = Schema.scan_last_slot semantic /\
  Schema.scan_seen typed = Schema.scan_seen semantic /\
  Schema.scan_ordered_slots typed = Schema.scan_ordered_slots semantic.

Definition erase_scan_scores (state : Schema.scan_state)
    : Schema.scan_state :=
  Schema.ScanState (Schema.scan_valid state)
    (Schema.scan_last_slot state) (Schema.scan_seen state)
    (Schema.scan_ordered_slots state) [].

Lemma aligned_message_child_type_matches :
  forall descriptor typed semantic,
    Schema.summary_program_type_id typed =
      Semantic.sem_type_id semantic ->
    Schema.message_child_type_matches descriptor typed =
      match Schema.descriptor_payload descriptor with
      | Schema.Message =>
          match Schema.descriptor_child_type_id descriptor with
          | Some type_id => Semantic.sem_type_id semantic =? type_id
          | None => false
          end
      | Schema.Scalar | Schema.StringMap | Schema.PathMap => true
      end.
Proof.
  intros descriptor typed semantic same_type.
  unfold Schema.message_child_type_matches.
  destruct (Schema.descriptor_payload descriptor);
    try reflexivity.
  destruct (Schema.descriptor_child_type_id descriptor);
    [now rewrite same_type|reflexivity].
Qed.

Lemma aligned_scan_step_after_erasing_scores :
  forall program state typed semantic,
    summary_alignment typed semantic ->
    erase_scan_scores (Schema.checked_field program state typed) =
      Semantic.scan_child program (erase_scan_scores state) semantic.
Proof.
  intros program [state_valid last seen slots scores] typed semantic
    [typed_valid [not_refused [same_tag [same_wire
      [same_payload [same_type _]]]]]].
  unfold Schema.checked_field, Semantic.scan_child.
  cbn [erase_scan_scores Schema.scan_valid Schema.scan_last_slot
    Schema.scan_seen Schema.scan_ordered_slots].
  destruct state_valid; [|reflexivity].
  rewrite same_tag.
  destruct (Schema.resolve_tag (Schema.program_fields program)
    (Semantic.sem_tag semantic)) as [descriptor|];
    [|reflexivity].
  rewrite typed_valid, not_refused, same_wire, same_payload.
  rewrite (aligned_message_child_type_matches
    descriptor typed semantic same_type).
  cbn [negb andb].
  match goal with
  | |- context [if ?admissible then _ else _] =>
      destruct admissible
  end;
    cbn [erase_scan_scores Schema.scan_valid Schema.scan_last_slot
      Schema.scan_seen Schema.scan_ordered_slots];
    reflexivity.
Qed.

Lemma aligned_scan_fold_after_erasing_scores :
  forall program results state,
    Forall (fun result =>
      summary_alignment (result_typed result)
        (result_semantic result)) results ->
    erase_scan_scores
      (fold_left (Schema.checked_field program)
        (map result_typed results) state) =
    fold_left (Semantic.scan_child program)
      (map result_semantic results) (erase_scan_scores state).
Proof.
  intros program results.
  induction results as [|first rest IH]; intros state aligned;
    simpl; [reflexivity|].
  inversion aligned as [|? ? first_aligned rest_aligned]; subst.
  rewrite (IH (Schema.checked_field program state
    (result_typed first)) rest_aligned).
  now rewrite (aligned_scan_step_after_erasing_scores
    program state (result_typed first)
    (result_semantic first) first_aligned).
Qed.

Lemma source_raw_has_empty_required_slots :
  forall bytes node raw,
    Source.raw_field_of_source bytes node = Some raw ->
    Schema.raw_wire_required raw = [] /\
    Schema.raw_semantic_required raw = [].
Proof.
  intros bytes node raw sourced.
  destruct node as [span field|span field children|span field];
    cbn [Source.raw_field_of_source Source.source_span
      Source.source_field Source.source_body] in sourced.
  - destruct (Schema.payload_eqb
      (Schema.descriptor_payload field) Schema.Scalar);
      inversion sourced; subst; split; reflexivity.
  - destruct (Schema.payload_eqb
      (Schema.descriptor_payload field) Schema.Message);
      [|discriminate].
    destruct (Schema.descriptor_child_type_id field);
      inversion sourced; subst; split; reflexivity.
  - destruct (Schema.descriptor_payload field);
      try discriminate.
    + destruct (Schema.descriptor_child_type_id field);
        inversion sourced; subst; split; reflexivity.
    + destruct (Schema.descriptor_child_type_id field);
        inversion sourced; subst; split; reflexivity.
Qed.

Lemma source_raw_reducers_have_same_program_family :
  forall raw typed_children semantic_children,
    Schema.raw_wire_required raw = [] ->
    Schema.raw_semantic_required raw = [] ->
    Schema.summary_program_type_id
      (Schema.reduce_field raw typed_children) =
    Semantic.sem_type_id
      (Semantic.reduce_semantic_field raw semantic_children).
Proof.
  intros [tag wire payload body declared type_id wire_required
    semantic_required] typed_children semantic_children
    empty_wire empty_semantic.
  cbn in empty_wire, empty_semantic.
  subst wire_required semantic_required.
  unfold Schema.reduce_field, Semantic.reduce_semantic_field,
    Schema.resolved_raw_program, Semantic.generated_wire_program.
  destruct payload; destruct type_id; reflexivity.
Qed.

Fixpoint source_reference
    (par_child : list nat -> option
      (bool * (Schema.typed_summary * Semantic.semantic_summary)))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (program : Schema.generated_program) (node : Source.source_tree)
    {struct node} : option typed_semantic_result :=
  match node with
  | Source.SourceScalar _ _ =>
      match checked_generated_scalar_raw bytes program node with
      | None => None
      | Some raw =>
          let typed := Schema.reduce_field raw [] in
          let semantic := Semantic.reduce_semantic_field raw [] in
          if Schema.summary_valid typed then
            if Semantic.sem_definitely_refused semantic then None
            else Some (TypedSemanticScalar raw typed semantic)
          else None
      end
  | Source.SourceMessage _ field children =>
      match checked_generated_raw bytes program node,
            Schema.descriptor_child_type_id field with
      | Some raw, Some type_id =>
          match Schema.lookup_projected_program type_id [] [] with
          | None => None
          | Some child_program =>
              let fix visit_children
                  (remaining : list Source.source_tree)
                  : option (list typed_semantic_result) :=
                match remaining with
                | [] => Some []
                | first :: rest =>
                    match source_reference par_child hash_fn bytes
                            child_program first,
                          visit_children rest with
                    | Some head, Some tail => Some (head :: tail)
                    | _, _ => None
                    end
                end in
              match visit_children children with
              | None => None
              | Some converted =>
                  let ordered :=
                    if type_id =? 20
                    then ordered_new_map_entries None
                      (new_map_typed_entries converted)
                    else true in
                  let typed := Schema.reduce_field raw
                    (map result_typed converted) in
                  let semantic := Semantic.reduce_semantic_field raw
                    (map result_semantic converted) in
                  if ordered then
                    if Schema.summary_valid typed then
                      if Semantic.sem_definitely_refused semantic then None
                      else Some (TypedSemanticMessage raw typed semantic
                        converted)
                    else None
                  else None
              end
          end
      | _, _ => None
      end
  | Source.SourceAuxiliary _ field =>
      auxiliary_reference par_child hash_fn bytes program
        (Schema.descriptor_slot field) node
  end.

Fixpoint source_children_reference
    (par_child : list nat -> option
      (bool * (Schema.typed_summary * Semantic.semantic_summary)))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (program : Schema.generated_program)
    (nodes : list Source.source_tree) {struct nodes}
    : option (list typed_semantic_result) :=
  match nodes with
  | [] => Some []
  | first :: rest =>
      match source_reference par_child hash_fn bytes program first,
            source_children_reference par_child hash_fn bytes program rest with
      | Some head, Some tail => Some (head :: tail)
      | _, _ => None
      end
  end.

Definition par_raw (body : list nat) : Schema.raw_field :=
  Schema.RawField 0 Schema.LengthDelimited Schema.Message
    body (length body) (Some 1) [] [].

Definition shorter_body (child parent : list nat) : Prop :=
  length child < length parent.

Lemma shorter_body_is_well_founded : well_founded shorter_body.
Proof.
  unfold shorter_body.
  apply (well_founded_ltof (list nat) (@length nat)).
Defined.

Definition guarded_par_child (parent : list nat)
    (recurse : forall child, shorter_body child parent ->
      option (bool * (Schema.typed_summary * Semantic.semantic_summary)))
    (child : list nat)
    : option (bool * (Schema.typed_summary * Semantic.semantic_summary)) :=
  match lt_dec (length child) (length parent) with
  | left shorter => recurse child shorter
  | right _ => None
  end.

Definition par_step (hash_fn : list nat -> nat) (body : list nat)
    (recurse : forall child, shorter_body child body ->
      option (bool * (Schema.typed_summary * Semantic.semantic_summary)))
    : option (bool * (Schema.typed_summary * Semantic.semantic_summary)) :=
  let nested_par := guarded_par_child body recurse in
  match Schema.lookup_projected_program 1 [] [],
        Source.decode_root 1 body with
  | Some program, Some nodes =>
      match source_children_reference nested_par hash_fn body program nodes,
            Source.source_par_semantic_default body nodes with
      | Some children, Some semantic_default =>
          let typed := Schema.reduce_field (par_raw body)
            (map result_typed children) in
          let semantic := Semantic.reduce_semantic_field (par_raw body)
            (map result_semantic children) in
          if Schema.summary_valid typed then
            if Semantic.sem_definitely_refused semantic then None
            else match Semantic.sem_score_evidence semantic with
                 | Semantic.ExactScore _ =>
                     Some (semantic_default, (typed, semantic))
                 | Semantic.ScoreNotDerived => None
                 end
          else None
      | _, _ => None
      end
  | _, _ => None
  end.

Definition par_reference (hash_fn : list nat -> nat)
    : list nat -> option
      (bool * (Schema.typed_summary * Semantic.semantic_summary)) :=
  Fix shorter_body_is_well_founded
    (fun _ => option
      (bool * (Schema.typed_summary * Semantic.semantic_summary)))
    (par_step hash_fn).

Definition root_reference (hash_fn : list nat -> nat)
    (type_id : nat) (bytes : list nat)
    : option (list typed_semantic_result) :=
  match Schema.lookup_projected_program type_id [] [],
        Source.decode_root type_id bytes with
  | Some program, Some nodes =>
      source_children_reference (par_reference hash_fn)
        hash_fn bytes program nodes
  | _, _ => None
  end.

Fixpoint source_tree_size (node : Source.source_tree) : nat :=
  match node with
  | Source.SourceScalar _ _ | Source.SourceAuxiliary _ _ => 1
  | Source.SourceMessage _ _ children =>
      let fix children_size (remaining : list Source.source_tree) : nat :=
        match remaining with
        | [] => 0
        | first :: rest =>
            source_tree_size first + children_size rest
        end in
      S (children_size children)
  end.

Fixpoint source_tree_list_size (nodes : list Source.source_tree) : nat :=
  match nodes with
  | [] => 0
  | first :: rest =>
      source_tree_size first + source_tree_list_size rest
  end.

Lemma source_tree_message_size_uses_list_size :
  forall span field children,
    source_tree_size (Source.SourceMessage span field children) =
      S (source_tree_list_size children).
Proof.
  intros span field children.
  induction children as [|first rest IH]; simpl; [reflexivity|].
  simpl in IH.
  lia.
Qed.

Lemma source_tree_member_size_at_most_list_size :
  forall node nodes,
    In node nodes ->
    source_tree_size node <= source_tree_list_size nodes.
Proof.
  intros node nodes.
  induction nodes as [|first rest IH]; intros member;
    simpl in *; [contradiction|].
  destruct member as [same|later].
  - subst node. lia.
  - specialize (IH later). lia.
Qed.

Lemma source_tree_message_child_is_smaller :
  forall span field children child,
    In child children ->
    source_tree_size child <
      source_tree_size (Source.SourceMessage span field children).
Proof.
  intros span field children child member.
  rewrite source_tree_message_size_uses_list_size.
  pose proof (source_tree_member_size_at_most_list_size
    child children member).
  lia.
Qed.

Definition source_tree_smaller
    (child parent : Source.source_tree) : Prop :=
  source_tree_size child < source_tree_size parent.

Lemma source_tree_smaller_is_well_founded :
  well_founded source_tree_smaller.
Proof.
  unfold source_tree_smaller.
  apply (well_founded_ltof Source.source_tree source_tree_size).
Defined.

Lemma source_checked_raw_agrees_with_recursive_descriptor_check :
  forall bytes program node,
    checked_generated_raw bytes program node =
      Recursive.checked_generated_raw bytes program node.
Proof. reflexivity. Qed.

Lemma source_checked_scalar_agrees_with_recursive_grammar_check :
  forall bytes program node,
    checked_generated_scalar_raw bytes program node =
      Recursive.checked_generated_scalar_raw bytes program node.
Proof. reflexivity. Qed.

Lemma independently_constructed_string_map_score_agrees_with_semantics :
  forall raw entry,
    string_map_semantic_summary raw entry =
      Recursive.recursive_auxiliary_semantic
        (Recursive.RecursiveStringMapResult raw entry).
Proof.
  intros raw entry.
  unfold string_map_semantic_summary,
    Recursive.recursive_auxiliary_semantic,
    Recursive.recursive_auxiliary_raw,
    auxiliary_semantic_summary,
    empty_par_score, Recursive.empty_par_canonical_score.
  destruct (Source.returned_map_par Semantic.semantic_summary entry)
    as [value|]; [destruct (Semantic.sem_score_evidence value)|];
    reflexivity.
Qed.

Lemma independently_constructed_pathmap_score_agrees_with_semantics :
  forall raw snapshot trie values,
    Some (auxiliary_semantic_summary raw
      (Semantic.ScoreBytes (Schema.raw_body raw))) =
    Recursive.recursive_auxiliary_semantic
      (Recursive.RecursivePathMapResult raw snapshot trie values).
Proof. reflexivity. Qed.

Theorem auxiliary_reference_projects_to_recursive_semantics :
  forall paired_child semantic_child hash_fn bytes program slot node,
    (forall body,
      par_semantic_projection (paired_child body) =
        semantic_child body) ->
    option_map semantic_projection
      (auxiliary_reference paired_child hash_fn bytes program slot node) =
    Recursive.recursive_auxiliary_reference semantic_child
      hash_fn bytes program slot node.
Proof.
  intros paired_child semantic_child hash_fn bytes program slot node
    agree.
  unfold auxiliary_reference,
    Recursive.recursive_auxiliary_reference.
  destruct node as [span field|span field children|span field];
    simpl; try reflexivity.
  change (Recursive.checked_generated_raw bytes program
    (Source.SourceAuxiliary span field)) with
    (checked_generated_raw bytes program
      (Source.SourceAuxiliary span field)).
  destruct (checked_generated_raw bytes program
    (Source.SourceAuxiliary span field)) as [raw|];
    [|reflexivity].
  destruct (Schema.descriptor_slot field =? slot);
    [|reflexivity].
  destruct (Schema.descriptor_payload field);
    try reflexivity.
  - rewrite <- (source_string_map_projection_commutes
      paired_child semantic_child bytes program slot
      (Source.SourceAuxiliary span field) agree).
    destruct (Source.reduce_source_map_node bytes program slot
      paired_child (Source.SourceAuxiliary span field))
      as [paired_entry|]; [|reflexivity].
    simpl.
    rewrite <- independently_constructed_string_map_score_agrees_with_semantics.
    destruct (string_map_semantic_summary raw
      (semantic_entry_of_pair_entry paired_entry)); reflexivity.
  - destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)))
      as [[snapshot trie]|]; [|reflexivity].
    rewrite <- (source_pathmap_projection_commutes
      paired_child semantic_child (EPM1.value_bodies snapshot) agree).
    destruct (Source.sequence_source_results
      (map paired_child (EPM1.value_bodies snapshot)))
      as [values|]; [|reflexivity].
    simpl.
    reflexivity.
Qed.

Theorem accepted_par_step_preserves_typed_validity_body_and_exact_score :
  forall hash_fn body recurse semantic_default typed semantic,
    par_step hash_fn body recurse =
      Some (semantic_default, (typed, semantic)) ->
    Schema.summary_valid typed = true /\
    Schema.summary_body typed = body /\
    Semantic.sem_body semantic = body /\
    Semantic.sem_definitely_refused semantic = false /\
    exists score,
      Semantic.sem_score_evidence semantic =
        Semantic.ExactScore score.
Proof.
  intros hash_fn body recurse semantic_default typed semantic accepted.
  unfold par_step in accepted.
  destruct (Schema.lookup_projected_program 1 [] [])
    as [program|]; [|discriminate].
  destruct (Source.decode_root 1 body)
    as [nodes|]; [|discriminate].
  destruct (source_children_reference
    (guarded_par_child body recurse) hash_fn body program nodes)
    as [children|]; [|discriminate].
  destruct (Source.source_par_semantic_default body nodes)
    as [observed|]; [|discriminate].
  set (reduced_typed := Schema.reduce_field (par_raw body)
    (map result_typed children)) in *.
  set (reduced_semantic := Semantic.reduce_semantic_field
    (par_raw body) (map result_semantic children)) in *.
  destruct (Schema.summary_valid reduced_typed)
    eqn:valid; [|discriminate].
  destruct (Semantic.sem_definitely_refused reduced_semantic)
    eqn:not_refused; [discriminate|].
  destruct (Semantic.sem_score_evidence reduced_semantic)
    as [score|] eqn:scored; [|discriminate].
  inversion accepted; subst typed semantic.
  split; [exact valid|].
  split.
  - unfold reduced_typed, Schema.reduce_field, par_raw.
    reflexivity.
  - split.
    + unfold reduced_semantic, Semantic.reduce_semantic_field,
        par_raw.
      reflexivity.
    + split; [exact not_refused|].
      exists score. exact scored.
Qed.

Theorem accepted_par_preserves_typed_validity_body_and_exact_score :
  forall hash_fn body semantic_default typed semantic,
    par_reference hash_fn body =
      Some (semantic_default, (typed, semantic)) ->
    Schema.summary_valid typed = true /\
    Schema.summary_body typed = body /\
    Semantic.sem_body semantic = body /\
    Semantic.sem_definitely_refused semantic = false /\
    exists score,
      Semantic.sem_score_evidence semantic =
        Semantic.ExactScore score.
Proof.
  intros hash_fn body semantic_default typed semantic accepted.
  unfold par_reference, Fix in accepted.
  destruct (shorter_body_is_well_founded body) as [recurse].
  cbn [Fix_F] in accepted.
  now apply accepted_par_step_preserves_typed_validity_body_and_exact_score
    in accepted.
Qed.

Lemma checked_generated_raw_has_valid_empty_requirements :
  forall bytes program node raw,
    checked_generated_raw bytes program node = Some raw ->
    Schema.valid_raw_body raw = true /\
    Schema.raw_wire_required raw = [] /\
    Schema.raw_semantic_required raw = [].
Proof.
  intros bytes program node raw checked.
  change (Recursive.checked_generated_raw bytes program node =
    Some raw) in checked.
  destruct (Recursive.checked_generated_raw_preserves_descriptor_body_and_validity
    bytes program node raw checked) as
    [_ [source_raw [_ valid]]].
  destruct (source_raw_has_empty_required_slots bytes node raw
    source_raw) as [empty_wire empty_semantic].
  repeat split; assumption.
Qed.

Lemma reduced_fields_preserve_summary_alignment :
  forall raw typed_children semantic_children,
    Schema.raw_wire_required raw = [] ->
    Schema.raw_semantic_required raw = [] ->
    Schema.summary_valid (Schema.reduce_field raw typed_children) = true ->
    Semantic.sem_definitely_refused
      (Semantic.reduce_semantic_field raw semantic_children) = false ->
    summary_alignment (Schema.reduce_field raw typed_children)
      (Semantic.reduce_semantic_field raw semantic_children).
Proof.
  intros raw typed_children semantic_children empty_wire
    empty_semantic valid not_refused.
  unfold summary_alignment.
  refine (conj valid (conj not_refused
    (conj eq_refl (conj eq_refl
      (conj eq_refl (conj _ eq_refl)))))).
  now apply source_raw_reducers_have_same_program_family.
Qed.

Lemma auxiliary_summaries_are_aligned :
  forall raw score,
    summary_alignment (auxiliary_typed_summary raw)
      (auxiliary_semantic_summary raw score).
Proof.
  intros raw score.
  unfold summary_alignment, auxiliary_typed_summary,
    auxiliary_semantic_summary.
  repeat split; reflexivity.
Qed.

Lemma accepted_string_map_score_has_aligned_summaries :
  forall raw entry semantic,
    string_map_semantic_summary raw entry = Some semantic ->
    summary_alignment (auxiliary_typed_summary raw) semantic.
Proof.
  intros raw entry semantic scored.
  unfold string_map_semantic_summary in scored.
  destruct (Source.returned_map_par Semantic.semantic_summary entry)
    as [value|].
  - destruct (Semantic.sem_score_evidence value)
      as [score|]; [|discriminate].
    inversion scored; subst semantic.
    apply auxiliary_summaries_are_aligned.
  - inversion scored; subst semantic.
    apply auxiliary_summaries_are_aligned.
Qed.

Lemma accepted_auxiliary_reference_has_aligned_summaries :
  forall par_child hash_fn bytes program slot node result,
    auxiliary_reference par_child hash_fn bytes program slot node =
      Some result ->
    summary_alignment (result_typed result) (result_semantic result).
Proof.
  intros par_child hash_fn bytes program slot node result accepted.
  unfold auxiliary_reference in accepted.
  destruct node as [span field|span field children|span field];
    try discriminate.
  destruct (checked_generated_raw bytes program
    (Source.SourceAuxiliary span field)) as [raw|];
    [|discriminate].
  destruct (Schema.descriptor_slot field =? slot);
    [|discriminate].
  destruct (Schema.descriptor_payload field);
    try discriminate.
  - destruct (Source.reduce_source_map_node bytes program slot
      par_child (Source.SourceAuxiliary span field))
      as [entry|]; [|discriminate].
    destruct (string_map_semantic_summary raw
      (semantic_entry_of_pair_entry entry)) as [semantic|]
      eqn:semantic_result;
      [|discriminate].
    inversion accepted; subst result.
    eapply accepted_string_map_score_has_aligned_summaries.
    exact semantic_result.
  - destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)))
      as [[snapshot trie]|]; [|discriminate].
    destruct (Source.sequence_source_results
      (map par_child (EPM1.value_bodies snapshot)))
      as [values|]; [|discriminate].
    inversion accepted; subst result.
    apply auxiliary_summaries_are_aligned.
Qed.

Theorem accepted_source_reference_has_aligned_summaries :
  forall par_child hash_fn bytes program node result,
    source_reference par_child hash_fn bytes program node =
      Some result ->
    summary_alignment (result_typed result) (result_semantic result).
Proof.
  intros par_child hash_fn bytes program node result accepted.
  destruct node as [span field|span field children|span field].
  - cbn [source_reference] in accepted.
    destruct (checked_generated_scalar_raw bytes program
      (Source.SourceScalar span field)) as [raw|]
      eqn:checked; [|discriminate].
    destruct (Schema.summary_valid (Schema.reduce_field raw []))
      eqn:valid; [|discriminate].
    destruct (Semantic.sem_definitely_refused
      (Semantic.reduce_semantic_field raw []))
      eqn:not_refused; [discriminate|].
    inversion accepted; subst result.
    assert (generated : checked_generated_raw bytes program
      (Source.SourceScalar span field) = Some raw).
    { change (Recursive.checked_generated_scalar_raw bytes program
        (Source.SourceScalar span field) = Some raw) in checked.
      exact (proj1
        (Recursive.checked_generated_scalar_uses_generated_and_grammar_validated_raw
          bytes program (Source.SourceScalar span field) raw checked)). }
    destruct (checked_generated_raw_has_valid_empty_requirements
      bytes program (Source.SourceScalar span field) raw generated)
      as [_ [empty_wire empty_semantic]].
    eapply reduced_fields_preserve_summary_alignment;
      eassumption.
  - cbn [source_reference] in accepted.
    destruct (checked_generated_raw bytes program
      (Source.SourceMessage span field children)) as [raw|]
      eqn:checked; [|discriminate].
    destruct (Schema.descriptor_child_type_id field)
      as [type_id|]; [|discriminate].
    destruct (Schema.lookup_projected_program type_id [] [])
      as [child_program|]; [|discriminate].
    destruct
      (let fix visit_children
          (remaining : list Source.source_tree)
          : option (list typed_semantic_result) :=
        match remaining with
        | [] => Some []
        | first :: rest =>
            match source_reference par_child hash_fn bytes
                    child_program first,
                  visit_children rest with
            | Some head, Some tail => Some (head :: tail)
            | _, _ => None
            end
        end in visit_children children)
      as [converted|] eqn:converted_result;
      [|discriminate].
    destruct (if type_id =? 20
      then ordered_new_map_entries None
        (new_map_typed_entries converted)
      else true) eqn:ordered; [|discriminate].
    destruct (Schema.summary_valid
      (Schema.reduce_field raw (map result_typed converted)))
      eqn:valid; [|discriminate].
    destruct (Semantic.sem_definitely_refused
      (Semantic.reduce_semantic_field raw
        (map result_semantic converted)))
      eqn:not_refused; [discriminate|].
    inversion accepted; subst result.
    destruct (checked_generated_raw_has_valid_empty_requirements
      bytes program (Source.SourceMessage span field children)
      raw checked) as [_ [empty_wire empty_semantic]].
    eapply reduced_fields_preserve_summary_alignment;
      eassumption.
  - cbn [source_reference] in accepted.
    eapply accepted_auxiliary_reference_has_aligned_summaries.
    exact accepted.
Qed.

Lemma empty_requirement_raw_programs_agree :
  forall raw,
    Schema.raw_wire_required raw = [] ->
    Schema.raw_semantic_required raw = [] ->
    Schema.resolved_raw_program raw =
      Semantic.generated_wire_program raw.
Proof.
  intros [tag wire payload body declared type_id wire_required
    semantic_required] empty_wire empty_semantic.
  cbn in empty_wire, empty_semantic.
  subst wire_required semantic_required.
  unfold Schema.resolved_raw_program,
    Semantic.generated_wire_program.
  destruct payload; destruct type_id; reflexivity.
Qed.

Lemma checked_message_raw_has_message_payload :
  forall bytes program span field children raw,
    checked_generated_raw bytes program
      (Source.SourceMessage span field children) = Some raw ->
    Schema.raw_payload raw = Schema.Message.
Proof.
  intros bytes program span field children raw checked.
  change (Recursive.checked_generated_raw bytes program
    (Source.SourceMessage span field children) = Some raw)
    in checked.
  destruct (Recursive.checked_generated_raw_preserves_descriptor_body_and_validity
    bytes program (Source.SourceMessage span field children)
    raw checked) as [_ [sourced _]].
  cbn [Source.raw_field_of_source Source.source_span
    Source.source_field Source.source_body] in sourced.
  destruct (Schema.payload_eqb
    (Schema.descriptor_payload field) Schema.Message);
    [|discriminate].
  destruct (Schema.descriptor_child_type_id field);
    inversion sourced; reflexivity.
Qed.

Lemma semantic_reducer_nonrefusal_requires_a_valid_scan :
  forall raw children program,
    Semantic.generated_wire_program raw = Some program ->
    Semantic.sem_definitely_refused
      (Semantic.reduce_semantic_field raw children) = false ->
    Schema.scan_valid
      (fold_left (Semantic.scan_child program) children
        Schema.empty_scan) = true.
Proof.
  intros raw children program resolved accepted.
  unfold Semantic.reduce_semantic_field in accepted.
  rewrite resolved in accepted.
  cbn [Semantic.sem_definitely_refused] in accepted.
  destruct (Schema.scan_valid
    (fold_left (Semantic.scan_child program) children
      Schema.empty_scan)) eqn:scan_valid;
    [reflexivity|].
  cbn [negb] in accepted.
  repeat rewrite Bool.orb_true_r in accepted.
  cbn [orb] in accepted.
  discriminate.
Qed.

Lemma accepted_source_children_have_aligned_summaries :
  forall par_child hash_fn bytes program nodes results,
    source_children_reference par_child hash_fn bytes program nodes =
      Some results ->
    Forall (fun result =>
      summary_alignment (result_typed result)
        (result_semantic result)) results.
Proof.
  intros par_child hash_fn bytes program nodes.
  induction nodes as [|first rest IH]; intros results accepted.
  - inversion accepted; constructor.
  - cbn [source_children_reference] in accepted.
    destruct (source_reference par_child hash_fn bytes program first)
      as [head|] eqn:converted_head; [|discriminate].
    destruct (source_children_reference par_child hash_fn bytes
      program rest) as [tail|] eqn:converted_rest;
      [|discriminate].
    inversion accepted; subst results.
    constructor.
    + exact (accepted_source_reference_has_aligned_summaries
        par_child hash_fn bytes program first head converted_head).
    + exact (IH tail eq_refl).
Qed.

Lemma projected_program_has_requested_wire_slots :
  forall type_id wire_required semantic_required program,
    Schema.lookup_projected_program type_id wire_required
      semantic_required = Some program ->
    Schema.program_wire_required program = wire_required.
Proof.
  intros type_id wire_required semantic_required program looked_up.
  unfold Schema.lookup_projected_program in looked_up.
  destruct (find (fun candidate =>
    GeneratedData.program_type_id candidate =? type_id)
    GeneratedData.generated_programs) as [candidate|];
    [|discriminate].
  inversion looked_up; subst program.
  reflexivity.
Qed.

Lemma accepted_message_semantics_implies_typed_validity :
  forall raw results,
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_wire_required raw = [] ->
    Schema.raw_semantic_required raw = [] ->
    Schema.valid_raw_body raw = true ->
    Forall (fun result =>
      summary_alignment (result_typed result)
        (result_semantic result)) results ->
    Semantic.sem_definitely_refused
      (Semantic.reduce_semantic_field raw
        (map result_semantic results)) = false ->
    Schema.summary_valid
      (Schema.reduce_field raw (map result_typed results)) = true.
Proof.
  intros raw results message empty_wire empty_semantic valid_raw
    aligned accepted.
  destruct (Semantic.generated_wire_program raw)
    as [program|] eqn:generated.
  2: {
    unfold Semantic.reduce_semantic_field in accepted.
    rewrite generated in accepted.
    cbn [Semantic.sem_definitely_refused negb orb] in accepted.
    discriminate.
  }
  assert (resolved : Schema.resolved_raw_program raw = Some program).
  { rewrite (empty_requirement_raw_programs_agree raw
      empty_wire empty_semantic).
    exact generated. }
  assert (required_empty : Schema.program_wire_required program = []).
  { unfold Semantic.generated_wire_program in generated.
    rewrite message in generated.
    destruct (Schema.raw_program_type_id raw) as [type_id|];
      [|discriminate].
    exact (projected_program_has_requested_wire_slots
      type_id [] [] program generated). }
  pose proof (semantic_reducer_nonrefusal_requires_a_valid_scan
    raw (map result_semantic results) program generated accepted)
    as semantic_scan_valid.
  pose proof (aligned_scan_fold_after_erasing_scores
    program results Schema.empty_scan aligned) as scanned.
  pose proof (f_equal Schema.scan_valid scanned) as same_scan_valid.
  change
    (Schema.scan_valid
      (fold_left (Schema.checked_field program)
        (map result_typed results) Schema.empty_scan) =
     Schema.scan_valid
      (fold_left (Semantic.scan_child program)
        (map result_semantic results) Schema.empty_scan))
    in same_scan_valid.
  rewrite semantic_scan_valid in same_scan_valid.
  unfold Schema.reduce_field.
  rewrite resolved.
  cbn [Schema.summary_valid].
  rewrite message, valid_raw, same_scan_valid, required_empty.
  reflexivity.
Qed.

Lemma semantic_projection_preserves_summary :
  forall result,
    Recursive.recursive_result_semantic
      (semantic_projection result) = result_semantic result.
Proof.
  intro result.
  destruct result; reflexivity.
Qed.

Lemma semantic_projection_preserves_child_summaries :
  forall results,
    map Recursive.recursive_result_semantic
      (map semantic_projection results) =
    map result_semantic results.
Proof.
  induction results as [|first rest IH]; [reflexivity|].
  simpl.
  rewrite semantic_projection_preserves_summary, IH.
  reflexivity.
Qed.

Lemma local_source_children_are_global_children :
  forall par_child hash_fn bytes program nodes,
    (let fix visit_children
        (remaining : list Source.source_tree)
        : option (list typed_semantic_result) :=
      match remaining with
      | [] => Some []
      | first :: rest =>
          match source_reference par_child hash_fn bytes program first,
                visit_children rest with
          | Some head, Some tail => Some (head :: tail)
          | _, _ => None
          end
      end in visit_children nodes) =
    source_children_reference par_child hash_fn bytes program nodes.
Proof.
  intros par_child hash_fn bytes program nodes.
  induction nodes as [|first rest IH]; simpl;
    [reflexivity|].
  now rewrite <- IH.
Qed.

Lemma source_children_projection_commutes :
  forall paired_child semantic_child hash_fn bytes program nodes,
    (forall node,
      In node nodes ->
      option_map semantic_projection
        (source_reference paired_child hash_fn bytes program node) =
      Recursive.recursive_source_reference semantic_child
        hash_fn bytes program node) ->
    option_map (map semantic_projection)
      (source_children_reference paired_child hash_fn bytes
        program nodes) =
    Recursive.recursive_source_children semantic_child
      hash_fn bytes program nodes.
Proof.
  intros paired_child semantic_child hash_fn bytes program nodes.
  induction nodes as [|first rest IH]; intros children_agree;
    [reflexivity|].
  cbn [source_children_reference
    Recursive.recursive_source_children].
  rewrite <- (children_agree first (or_introl eq_refl)).
  rewrite <- (IH (fun node member =>
    children_agree node (or_intror member))).
  destruct (source_reference paired_child hash_fn bytes program first)
    as [head|];
    destruct (source_children_reference paired_child hash_fn bytes
      program rest) as [tail|];
    reflexivity.
Qed.
