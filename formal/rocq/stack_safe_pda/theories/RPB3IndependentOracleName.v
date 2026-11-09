From Stdlib Require Import List Bool Arith NArith.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.

(** Family scoring consumes only the typed source values and source-owned
    scores.  The eventual recursive evaluator is responsible for producing
    these summaries from each generated field and auxiliary frame. *)
Definition source_child_at (tag : nat)
    (children : list Core.source_exact_summary)
    : option Core.source_exact_summary :=
  find (fun child => Core.source_summary_tag child =? tag) children.

Definition source_bool_or_absent (tag : nat)
    (children : list Core.source_exact_summary) : option bool :=
  match source_child_at tag children with
  | None => Some false
  | Some child =>
      match Core.source_summary_value child with
      | Core.TypedSourceScalar _ (Core.SourceBool value) => Some value
      | _ => None
      end
  end.

Definition source_pathmap_auxiliary_score (raw : Schema.raw_field)
    : Core.source_score :=
  Core.SourceBytes (Schema.raw_body raw).

Definition source_empty_par_score : Core.source_score :=
  Core.source_score_constructor 999 [Core.SourceInteger 0].

Definition source_string_map_auxiliary_score (key : list nat)
    (nested : option (bool * Core.source_exact_summary))
    : option Core.source_score :=
  let value_score :=
    match nested with
    | None => Some source_empty_par_score
    | Some (semantic_default, summary) =>
        if semantic_default || Core.source_summary_refused summary
        then None else Core.source_summary_exact_score summary
    end in
  option_map (fun score => Core.SourceNode
    (Core.source_score_forest_of_list
      [Core.SourceString key; score])) value_score.

Definition source_pathmap_values_accepted
    (values : list (bool * Core.source_exact_summary)) : bool :=
  forallb (fun entry =>
    let '(semantic_default, summary) := entry in
    negb semantic_default && negb (Core.source_summary_refused summary))
    values.

Definition source_name_completeness (type_id : nat)
    (state : Admission.source_admission_state) : option bool :=
  match type_id with
  | 31 => Some (Admission.source_seen_slot state 3)
  | 32 => Some (Admission.source_seen_slot state 0)
  | 55 | 60 => Some true
  | _ => None
  end.

Definition source_epathmap_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_child_at 9 children, source_bool_or_absent 4 children with
  | Some child, Some connective =>
      match Core.source_summary_exact_score child with
      | Some (Core.SourceBytes snapshot) =>
          Some (Core.source_score_constructor 13
            [Core.SourceBytes snapshot;
             Core.SourceInteger (if connective then 1 else 0)])
      | _ => None
      end
  | _, _ => None
  end.

Definition source_pathmap_snapshot_from_score (score : Core.source_score)
    : option (list nat) :=
  match score with
  | Core.SourceNode (Core.SourceCons (Core.SourceInteger constructor)
      (Core.SourceCons (Core.SourceBytes snapshot)
        (Core.SourceCons (Core.SourceInteger _) Core.SourceNil))) =>
      if N.eqb constructor 13 then Some snapshot else None
  | _ => None
  end.

Definition source_ezipper_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match source_child_at 1 children, source_bool_or_absent 5 children with
  | Some child, Some connective =>
      match Core.source_summary_exact_score child with
      | Some pathmap =>
          option_map (fun snapshot => Core.source_score_constructor 14
            [Core.SourceBytes snapshot;
             Core.SourceInteger (if connective then 1 else 0)])
            (source_pathmap_snapshot_from_score pathmap)
      | None => None
      end
  | _, _ => None
  end.

Definition source_connective_scalar_constructor (tag : nat) : option N :=
  match Schema.lookup_projected_program 55 [] [] with
  | None => None
  | Some program =>
      match Schema.resolve_tag (Schema.program_fields program) tag with
      | Some field =>
          match Schema.descriptor_payload field,
              Schema.descriptor_wire field,
              Schema.descriptor_oneof field with
          | Schema.Scalar, Schema.Varint, Some 0 =>
              if (5 <=? tag) && (tag <=? 9)
              then Some (N.of_nat (tag + 399)) else None
          | _, _, _ => None
          end
      | None => None
      end
  end.

Definition source_connective_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match children with
  | [] => Some (Core.SourceInteger 0)
  | [child] =>
      if Core.source_summary_refused child then None else
      match Core.source_summary_value child with
      | Core.TypedSourceScalar _ (Core.SourceBool value) =>
          option_map (fun constructor =>
            Core.source_score_constructor constructor
              [Core.SourceInteger (if value then 1 else 0)])
            (source_connective_scalar_constructor
              (Core.source_summary_tag child))
      | Core.TypedSourceMessage _ _ =>
          if (Core.source_summary_tag child =? 3) &&
              (match Core.source_summary_type_id child with
               | Some type_id => type_id =? 1
               | None => false
               end)
          then option_map (fun score =>
            Core.source_score_constructor 400 [score])
            (Core.source_summary_exact_score child)
          else None
      | _ => None
      end
  | _ => None
  end.

Definition source_unforgeable_constructor (tag : nat)
    : option (N * bool) :=
  match tag with
  | 1 => Some (5%N, true)
  | 2 => Some (11%N, true)
  | 3 => Some (10%N, true)
  | 4 => Some (12%N, false)
  | _ => None
  end.

Definition source_unforgeable_contract (tag : nat)
    : option (nat * N * bool) :=
  match Schema.lookup_projected_program 60 [] [],
      source_unforgeable_constructor tag with
  | Some program, Some (constructor, bytes_required) =>
      match Schema.resolve_tag (Schema.program_fields program) tag with
      | Some field =>
          match Schema.descriptor_wire field,
              Schema.descriptor_payload field,
              Schema.descriptor_child_type_id field with
          | Schema.LengthDelimited, Schema.Message, Some type_id =>
              Some (type_id, constructor, bytes_required)
          | _, _, _ => None
          end
      | None => None
      end
  | _, _ => None
  end.

Definition source_unforgeable_leaf_bytes
    (child : Core.source_exact_summary) : option (list nat) :=
  match Core.source_summary_value child with
  | Core.TypedSourceMessage raw [] =>
      if length (Schema.raw_body raw) =? 0 then Some [] else None
  | Core.TypedSourceMessage _
      [Core.TypedSourceScalar raw (Core.SourceBlob bytes)] =>
      if Schema.raw_tag raw =? 1 then Some bytes else None
  | _ => None
  end.

Definition source_unforgeable_score
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match children with
  | [] => Some (Core.source_score_constructor 0 [])
  | [child] =>
      if Core.source_summary_refused child then None else
      match source_unforgeable_contract (Core.source_summary_tag child) with
      | Some (type_id, constructor, bytes_required) =>
          if (match Core.source_summary_type_id child with
              | Some observed => observed =? type_id
              | None => false
              end) &&
              Schema.wire_eqb (Schema.raw_wire
                (Core.source_summary_raw child)) Schema.LengthDelimited &&
              Schema.payload_eqb (Schema.raw_payload
                (Core.source_summary_raw child)) Schema.Message
          then if bytes_required
            then option_map (fun bytes =>
              Core.source_score_constructor constructor
                [Core.SourceBytes bytes])
              (source_unforgeable_leaf_bytes child)
            else if length (Core.source_summary_body child) =? 0
              then Some (Core.source_score_constructor constructor [])
              else None
          else None
      | None => None
      end
  | _ => None
  end.

Definition source_name_family_score (type_id : nat)
    (complete : option bool)
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match type_id, complete with
  | 31, Some true => source_epathmap_score children
  | 32, Some true => source_ezipper_score children
  | 55, Some true => source_connective_score children
  | 60, Some true => source_unforgeable_score children
  | _, _ => None
  end.

Lemma source_pathmap_score_is_raw_snapshot :
  forall raw,
    source_pathmap_auxiliary_score raw =
      Core.SourceBytes (Schema.raw_body raw).
Proof. reflexivity. Qed.

Lemma source_string_map_rejects_default_present_par :
  forall key summary,
    source_string_map_auxiliary_score key (Some (true, summary)) = None.
Proof. reflexivity. Qed.

Lemma source_pathmap_values_reject_default_par :
  forall prefix summary suffix,
    source_pathmap_values_accepted
      (prefix ++ (true, summary) :: suffix) = false.
Proof.
  intros prefix summary suffix.
  unfold source_pathmap_values_accepted.
  rewrite forallb_app. simpl.
  now rewrite andb_false_r.
Qed.

Lemma source_pathmap_values_reject_refused_par :
  forall prefix summary suffix,
    Core.source_summary_refused summary = true ->
    source_pathmap_values_accepted
      (prefix ++ (false, summary) :: suffix) = false.
Proof.
  intros prefix summary suffix refused.
  unfold source_pathmap_values_accepted.
  rewrite forallb_app. simpl.
  rewrite refused. simpl.
  now rewrite andb_false_r.
Qed.

Lemma source_pathmap_score_roundtrip :
  forall snapshot flag,
    source_pathmap_snapshot_from_score
      (Core.source_score_constructor 13
        [Core.SourceBytes snapshot; Core.SourceInteger flag]) =
      Some snapshot.
Proof. intros. reflexivity. Qed.

Lemma source_name_family_absent_connective_score :
  source_name_family_score 55 (Some true) [] =
    Some (Core.SourceInteger 0).
Proof. reflexivity. Qed.

Lemma source_name_family_absent_unforgeable_score :
  source_name_family_score 60 (Some true) [] =
    Some (Core.source_score_constructor 0 []).
Proof. reflexivity. Qed.

Lemma source_generated_unforgeable_arms :
  map source_unforgeable_contract [1; 2; 3; 4] =
    [Some (61, 5%N, true); Some (62, 11%N, true);
     Some (63, 10%N, true); Some (64, 12%N, false)].
Proof. vm_compute. reflexivity. Qed.

Print Assumptions source_pathmap_score_is_raw_snapshot.
Print Assumptions source_string_map_rejects_default_present_par.
Print Assumptions source_pathmap_values_reject_default_par.
Print Assumptions source_pathmap_values_reject_refused_par.
Print Assumptions source_pathmap_score_roundtrip.
Print Assumptions source_name_family_absent_connective_score.
Print Assumptions source_name_family_absent_unforgeable_score.
Print Assumptions source_generated_unforgeable_arms.

Lemma source_generated_connective_scalar_constructors :
  map source_connective_scalar_constructor [5; 6; 7; 8; 9] =
    [Some 404%N; Some 405%N; Some 406%N;
     Some 407%N; Some 408%N].
Proof. vm_compute. reflexivity. Qed.

Print Assumptions source_generated_connective_scalar_constructors.
