(** Independent oracle rules for the expression payload families (leaf L02 of
    the score-arms design, sections 2 and 3).

    Each rule reads the typed source children and source-owned scores of the
    oracle; none reads a target [semantic_summary].  Citations: P is
    models/src/rust/rholang/protobuf_admission.rs, C is canonical_semantics.rs,
    SC is sorter/sort_combine.rs, all under models/src/rust/rholang/.

    Placement: after RPB3IndependentOracleProcess and before
    RPB3IndependentOracleRecursive.  Like RPB3IndependentOracleName, this
    theory depends only on Core and Admission, so it has no cycle with
    Recursive.  The [source_payload_*] helpers have the same shape as the
    Process helpers of the corresponding names, without importing Process.

    Scores never read locally_free (P:3125-3131, P:3190-3196, P:3317-3323). *)

From Stdlib Require Import List Bool Arith NArith ZArith.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission.

Import ListNotations.

(** ** Reading typed children *)

Definition source_payload_child_at (tag : nat)
    (children : list source_exact_summary)
    : option source_exact_summary :=
  find (fun child => source_summary_tag child =? tag) children.

Definition source_payload_typed_message_score (type_id : nat)
    (child : source_exact_summary) : option source_score :=
  match source_summary_value child, source_summary_type_id child with
  | TypedSourceMessage _ _, Some observed =>
      if (observed =? type_id) && negb (source_summary_refused child)
      then source_summary_exact_score child else None
  | _, _ => None
  end.

Definition source_payload_typed_child_score (tag type_id : nat)
    (children : list source_exact_summary) : option source_score :=
  match source_payload_child_at tag children with
  | Some child => source_payload_typed_message_score type_id child
  | None => None
  end.

Fixpoint source_payload_typed_selected_scores (tag type_id : nat)
    (children : list source_exact_summary) : option (list source_score) :=
  match children with
  | [] => Some []
  | child :: rest =>
      match source_payload_typed_selected_scores tag type_id rest with
      | None => None
      | Some scores =>
          if source_summary_tag child =? tag then
            option_map (fun score => score :: scores)
              (source_payload_typed_message_score type_id child)
          else Some scores
      end
  end.

Definition source_payload_typed_optional_score (tag type_id : nat)
    (children : list source_exact_summary)
    : option (option source_score) :=
  match source_payload_child_at tag children with
  | None => Some None
  | Some child =>
      option_map Some (source_payload_typed_message_score type_id child)
  end.

Definition source_payload_typed_optional_score_or (tag type_id : nat)
    (fallback : source_score) (children : list source_exact_summary)
    : option source_score :=
  match source_payload_typed_optional_score tag type_id children with
  | Some None => Some fallback
  | Some (Some score) => Some score
  | None => None
  end.

Definition source_payload_bool_or_absent (tag : nat)
    (children : list source_exact_summary) : option bool :=
  match source_payload_child_at tag children with
  | None => Some false
  | Some child =>
      if source_summary_refused child then None else
      match source_summary_value child with
      | TypedSourceScalar _ (SourceBool value) => Some value
      | _ => None
      end
  end.

Definition source_payload_i32_or_absent (tag : nat)
    (children : list source_exact_summary) : option Z :=
  match source_payload_child_at tag children with
  | None => Some 0%Z
  | Some child =>
      if source_summary_refused child then None else
      match source_summary_value child with
      | TypedSourceScalar _ (SourceI32 value) => Some value
      | _ => None
      end
  end.

Definition source_payload_signed_word (value : Z) : N :=
  Z.to_N (Z.modulo value 18446744073709551616).

Definition source_payload_bool_word (value : bool) : source_score :=
  SourceInteger (if value then 1 else 0).

(** The score of an absent collection remainder (P:3120). *)
Definition source_minus_one : source_score :=
  SourceInteger (source_payload_signed_word (-1)).

(** ** Var, type 6 (P:1202-1226, C:203-226)

    An absent oneof scores the integer 0; its arms are typed source values. *)
Definition source_var_level_score (constructor : N) (level : Z)
    : source_score :=
  source_score_constructor constructor
    [SourceInteger (source_payload_signed_word level)].

Definition source_var_score (children : list source_exact_summary)
    : option source_score :=
  match children with
  | [] => Some (SourceInteger 0)
  | [child] =>
      if source_summary_refused child then None else
      match source_summary_value child with
      | TypedSourceScalar _ (SourceI32 level) =>
          if source_summary_tag child =? 1
          then Some (source_var_level_score 50 level)
          else if source_summary_tag child =? 2
          then Some (source_var_level_score 51 level)
          else None
      | TypedSourceMessage _ _ =>
          if (source_summary_tag child =? 3) &&
              (match source_summary_type_id child with
               | Some type_id => type_id =? 7
               | None => false
               end)
          then Some (source_score_constructor 52 [])
          else None
      | _ => None
      end
  | _ => None
  end.

(** ** EVar 35 and the unary operators ENot 36, ENeg 37
    (P:1442-1489, C:770-776, C:997-1001, SC:986-994) *)
Definition source_evar_score (children : list source_exact_summary)
    : option source_score :=
  option_map (fun var_score => source_score_constructor 100 [var_score])
    (source_payload_typed_child_score 1 6 children).

Definition source_unary_payload_score (constructor : N)
    (children : list source_exact_summary) : option source_score :=
  option_map (fun operand => source_score_constructor constructor [operand])
    (source_payload_typed_child_score 1 1 children).

(** ** Binary operators 38..54 (P:1491-1532, C:778-785, C:1041-1046,
    SC:921-957, SC:998-1017) *)
Definition source_binary_payload_score (constructor : N)
    (children : list source_exact_summary) : option source_score :=
  match source_payload_typed_child_score 1 1 children,
      source_payload_typed_child_score 2 1 children with
  | Some first, Some second =>
      Some (source_score_constructor constructor [first; second])
  | _, _ => None
  end.

(** ** Ordered collections EList 27, ETuple 28, ESet 29 (P:1095, P:3085-3143,
    C:787-802, C:993-996, SC:1021-1063, SC:1502-1539) *)
Definition source_collection_remainder (has_remainder : bool)
    (children : list source_exact_summary)
    : option (list source_score) :=
  if has_remainder
  then option_map (fun remainder => [remainder])
    (source_payload_typed_optional_score_or 5 6 source_minus_one children)
  else Some [].

Definition source_ordered_collection_score (constructor : N)
    (has_remainder : bool) (children : list source_exact_summary)
    : option source_score :=
  match source_collection_remainder has_remainder children,
      source_payload_typed_selected_scores 1 1 children,
      source_payload_bool_or_absent 4 children with
  | Some remainder, Some elements, Some connective =>
      Some (source_score_constructor constructor
        (remainder ++ elements ++ [source_payload_bool_word connective]))
  | _, _, _ => None
  end.

(** ** Semantic identity of a Par (P:1060-1069, P:1586-1615, P:7538,
    canonical_par_identity.rs:1-7, 39-54).

    The identity of a Par is its body without the whole root locally_free
    field.  The target removes that field's byte span.  The oracle instead
    re-serializes the typed root children other than tag 9, each with a
    canonical key and length, so it shares no decoding of the identity span
    with the target.  Only direct children are inspected: a nested locally_free
    lies inside some child's raw body and stays in the identity. *)
Fixpoint source_varint_groups (fuel : nat) (value : N) : list nat :=
  match fuel with
  | 0 => []
  | S rest =>
      if N.ltb value 128
      then [N.to_nat value]
      else (N.to_nat (N.modulo value 128) + 128)
        :: source_varint_groups rest (N.div value 128)
  end.

(** Ten seven-bit groups cover every unsigned 64-bit value. *)
Definition source_canonical_varint (value : N) : list nat :=
  source_varint_groups 10 value.

Definition source_wire_code (wire : wire_kind) : nat :=
  match wire with
  | Varint => 0
  | Fixed64 => 1
  | LengthDelimited => 2
  | Fixed32 => 5
  end.

Definition source_encoded_field (raw : raw_field) : list nat :=
  source_canonical_varint
    (N.of_nat (raw_tag raw * 8 + source_wire_code (raw_wire raw))) ++
  match raw_wire raw with
  | LengthDelimited =>
      source_canonical_varint (N.of_nat (length (raw_body raw))) ++
        raw_body raw
  | _ => raw_body raw
  end.

Definition source_par_identity_bytes (value : typed_source_value)
    : list nat :=
  match value with
  | TypedSourceMessage _ children =>
      flat_map (fun child =>
        if raw_tag (source_value_raw child) =? 9 then []
        else source_encoded_field (source_value_raw child)) children
  | _ => []
  end.

(** ** KeyValuePair carrier, type 34 (P:1534-1541).

    The pair has no standalone score in Rust.  The carrier exposes the key
    score and the key's semantic identity to the EMap parent. *)
Definition source_kvp_carrier (children : list source_exact_summary)
    : option source_score :=
  match source_payload_child_at 1 children with
  | None => None
  | Some key =>
      option_map (fun key_score =>
        source_score_constructor 0
          [key_score;
           SourceBytes (source_par_identity_bytes
             (source_summary_value key))])
        (source_payload_typed_message_score 1 key)
  end.

Definition source_kvp_carrier_parts (child : source_exact_summary)
    : option (source_score * list nat) :=
  match source_payload_typed_message_score 34 child with
  | Some (SourceNode (SourceCons (SourceInteger constructor)
      (SourceCons key_score
        (SourceCons (SourceBytes identity_bytes) SourceNil)))) =>
      if N.eqb constructor 0 then Some (key_score, identity_bytes) else None
  | _ => None
  end.

(** ** EMap, type 30 (P:3145-3216, C:1047-1052, SC:1428-1493).

    Only key scores enter; values never do. *)
Fixpoint source_emap_key_scores (children : list source_exact_summary)
    : option (list source_score) :=
  match children with
  | [] => Some []
  | child :: rest =>
      match source_emap_key_scores rest with
      | None => None
      | Some keys =>
          if source_summary_tag child =? 1 then
            option_map (fun parts => fst parts :: keys)
              (source_kvp_carrier_parts child)
          else Some keys
      end
  end.

Definition source_emap_score (children : list source_exact_summary)
    : option source_score :=
  match source_payload_typed_optional_score_or 5 6 source_minus_one children,
      source_emap_key_scores children,
      source_payload_bool_or_absent 4 children with
  | Some remainder, Some keys, Some connective =>
      Some (source_score_constructor 9
        (remainder :: keys ++ [source_payload_bool_word connective]))
  | _, _, _ => None
  end.

(** ** EMethod, type 33 (P:3303-3341, C:815-828, C:1062-1066, SC:1214-1237) *)
Definition source_emethod_name_or_empty
    (children : list source_exact_summary) : option (list nat) :=
  match source_payload_child_at 1 children with
  | None => Some []
  | Some child =>
      if source_summary_refused child then None else
      match source_summary_value child with
      | TypedSourceScalar _ (SourceText bytes) => Some bytes
      | _ => None
      end
  end.

Definition source_emethod_score (children : list source_exact_summary)
    : option source_score :=
  match source_emethod_name_or_empty children,
      source_payload_typed_child_score 2 1 children,
      source_payload_typed_selected_scores 3 1 children,
      source_payload_bool_or_absent 6 children with
  | Some method, Some target, Some arguments, Some connective =>
      Some (source_score_constructor 115
        (SourceString method :: target ::
          (arguments ++ [source_payload_bool_word connective])))
  | _, _, _, _ => None
  end.

(** ** ConnectiveBody 57 and VarRef 56 carriers, and the Connective arms And,
    Or and VarRef (P:1150-1168, P:2956-2989, C:738-745) *)
Definition source_connective_body_carrier
    (children : list source_exact_summary) : option source_score :=
  option_map (source_score_constructor 0)
    (source_payload_typed_selected_scores 1 1 children).

Definition source_var_ref_carrier (children : list source_exact_summary)
    : option source_score :=
  match source_payload_i32_or_absent 1 children,
      source_payload_i32_or_absent 2 children with
  | Some index, Some depth =>
      Some (source_score_constructor 403
        [SourceInteger (source_payload_signed_word index);
         SourceInteger (source_payload_signed_word depth)])
  | _, _ => None
  end.

Definition source_connective_body_arm (constructor : N)
    (child : source_exact_summary) : option source_score :=
  match source_payload_typed_message_score 57 child with
  | Some (SourceNode (SourceCons (SourceInteger carrier) forest)) =>
      if N.eqb carrier 0
      then Some (SourceNode (SourceCons (SourceInteger constructor) forest))
      else None
  | _ => None
  end.

Definition source_connective_message_arm (child : source_exact_summary)
    : option source_score :=
  match source_summary_tag child with
  | 1 => source_connective_body_arm 401 child
  | 2 => source_connective_body_arm 402 child
  | 4 => source_payload_typed_message_score 56 child
  | _ => None
  end.

(** ** Completeness of the payload families (C:993-1066; EMap P:3163-3170) *)
Definition source_expr_payload_completeness (type_id : nat)
    (state : source_admission_state)
    (children : list source_exact_summary) : option bool :=
  match type_id with
  | 6 | 27 | 28 | 29 => Some true
  | 30 =>
      Some (forallb (fun child =>
        if source_summary_tag child =? 1
        then match source_family_complete (source_summary_analysis child) with
             | Some true => true
             | _ => false
             end
        else true) children)
  | 33 => Some (source_seen_slot state 1)
  | 34 => Some (source_seen_slot state 0 && source_seen_slot state 1)
  | 35 | 36 | 37 => Some (source_seen_slot state 0)
  | 38 | 39 | 40 | 41 | 42 | 43 | 44 | 45 | 46 | 47 | 48 | 49 | 50 | 51 | 52
  | 53 | 54 => Some (source_seen_slot state 0 && source_seen_slot state 1)
  | _ => None
  end.

(** ** Payload score dispatch *)
Definition source_expr_payload_score (type_id : nat)
    (children : list source_exact_summary) : option source_score :=
  match type_id with
  | 6 => source_var_score children
  | 27 => source_ordered_collection_score 6 true children
  | 28 => source_ordered_collection_score 7 false children
  | 29 => source_ordered_collection_score 8 true children
  | 30 => source_emap_score children
  | 33 => source_emethod_score children
  | 35 => source_evar_score children
  | 36 => source_unary_payload_score 112 children
  | 37 => source_unary_payload_score 101 children
  | 38 => source_binary_payload_score 102 children
  | 39 => source_binary_payload_score 103 children
  | 40 => source_binary_payload_score 122 children
  | 41 => source_binary_payload_score 104 children
  | 42 => source_binary_payload_score 105 children
  | 43 => source_binary_payload_score 106 children
  | 44 => source_binary_payload_score 107 children
  | 45 => source_binary_payload_score 108 children
  | 46 => source_binary_payload_score 109 children
  | 47 => source_binary_payload_score 110 children
  | 48 => source_binary_payload_score 111 children
  | 49 => source_binary_payload_score 113 children
  | 50 => source_binary_payload_score 114 children
  | 51 => source_binary_payload_score 118 children
  | 52 => source_binary_payload_score 119 children
  | 53 => source_binary_payload_score 120 children
  | 54 => source_binary_payload_score 121 children
  | _ => None
  end.

(** The single fall-through for [source_message_score], as the design's
    "one fall-through" requires.  It is the source counterpart of the four
    arms the target adds to [score_evidence_for]: the carriers 57 and 56
    regardless of completeness, the pair carrier 34 and the payload families
    only when complete. *)
Definition source_expr_payload_family_score (type_id : nat)
    (complete : option bool) (children : list source_exact_summary)
    : option source_score :=
  if type_id =? 57 then source_connective_body_carrier children
  else if type_id =? 56 then source_var_ref_carrier children
  else match complete with
       | Some true =>
           if type_id =? 34 then source_kvp_carrier children
           else source_expr_payload_score type_id children
       | _ => None
       end.
