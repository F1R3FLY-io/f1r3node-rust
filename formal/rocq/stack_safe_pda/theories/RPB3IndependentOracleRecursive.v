From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import EPM1 EPM1ACTree03
  RPB3DescriptorProjectionData RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleCost
  RPB3IndependentOracleProcess RPB3IndependentOracleName.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Cost := RPB3IndependentOracleCost.
Module Process := RPB3IndependentOracleProcess.
Module Name := RPB3IndependentOracleName.

Definition source_par_raw (body : list nat) : Schema.raw_field :=
  Schema.RawField 0 Schema.LengthDelimited Schema.Message
    body (length body) (Some 1) [] [].

Fixpoint source_par_component_scores
    (children : list Core.source_exact_summary)
    : option (list Core.source_score) :=
  match children with
  | [] => Some []
  | child :: rest =>
      match source_par_component_scores rest with
      | None => None
      | Some scores =>
          match Core.source_summary_value child with
          | Core.TypedSourceMessage _ _ =>
              option_map (fun score => score :: scores)
                (Core.source_summary_exact_score child)
          | Core.TypedSourceScalar _ _ => Some scores
          | Core.TypedSourceStringMap _ _ _
          | Core.TypedSourcePathMap _ _ _ => None
          end
      end
  end.

Definition source_par_score (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_par_component_scores children,
      Process.source_bool_or_absent 10 children with
  | Some components, Some connective =>
      Some (Core.source_score_constructor 999
        (components ++ [Process.source_bool_word connective]))
  | _, _ => None
  end.

(** The serialization is separately defined over the source-owned score
    constructors.  It is used only for canonical Par ordering; no target
    score comparator is called. *)
Definition source_integer_bytes (bits : N) : list nat :=
  [1; 1;
   N.to_nat (N.lxor (N.land (N.shiftr bits 56) 255) 128);
   N.to_nat (N.land (N.shiftr bits 48) 255);
   N.to_nat (N.land (N.shiftr bits 40) 255);
   N.to_nat (N.land (N.shiftr bits 32) 255);
   N.to_nat (N.land (N.shiftr bits 24) 255);
   N.to_nat (N.land (N.shiftr bits 16) 255);
   N.to_nat (N.land (N.shiftr bits 8) 255);
   N.to_nat (N.land bits 255)].

Fixpoint source_terminated_bytes (bytes : list nat) : list nat :=
  match bytes with
  | [] => [0; 0]
  | byte :: rest =>
      if byte =? 0 then 0 :: 255 :: source_terminated_bytes rest
      else byte :: source_terminated_bytes rest
  end.

Fixpoint source_score_bytes (score : Core.source_score) : list nat :=
  match score with
  | Core.SourceInteger value => source_integer_bytes value
  | Core.SourceString bytes => 1 :: 2 :: source_terminated_bytes bytes
  | Core.SourceBytes bytes => 1 :: 3 :: source_terminated_bytes bytes
  | Core.SourceNode children => 2 :: source_score_forest_bytes children
  end
with source_score_forest_bytes (scores : Core.source_score_forest)
    : list nat :=
  match scores with
  | Core.SourceNil => [0]
  | Core.SourceCons score rest =>
      source_score_bytes score ++ source_score_forest_bytes rest
  end.

Definition source_score_body_order
    (left right : Core.source_exact_summary) : option bool :=
  match Core.source_summary_exact_score left,
      Core.source_summary_exact_score right with
  | Some left_score, Some right_score =>
      let left_bytes := source_score_bytes left_score in
      let right_bytes := source_score_bytes right_score in
      if Process.source_bytes_leq left_bytes right_bytes &&
          Process.source_bytes_leq right_bytes left_bytes
      then Some (Process.source_bytes_leq
        (Core.source_summary_body left) (Core.source_summary_body right))
      else Some (Process.source_bytes_leq left_bytes right_bytes)
  | _, _ => None
  end.

Definition source_and_known (left right : option bool) : option bool :=
  match left, right with
  | Some false, _ | _, Some false => Some false
  | Some true, Some true => Some true
  | _, _ => None
  end.

Fixpoint source_par_component_order
    (children : list Core.source_exact_summary) : option bool :=
  match children with
  | [] | [_] => Some true
  | first :: ((second :: _) as rest) =>
      source_and_known
        (if Core.source_summary_tag first =?
            Core.source_summary_tag second
         then source_score_body_order first second else Some true)
        (source_par_component_order rest)
  end.

Definition source_order_definitely_bad (order : option bool) : bool :=
  match order with Some false => true | _ => false end.

Fixpoint source_string_map_keys_ordered_after
    (previous : option (nat * list nat))
    (children : list Core.source_exact_summary) : bool :=
  match children with
  | [] => true
  | child :: rest =>
      match Core.source_summary_value child with
      | Core.TypedSourceStringMap raw key _ =>
          let tag := Schema.raw_tag raw in
          let follows :=
            match previous with
            | Some (prior_tag, prior_key) =>
                if prior_tag =? tag
                then Source.byte_lex_lt prior_key key else true
            | None => true
            end in
          follows && source_string_map_keys_ordered_after
            (Some (tag, key)) rest
      | _ => source_string_map_keys_ordered_after None rest
      end
  end.

Definition source_string_map_keys_ordered
    (children : list Core.source_exact_summary) : bool :=
  source_string_map_keys_ordered_after None children.

Definition source_message_completeness (type_id : nat)
    (scan : Admission.source_admission_state)
    (children : list Core.source_exact_summary) : option bool :=
  if type_id =? 13 then Some true
  else match Process.source_process_completeness type_id scan children with
  | Some complete => Some complete
  | None => Name.source_name_completeness type_id scan
  end.

Definition source_message_score (type_id : nat)
    (complete : option bool)
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  if type_id =? 1 then source_par_score children
  else if Cost.cost_family type_id then
    if (type_id =? 13) &&
        negb (match complete with Some true => true | _ => false end)
    then None else Cost.cost_family_score type_id children
  else match Process.source_process_family_score type_id complete children with
  | Some score => Some score
  | None => Name.source_name_family_score type_id complete children
  end.

Definition source_message_extra_refused (type_id : nat)
    (complete : option bool)
    (scan : Admission.source_admission_state)
    (children : list Core.source_exact_summary) : bool :=
  Process.source_process_family_refused type_id complete scan children ||
    (if type_id =? 1
     then source_order_definitely_bad (source_par_component_order children)
     else false).

Definition source_message_summary (raw : Schema.raw_field)
    (program : Schema.generated_program)
    (children : list Core.source_exact_summary)
    : Core.source_exact_summary :=
  let type_id := Schema.program_family program in
  let scan := Admission.source_scan_children program children in
  let complete := source_message_completeness type_id scan children in
  let value := Core.TypedSourceMessage raw
    (map Core.source_summary_value children) in
  let refused :=
    negb (Admission.source_raw_bytes_well_formed raw) ||
    negb (Admission.source_scan_valid scan) ||
    negb (Admission.source_required_pathmaps_present program scan) ||
    negb (source_string_map_keys_ordered children) ||
    source_message_extra_refused type_id complete scan children in
  Core.SourceExactSummary value
    (Core.SourceFamilyAnalysis complete
      (source_message_score type_id complete children) refused
      (if type_id =? 16
       then Process.source_bind_source_present scan else false)
      (if type_id =? 21
       then Process.source_case_required_present scan else false))
    (if type_id =? 1 then Process.source_par_typed_default value else None).

Definition source_valid_string_map_key
    (source : option (Source.Wire.field_span * list nat))
    : option (list nat) :=
  match source with
  | None => Some []
  | Some (_, key) =>
      if Core.bytes_in_range key &&
          Source.indexed_scan_utf8_accepts key &&
          match key with [] => false | _ => true end
      then Some key else None
  end.

(** One source node evaluation takes its child callback as an explicit
    argument.  The enclosing message step uses this same branch body once,
    and the named function exposes the positional source result to proofs. *)
Definition source_evaluate_node
    (recurse : Schema.raw_field -> option Core.source_exact_summary)
    (hash_fn : list nat -> nat) (body : list nat)
    (program : Schema.generated_program) (node : Source.source_tree)
    : option Core.source_exact_summary :=
                    match Source.raw_field_of_source body node with
                    | None => None
                    | Some child_raw =>
                        match node with
                        | Source.SourceScalar _ _ =>
                            option_map (fun value =>
                              Core.SourceExactSummary value
                                (Core.SourceFamilyAnalysis
                                  None None false false false) None)
                              (Admission.source_typed_scalar
                                program child_raw)
                        | Source.SourceMessage _ _ _ =>
                            recurse child_raw
                        | Source.SourceAuxiliary _ field =>
                            match Schema.descriptor_payload field with
                            | Schema.StringMap =>
                                match Source.refine_string_map_entry
                                    body program node with
                                | None => None
                                | Some entry =>
                                    match source_valid_string_map_key
                                        (Source.typed_map_key_source entry) with
                                    | None => None
                                    | Some key =>
                                        let nested :=
                                          match Source.typed_map_value_source
                                              entry with
                                          | None => Some None
                                          | Some (_, value_body) =>
                                              match recurse
                                                  (source_par_raw value_body) with
                                              | Some summary =>
                                                  match Core.source_summary_semantic_default
                                                      summary with
                                                  | Some false =>
                                                      if Core.source_summary_refused
                                                          summary
                                                      then None
                                                      else Some (Some
                                                        (false, summary))
                                                  | _ => None
                                                  end
                                              | None => None
                                              end
                                          end in
                                        match nested with
                                        | None => None
                                        | Some selected =>
                                            match Name.source_string_map_auxiliary_score
                                                key selected with
                                            | None => None
                                            | Some score =>
                                                Some (Core.SourceExactSummary
                                                  (Core.TypedSourceStringMap
                                                    child_raw key
                                                    (option_map (fun paired =>
                                                      (fst paired,
                                                        Core.source_summary_value
                                                          (snd paired)))
                                                      selected))
                                                  (Core.SourceFamilyAnalysis
                                                    (Some true) (Some score)
                                                    false false false) None)
                                            end
                                        end
                                    end
                                end
                            | Schema.PathMap =>
                                match EPM1ACTree03.decode_epm1_actree03
                                    hash_fn (Schema.raw_body child_raw) with
                                | None => None
                                | Some (snapshot, _) =>
                                    let selected :=
                                      Source.sequence_source_results
                                        (map (fun value_body =>
                                          match recurse
                                              (source_par_raw value_body) with
                                          | Some summary =>
                                              match Core.source_summary_semantic_default
                                                  summary with
                                              | Some false =>
                                                  if Core.source_summary_refused
                                                      summary
                                                  then None else Some
                                                    (false, summary)
                                              | _ => None
                                              end
                                          | None => None
                                          end) (EPM1.value_bodies snapshot)) in
                                    match selected with
                                    | None => None
                                    | Some values =>
                                        Some (Core.SourceExactSummary
                                          (Core.TypedSourcePathMap
                                            child_raw snapshot
                                            (map (fun paired =>
                                              (fst paired,
                                                Core.source_summary_value
                                                  (snd paired))) values))
                                          (Core.SourceFamilyAnalysis
                                            (Some true)
                                            (Some
                                              (Name.source_pathmap_auxiliary_score
                                                child_raw))
                                            false false false) None)
                                    end
                                end
                            | Schema.Scalar | Schema.Message => None
                            end
                        end
                    end.

(** One source evaluation layer takes its child callback as an explicit
    argument.  This keeps recursive calls named in the fold/unfold equation,
    so finite-body fuel reasoning can compare them without reducing an inner
    generated Par program. *)
Definition source_evaluate_message_step
    (recurse : Schema.raw_field -> option Core.source_exact_summary)
    (hash_fn : list nat -> nat) (raw : Schema.raw_field)
    : option Core.source_exact_summary :=
      match Schema.raw_payload raw, Schema.raw_program_type_id raw with
      | Schema.Message, Some type_id =>
          match Schema.lookup_projected_program type_id [] [] with
          | None => None
          | Some program =>
              let body := Schema.raw_body raw in
              match Source.decode_region (S (length body)) body program
                  0 (length body) with
              | None => None
              | Some nodes =>
                  option_map (source_message_summary raw program)
                    (Source.sequence_source_results
                      (map (source_evaluate_node recurse hash_fn body
                        program) nodes))
              end
          end
      | _, _ => None
      end.

Lemma source_evaluate_message_step_selected_children :
  forall recurse hash_fn raw type_id program nodes,
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    Source.decode_region (S (length (Schema.raw_body raw)))
      (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
      Some nodes ->
    source_evaluate_message_step recurse hash_fn raw =
      option_map (source_message_summary raw program)
        (Source.sequence_source_results
          (map (source_evaluate_node recurse hash_fn
            (Schema.raw_body raw) program) nodes)).
Proof.
  intros recurse hash_fn raw type_id program nodes
    payload family looked_up decoded.
  unfold source_evaluate_message_step.
  now rewrite payload, family, looked_up, decoded.
Qed.

(** Every recursive call decreases [fuel].  The public root starts at
    [S (length body)]; byte adequacy for all decoded sub-bodies is proved
    separately. *)
Fixpoint source_evaluate_message (fuel : nat)
    (hash_fn : list nat -> nat) (raw : Schema.raw_field) {struct fuel}
    : option Core.source_exact_summary :=
  match fuel with
  | 0 => None
  | S smaller =>
      source_evaluate_message_step
        (source_evaluate_message smaller hash_fn) hash_fn raw
  end.

Lemma source_evaluate_message_unfold :
  forall fuel hash_fn raw,
    source_evaluate_message (S fuel) hash_fn raw =
      source_evaluate_message_step
        (source_evaluate_message fuel hash_fn) hash_fn raw.
Proof. reflexivity. Qed.

Definition source_par_reference (hash_fn : list nat -> nat)
    (body : list nat) : option (bool * Core.source_exact_summary) :=
  match source_evaluate_message (S (length body)) hash_fn
      (source_par_raw body) with
  | Some summary =>
      if Core.source_summary_refused summary then None
      else option_map (fun semantic_default => (semantic_default, summary))
        (Core.source_summary_semantic_default summary)
  | None => None
  end.

Lemma source_par_score_preserves_components :
  forall children scores connective,
    source_par_component_scores children = Some scores ->
    Process.source_bool_or_absent 10 children = Some connective ->
    source_par_score children =
      Some (Core.source_score_constructor 999
        (scores ++ [Process.source_bool_word connective])).
Proof.
  intros children scores connective collected flag.
  unfold source_par_score.
  now rewrite collected, flag.
Qed.

Lemma source_message_summary_retains_raw_body :
  forall raw program children,
    Core.source_summary_body
      (source_message_summary raw program children) =
      Schema.raw_body raw.
Proof. reflexivity. Qed.

Lemma source_message_summary_refuses_invalid_scan :
  forall raw program children,
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = false ->
    Core.source_summary_refused
      (source_message_summary raw program children) = true.
Proof.
  intros raw program children invalid.
  unfold source_message_summary.
  rewrite invalid. simpl.
  rewrite orb_true_r. reflexivity.
Qed.

Lemma source_par_reference_accepts_only_unrefused :
  forall hash_fn body semantic_default summary,
    source_par_reference hash_fn body =
      Some (semantic_default, summary) ->
    Core.source_summary_refused summary = false /\
    Core.source_summary_semantic_default summary = Some semantic_default.
Proof.
  intros hash_fn body semantic_default summary accepted.
  unfold source_par_reference in accepted.
  destruct (source_evaluate_message (S (length body)) hash_fn
      (source_par_raw body)) as [candidate|] eqn:evaluated;
    [|discriminate].
  destruct (Core.source_summary_refused candidate) eqn:refused;
    [discriminate|].
  destruct (Core.source_summary_semantic_default candidate)
    as [observed|] eqn:default; [|discriminate].
  inversion accepted; subst summary semantic_default.
  now split.
Qed.

Print Assumptions source_par_score_preserves_components.
Print Assumptions source_message_summary_retains_raw_body.
Print Assumptions source_message_summary_refuses_invalid_scan.
Print Assumptions source_par_reference_accepts_only_unrefused.
Print Assumptions source_evaluate_message_step_selected_children.
Print Assumptions source_evaluate_message_unfold.
