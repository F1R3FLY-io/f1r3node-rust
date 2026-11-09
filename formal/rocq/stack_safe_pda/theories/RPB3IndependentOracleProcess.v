From Stdlib Require Import List Bool Arith NArith ZArith.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.

Definition source_child_at (tag : nat)
    (children : list Core.source_exact_summary)
    : option Core.source_exact_summary :=
  find (fun child => Core.source_summary_tag child =? tag) children.

Definition source_child_score (tag : nat)
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_child_at tag children with
  | Some child => Core.source_summary_exact_score child
  | None => None
  end.

Fixpoint source_selected_scores (tag : nat)
    (children : list Core.source_exact_summary)
    : option (list Core.source_score) :=
  match children with
  | [] => Some []
  | child :: rest =>
      match source_selected_scores tag rest with
      | None => None
      | Some scores =>
          if Core.source_summary_tag child =? tag then
            option_map (fun score => score :: scores)
              (Core.source_summary_exact_score child)
          else Some scores
      end
  end.

Definition source_optional_score (tag : nat)
    (children : list Core.source_exact_summary)
    : option (option Core.source_score) :=
  match source_child_at tag children with
  | None => Some None
  | Some child => option_map Some (Core.source_summary_exact_score child)
  end.

Definition source_optional_score_or (tag : nat)
    (fallback : Core.source_score)
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_optional_score tag children with
  | Some None => Some fallback
  | Some (Some score) => Some score
  | None => None
  end.

Definition source_typed_message_score (type_id : nat)
    (child : Core.source_exact_summary)
    : option Core.source_score :=
  match Core.source_summary_value child,
      Core.source_summary_type_id child with
  | Core.TypedSourceMessage _ _, Some observed =>
      if (observed =? type_id) &&
          negb (Core.source_summary_refused child)
      then Core.source_summary_exact_score child else None
  | _, _ => None
  end.

Definition source_typed_child_score (tag type_id : nat)
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_child_at tag children with
  | Some child => source_typed_message_score type_id child
  | None => None
  end.

Fixpoint source_typed_selected_scores (tag type_id : nat)
    (children : list Core.source_exact_summary)
    : option (list Core.source_score) :=
  match children with
  | [] => Some []
  | child :: rest =>
      match source_typed_selected_scores tag type_id rest with
      | None => None
      | Some scores =>
          if Core.source_summary_tag child =? tag then
            option_map (fun score => score :: scores)
              (source_typed_message_score type_id child)
          else Some scores
      end
  end.

Definition source_typed_optional_score (tag type_id : nat)
    (children : list Core.source_exact_summary)
    : option (option Core.source_score) :=
  match source_child_at tag children with
  | None => Some None
  | Some child => option_map Some (source_typed_message_score type_id child)
  end.

Definition source_typed_optional_score_or (tag type_id : nat)
    (fallback : Core.source_score)
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_typed_optional_score tag type_id children with
  | Some None => Some fallback
  | Some (Some score) => Some score
  | None => None
  end.

Definition source_bool_or_absent (tag : nat)
    (children : list Core.source_exact_summary) : option bool :=
  match source_child_at tag children with
  | None => Some false
  | Some child =>
      if Core.source_summary_refused child then None else
      match Core.source_summary_value child with
      | Core.TypedSourceScalar _ (Core.SourceBool value) => Some value
      | _ => None
      end
  end.

Definition source_i32_or_absent (tag : nat)
    (children : list Core.source_exact_summary) : option Z :=
  match source_child_at tag children with
  | None => Some 0%Z
  | Some child =>
      if Core.source_summary_refused child then None else
      match Core.source_summary_value child with
      | Core.TypedSourceScalar _ (Core.SourceI32 value) => Some value
      | _ => None
      end
  end.

Definition source_signed_word (value : Z) : N :=
  Z.to_N (Z.modulo value 18446744073709551616).

Definition source_bool_word (value : bool) : Core.source_score :=
  Core.SourceInteger (if value then 1 else 0).

Definition source_empty_par_score : Core.source_score :=
  Core.source_score_constructor 999 [Core.SourceInteger 0].

Definition source_bundle_constructor (read write : bool) : N :=
  if read then if write then 308 else 306
  else if write then 307 else 305.

Definition source_bundle_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_child_score 1 children,
      source_bool_or_absent 2 children,
      source_bool_or_absent 3 children with
  | Some body, Some write, Some read =>
      Some (Core.source_score_constructor
        (source_bundle_constructor read write) [body])
  | _, _, _ => None
  end.

Definition source_send_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_bool_or_absent 3 children,
      source_child_score 1 children,
      source_selected_scores 2 children,
      source_bool_or_absent 6 children,
      source_optional_score 7 children with
  | Some persistent, Some channel, Some data,
      Some connective, Some cost =>
      Some (Core.source_score_constructor 300
        (source_bool_word persistent :: channel ::
          (data ++ [source_bool_word connective] ++
            match cost with None => [] | Some score => [score] end)))
  | _, _, _, _, _ => None
  end.

Definition source_if_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_child_score 1 children, source_child_score 2 children,
      source_child_score 3 children, source_bool_or_absent 5 children with
  | Some condition, Some if_true, Some if_false, Some connective =>
      Some (Core.source_score_constructor 309
        [condition; if_true; if_false; source_bool_word connective])
  | _, _, _, _ => None
  end.

Definition source_scores_or_absent (scores : list Core.source_score)
    : list Core.source_score :=
  match scores with [] => [Core.SourceInteger 0] | _ => scores end.

Definition source_new_uris (children : list Core.source_exact_summary)
    : list Core.source_score :=
  map (fun child => Core.SourceString (Core.source_summary_body child))
    (filter (fun child => Core.source_summary_tag child =? 3) children).

Definition source_new_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_i32_or_absent 1 children,
      source_child_score 2 children,
      source_selected_scores 4 children with
  | Some count, Some body, Some injections =>
      Some (Core.source_score_constructor 303
        (Core.SourceInteger (source_signed_word count) ::
          (source_scores_or_absent (source_new_uris children) ++
           source_scores_or_absent injections ++ [body])))
  | _, _, _ => None
  end.

Definition source_receive_bind_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_typed_child_score 2 1 children,
      source_typed_selected_scores 1 1 children,
      source_typed_optional_score_or 3 6 (Core.SourceInteger 0) children,
      source_typed_optional_score 5 12 children with
  | Some source, Some patterns, Some remainder, Some cost =>
      Some (Core.SourceNode (Core.source_score_forest_of_list
        (source :: patterns ++ [remainder] ++
          match cost with None => [] | Some score => [score] end)))
  | _, _, _, _ => None
  end.

Definition source_match_case_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_typed_child_score 1 1 children,
      source_typed_child_score 2 1 children,
      source_i32_or_absent 3 children,
      source_typed_optional_score_or 4 1 source_empty_par_score children with
  | Some pattern, Some source, Some free_count, Some guard =>
      Some (Core.SourceNode (Core.source_score_forest_of_list
        [pattern; source;
         Core.SourceInteger (source_signed_word free_count); guard]))
  | _, _, _, _ => None
  end.

(** Semantic default is derived from the typed Par itself.  In the generated
    Par descriptor, the repeated message fields are precisely the process
    components.  A present connective is valid only as the true scalar [1]. *)
Definition source_par_component_tags : list nat :=
  match Schema.lookup_projected_program 1 [] [] with
  | None => []
  | Some program =>
      map Schema.descriptor_tag
        (filter (fun field =>
          Schema.descriptor_repeated field &&
          Schema.payload_eqb (Schema.descriptor_payload field)
            Schema.Message) (Schema.program_fields program))
  end.

Definition source_value_tag (value : Core.typed_source_value) : nat :=
  Schema.raw_tag (Core.source_value_raw value).

Definition source_par_typed_default (value : Core.typed_source_value)
    : option bool :=
  match value with
  | Core.TypedSourceMessage raw children =>
      match Schema.raw_program_type_id raw with
      | Some 1 =>
          let has_component := existsb (fun child =>
            existsb (Nat.eqb (source_value_tag child))
              source_par_component_tags) children in
          match find (fun child => source_value_tag child =? 10) children with
          | None => Some (negb has_component)
          | Some (Core.TypedSourceScalar connective (Core.SourceBool true)) =>
              match Schema.raw_body connective with
              | [1] => Some false
              | _ => None
              end
          | _ => None
          end
      | _ => None
      end
  | _ => None
  end.

Definition source_present_default_condition
    (children : list Core.source_exact_summary) : bool :=
  existsb (fun child =>
    (Core.source_summary_tag child =? 8) &&
    negb (Core.source_summary_refused child) &&
    match source_par_typed_default (Core.source_summary_value child) with
    | Some true => true
    | _ => false
    end) children.

Definition source_match_case_has_default_guard
    (child : Core.source_exact_summary) : bool :=
  if (Core.source_summary_tag child =? 2) &&
      negb (Core.source_summary_refused child) then
    match Core.source_summary_value child with
    | Core.TypedSourceMessage raw values =>
        match Schema.raw_program_type_id raw with
        | Some 21 =>
            match find (fun value => source_value_tag value =? 4) values with
            | Some guard =>
                match source_par_typed_default guard with
                | Some true => true
                | _ => false
                end
            | None => false
            end
        | _ => false
        end
    | _ => false
    end
  else false.

Definition source_present_default_match_guard
    (children : list Core.source_exact_summary) : bool :=
  existsb source_match_case_has_default_guard children.

Definition source_receive_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  if source_present_default_condition children then None else
  match source_bool_or_absent 3 children,
      source_bool_or_absent 4 children,
      source_typed_selected_scores 1 16 children,
      source_typed_child_score 2 1 children,
      source_i32_or_absent 5 children,
      source_bool_or_absent 7 children,
      source_typed_optional_score_or 8 1 source_empty_par_score children,
      source_typed_optional_score 9 12 children with
  | Some persistent, Some peek, Some binds, Some body,
      Some count, Some connective, Some condition, Some cost =>
      Some (Core.source_score_constructor 301
        ([source_bool_word persistent; source_bool_word peek] ++ binds ++
          [body; Core.SourceInteger (source_signed_word count);
           source_bool_word connective; condition] ++
          match cost with None => [] | Some score => [score] end))
  | _, _, _, _, _, _, _, _ => None
  end.

Definition source_match_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  if source_present_default_match_guard children then None else
  match source_typed_child_score 1 1 children,
      source_typed_selected_scores 2 21 children,
      source_bool_or_absent 5 children with
  | Some target, Some cases, Some connective =>
      Some (Core.source_score_constructor 304
        (target :: cases ++ [source_bool_word connective]))
  | _, _, _ => None
  end.

(** The generated BigRational and FixedPoint children have semantic fields
    but no standalone score.  Their Expr parent scores the typed fields.
    Absence has the same default as the production child reducers. *)
Definition source_ground_field_at (tag : nat)
    (values : list Core.typed_source_value)
    : option Core.typed_source_value :=
  find (fun value =>
    Schema.raw_tag (Core.source_value_raw value) =? tag) values.

Definition source_ground_blob_or_absent (tag : nat)
    (values : list Core.typed_source_value) : option (list nat) :=
  match source_ground_field_at tag values with
  | None => Some []
  | Some (Core.TypedSourceScalar raw (Core.SourceBlob bytes)) =>
      match Schema.raw_wire raw, Schema.raw_payload raw,
          Schema.raw_program_type_id raw with
      | Schema.LengthDelimited, Schema.Scalar, None => Some bytes
      | _, _, _ => None
      end
  | _ => None
  end.

Definition source_ground_u32_or_absent (tag : nat)
    (values : list Core.typed_source_value) : option N :=
  match source_ground_field_at tag values with
  | None => Some 0%N
  | Some (Core.TypedSourceScalar raw (Core.SourceU32 value)) =>
      match Schema.raw_wire raw, Schema.raw_payload raw,
          Schema.raw_program_type_id raw with
      | Schema.Varint, Schema.Scalar, None => Some value
      | _, _, _ => None
      end
  | _ => None
  end.

Definition source_ground_bigrat_score
    (values : list Core.typed_source_value)
    : option Core.source_score :=
  match source_ground_blob_or_absent 1 values,
      source_ground_blob_or_absent 2 values with
  | Some numerator, Some denominator =>
      Some (Core.source_score_constructor 16
        [Core.SourceBytes numerator; Core.SourceBytes denominator])
  | _, _ => None
  end.

Definition source_ground_fixed_point_score
    (values : list Core.typed_source_value)
    : option Core.source_score :=
  match source_ground_blob_or_absent 1 values,
      source_ground_u32_or_absent 2 values with
  | Some unscaled, Some scale =>
      Some (Core.source_score_constructor 17
        [Core.SourceBytes unscaled;
         Core.SourceNode (Core.SourceCons (Core.SourceInteger scale)
           Core.SourceNil)])
  | _, _ => None
  end.

Definition source_expr_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match children with
  | [] => Some (Core.source_score_constructor 0 [])
  | [child] =>
      match Core.source_summary_value child with
      | Core.TypedSourceScalar _ (Core.SourceBool value) =>
          if Core.source_summary_tag child =? 1
          then Some (Core.source_score_constructor 1
            [source_bool_word (negb value)]) else None
      | Core.TypedSourceScalar _ (Core.SourceI64 value) =>
          if Core.source_summary_tag child =? 2
          then Some (Core.source_score_constructor 2
            [Core.SourceInteger (source_signed_word value)]) else None
      | Core.TypedSourceScalar _ (Core.SourceU64 bits) =>
          if Core.source_summary_tag child =? 34
          then Some (Core.source_score_constructor 14
            [Core.SourceInteger bits]) else None
      | Core.TypedSourceScalar _ (Core.SourceText value) =>
          if Core.source_summary_tag child =? 3
          then Some (Core.source_score_constructor 3
            [Core.SourceString value])
          else if Core.source_summary_tag child =? 4
          then Some (Core.source_score_constructor 4
            [Core.SourceString value]) else None
      | Core.TypedSourceScalar _ (Core.SourceBlob value) =>
          if Core.source_summary_tag child =? 25
          then Some (Core.source_score_constructor 116
            [Core.SourceBytes value])
          else if Core.source_summary_tag child =? 35
          then Some (Core.source_score_constructor 15
            [Core.SourceBytes value]) else None
      | Core.TypedSourceMessage _ values =>
          match Core.source_summary_tag child with
          | 36 =>
              match Core.source_summary_type_id child with
              | Some 25 =>
                  if Core.source_summary_refused child then None
                  else source_ground_bigrat_score values
              | _ => None
              end
          | 37 =>
              match Core.source_summary_type_id child with
              | Some 26 =>
                  if Core.source_summary_refused child then None
                  else source_ground_fixed_point_score values
              | _ => None
              end
          | _ => Core.source_summary_exact_score child
          end
      | _ => None
      end
  | _ => None
  end.

Example source_expr_gdouble_zero_is_scored :
  Core.independently_decode_scalar Data.PBFixed64 false
    [0; 0; 0; 0; 0; 0; 0; 0] = Some (Core.SourceU64 0) /\
  source_expr_score
    [Core.SourceExactSummary
      (Core.TypedSourceScalar
        (Schema.RawField 34 Schema.Fixed64 Schema.Scalar
          [0; 0; 0; 0; 0; 0; 0; 0] 8 None [] [])
        (Core.SourceU64 0))
      (Core.SourceFamilyAnalysis None None false false false) None] =
    Some (Core.source_score_constructor 14 [Core.SourceInteger 0]).
Proof. vm_compute. split; reflexivity. Qed.

Example source_expr_gdouble_sign_bit_is_scored :
  Core.independently_decode_scalar Data.PBFixed64 false
    [0; 0; 0; 0; 0; 0; 0; 128] =
      Some (Core.SourceU64 9223372036854775808) /\
  source_expr_score
    [Core.SourceExactSummary
      (Core.TypedSourceScalar
        (Schema.RawField 34 Schema.Fixed64 Schema.Scalar
          [0; 0; 0; 0; 0; 0; 0; 128] 8 None [] [])
        (Core.SourceU64 9223372036854775808))
      (Core.SourceFamilyAnalysis None None false false false) None] =
    Some (Core.source_score_constructor 14
      [Core.SourceInteger 9223372036854775808]).
Proof. vm_compute. split; reflexivity. Qed.

Example source_expr_gbigint_empty_is_scored :
  Core.independently_decode_scalar Data.PBBytes false [] =
    Some (Core.SourceBlob []) /\
  source_expr_score
    [Core.SourceExactSummary
      (Core.TypedSourceScalar
        (Schema.RawField 35 Schema.LengthDelimited Schema.Scalar
          [] 0 None [] [])
        (Core.SourceBlob []))
      (Core.SourceFamilyAnalysis None None false false false) None] =
    Some (Core.source_score_constructor 15 [Core.SourceBytes []]).
Proof. vm_compute. split; reflexivity. Qed.

Example source_expr_gbigint_bytes_are_scored :
  source_expr_score
    [Core.SourceExactSummary
      (Core.TypedSourceScalar
        (Schema.RawField 35 Schema.LengthDelimited Schema.Scalar
          [1; 2; 3] 3 None [] [])
        (Core.SourceBlob [1; 2; 3]))
      (Core.SourceFamilyAnalysis None None false false false) None] =
    Some (Core.source_score_constructor 15
      [Core.SourceBytes [1; 2; 3]]).
Proof. vm_compute. reflexivity. Qed.

Example source_expr_wrong_ground_tag_remains_unscored :
  source_expr_score
    [Core.SourceExactSummary
      (Core.TypedSourceScalar
        (Schema.RawField 35 Schema.Fixed64 Schema.Scalar
          [1; 0; 0; 0; 0; 0; 0; 0] 8 None [] [])
        (Core.SourceU64 1))
      (Core.SourceFamilyAnalysis None None false false false) None] = None.
Proof. vm_compute. reflexivity. Qed.

Definition source_process_family_score (type_id : nat)
    (complete : option bool)
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match type_id, complete with
  | 16, _ => source_receive_bind_score children
  | 21, _ => source_match_case_score children
  | 8, Some true => source_bundle_score children
  | 15, Some true => source_send_score children
  | 19, Some true => source_receive_score children
  | 20, Some true => source_new_score children
  | 22, Some true => source_match_score children
  | 23, Some true => source_if_score children
  | 24, Some true => source_expr_score children
  | 24, Some false => Some (Core.source_score_constructor 2147483647 [])
  | 8, Some false | 15, Some false | 19, Some false
  | 20, Some false | 22, Some false | 23, Some false =>
      Some (Core.source_score_constructor 2147483647 [])
  | _, _ => None
  end.

Definition source_bind_source_present
    (state : Admission.source_admission_state) : bool :=
  Admission.source_seen_slot state 1.

Definition source_case_required_present
    (state : Admission.source_admission_state) : bool :=
  Admission.source_seen_slot state 0 && Admission.source_seen_slot state 1.

Definition source_all_bind_sources_present
    (children : list Core.source_exact_summary) : bool :=
  forallb (fun child =>
    if Core.source_summary_tag child =? 1
    then Core.source_family_bind_source_present
      (Core.source_summary_analysis child)
    else true) children.

Definition source_all_match_cases_required
    (children : list Core.source_exact_summary) : bool :=
  forallb (fun child =>
    if Core.source_summary_tag child =? 2
    then Core.source_family_case_required_present
      (Core.source_summary_analysis child)
    else true) children.

Definition source_process_completeness (type_id : nat)
    (state : Admission.source_admission_state)
    (children : list Core.source_exact_summary) : option bool :=
  match type_id with
  | 8 => Some (Admission.source_seen_slot state 0)
  | 15 => Some (Admission.source_seen_slot state 0)
  | 19 => Some (Admission.source_seen_slot state 1 &&
      source_all_bind_sources_present children)
  | 20 => Some (Admission.source_seen_slot state 1)
  | 22 => Some (Admission.source_seen_slot state 0 &&
      source_all_match_cases_required children)
  | 23 => Some (Admission.source_seen_slot state 0 &&
      Admission.source_seen_slot state 1 &&
      Admission.source_seen_slot state 2)
  | 24 =>
      match children with
      | [] => Some true
      | [child] =>
          match Core.source_summary_value child with
          | Core.TypedSourceScalar _ _ => Some true
          | _ => Core.source_family_complete
              (Core.source_summary_analysis child)
          end
      | _ => None
      end
  | 25 | 26 => Some true
  | _ => None
  end.

Fixpoint source_bytes_leq (left right : list nat) : bool :=
  match left, right with
  | [], _ => true
  | _ :: _, [] => false
  | first :: rest, second :: suffix =>
      if first <? second then true
      else if second <? first then false
      else source_bytes_leq rest suffix
  end.

Fixpoint source_byte_sequence_ordered (bodies : list (list nat)) : bool :=
  match bodies with
  | [] | [_] => true
  | first :: ((second :: _) as rest) =>
      source_bytes_leq first second && source_byte_sequence_ordered rest
  end.

Definition source_new_uri_ordered
    (children : list Core.source_exact_summary) : bool :=
  source_byte_sequence_ordered
    (map Core.source_summary_body
      (filter (fun child => Core.source_summary_tag child =? 3) children)).

Definition source_process_family_refused (type_id : nat)
    (complete : option bool)
    (state : Admission.source_admission_state)
    (children : list Core.source_exact_summary) : bool :=
  match type_id with
  | 19 =>
      match complete with
      | Some true => source_present_default_condition children
      | _ => false
      end
  | 20 => Admission.source_seen_slot state 1 &&
      negb (source_new_uri_ordered children)
  | 22 =>
      match complete with
      | Some true => source_present_default_match_guard children
      | _ => false
      end
  | _ => false
  end.

Lemma source_generated_par_component_tags :
  source_par_component_tags = [1; 2; 4; 5; 6; 7; 8; 11; 12; 13].
Proof. vm_compute. reflexivity. Qed.

Lemma source_complete_receive_default_condition_refused :
  forall state children,
    source_present_default_condition children = true ->
    source_process_family_refused 19 (Some true) state children = true /\
    source_process_family_score 19 (Some true) children = None.
Proof.
  intros state children present.
  split.
  - change (source_present_default_condition children = true).
    exact present.
  - change (source_receive_score children = None).
    unfold source_receive_score. now rewrite present.
Qed.

Lemma source_complete_match_default_guard_refused :
  forall state children,
    source_present_default_match_guard children = true ->
    source_process_family_refused 22 (Some true) state children = true /\
    source_process_family_score 22 (Some true) children = None.
Proof.
  intros state children present.
  split.
  - change (source_present_default_match_guard children = true).
    exact present.
  - change (source_match_score children = None).
    unfold source_match_score. now rewrite present.
Qed.

Lemma source_expr_absent_has_exact_score :
  source_process_family_score 24 (Some true) [] =
    Some (Core.source_score_constructor 0 []).
Proof. reflexivity. Qed.

Lemma source_incomplete_send_has_nonsemantic_score :
  forall children,
    source_process_family_score 15 (Some false) children =
      Some (Core.source_score_constructor 2147483647 []).
Proof. reflexivity. Qed.

Print Assumptions source_generated_par_component_tags.
Print Assumptions source_expr_gdouble_zero_is_scored.
Print Assumptions source_expr_gdouble_sign_bit_is_scored.
Print Assumptions source_expr_gbigint_empty_is_scored.
Print Assumptions source_expr_gbigint_bytes_are_scored.
Print Assumptions source_expr_wrong_ground_tag_remains_unscored.
Print Assumptions source_complete_receive_default_condition_refused.
Print Assumptions source_complete_match_default_guard_refused.
Print Assumptions source_expr_absent_has_exact_score.
Print Assumptions source_incomplete_send_has_nonsemantic_score.
