From Stdlib Require Import List Bool Arith NArith ZArith.
From StackSafePDA Require Import StackSafePDA RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.

Definition cost_family (type_id : nat) : bool :=
  (9 <=? type_id) && (type_id <=? 13).

Definition cost_program (raw : Schema.raw_field)
    : option (nat * Schema.generated_program) :=
  match Schema.raw_payload raw, Schema.raw_program_type_id raw with
  | Schema.Message, Some type_id =>
      if cost_family type_id then
        option_map (fun program => (type_id, program))
          (Schema.lookup_projected_program type_id [] [])
      else None
  | _, _ => None
  end.

Definition source_scalar_leaf (program : Schema.generated_program)
    (raw : Schema.raw_field) : option Core.source_exact_summary :=
  option_map (fun value =>
    Core.SourceExactSummary value
      (Core.SourceFamilyAnalysis None None false false false) None)
    (Admission.source_typed_scalar program raw).

Fixpoint selected_source_scores (tag : nat)
    (children : list Core.source_exact_summary)
    : option (list Core.source_score) :=
  match children with
  | [] => Some []
  | child :: rest =>
      match selected_source_scores tag rest with
      | None => None
      | Some scores =>
          if Core.source_summary_tag child =? tag then
            option_map (fun score => score :: scores)
              (Core.source_summary_exact_score child)
          else Some scores
      end
  end.

Definition scalar_i32_or_zero (tag : nat)
    (children : list Core.source_exact_summary) : option Z :=
  match find (fun child => Core.source_summary_tag child =? tag)
      children with
  | None => Some 0%Z
  | Some child =>
      match Core.source_summary_value child with
      | Core.TypedSourceScalar _ (Core.SourceI32 value) => Some value
      | _ => None
      end
  end.

Definition scalar_u32_or_zero (tag : nat)
    (children : list Core.source_exact_summary) : option N :=
  match find (fun child => Core.source_summary_tag child =? tag)
      children with
  | None => Some 0%N
  | Some child =>
      match Core.source_summary_value child with
      | Core.TypedSourceScalar _ (Core.SourceU32 value) => Some value
      | _ => None
      end
  end.

Definition scalar_u64_or_zero (tag : nat)
    (children : list Core.source_exact_summary) : option N :=
  match find (fun child => Core.source_summary_tag child =? tag)
      children with
  | None => Some 0%N
  | Some child =>
      match Core.source_summary_value child with
      | Core.TypedSourceScalar _ (Core.SourceU64 value) => Some value
      | _ => None
      end
  end.

Definition optional_source_score (tag : nat)
    (children : list Core.source_exact_summary)
    : option (option Core.source_score) :=
  match find (fun child => Core.source_summary_tag child =? tag)
      children with
  | None => Some None
  | Some child => option_map Some (Core.source_summary_exact_score child)
  end.

Definition signed_source_word (value : Z) : N :=
  Z.to_N (Z.modulo value 18446744073709551616).

Definition source_u64_be_bytes (value : N) : list nat :=
  [N.to_nat (N.land (N.shiftr value 56) 255);
   N.to_nat (N.land (N.shiftr value 48) 255);
   N.to_nat (N.land (N.shiftr value 40) 255);
   N.to_nat (N.land (N.shiftr value 32) 255);
   N.to_nat (N.land (N.shiftr value 24) 255);
   N.to_nat (N.land (N.shiftr value 16) 255);
   N.to_nat (N.land (N.shiftr value 8) 255);
   N.to_nat (N.land value 255)].

Definition cost_atom_score (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match children with
  | [] => Some (Core.source_score_constructor 505 [])
  | [child] =>
      match Core.source_summary_tag child,
          Core.source_summary_value child with
      | 1, Core.TypedSourceScalar _ (Core.SourceBlob bytes) =>
          Some (Core.source_score_constructor 502 [Core.SourceBytes bytes])
      | 2, Core.TypedSourceScalar _ (Core.SourceBlob bytes) =>
          Some (Core.source_score_constructor 503 [Core.SourceBytes bytes])
      | 3, Core.TypedSourceMessage _ _ =>
          Core.source_summary_exact_score child
      | 4, Core.TypedSourceScalar _ (Core.SourceBlob bytes) =>
          Some (Core.source_score_constructor 507 [Core.SourceBytes bytes])
      | _, _ => None
      end
  | _ => None
  end.

Definition cost_bound_score (children : list Core.source_exact_summary)
    : option Core.source_score :=
  option_map (fun index => Core.source_score_constructor 504
    [Core.SourceInteger (signed_source_word index)])
    (scalar_i32_or_zero 1 children).

Definition cost_region_score (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match scalar_u32_or_zero 1 children,
      scalar_u64_or_zero 2 children,
      scalar_u64_or_zero 3 children,
      scalar_u64_or_zero 4 children,
      scalar_u64_or_zero 5 children,
      optional_source_score 6 children,
      scalar_u32_or_zero 7 children with
  | Some version, Some start_line, Some start_column,
      Some end_line, Some end_column, Some signer, Some ordinal =>
      Some (Core.source_score_constructor 500
        [Core.SourceInteger version;
         Core.SourceBytes (source_u64_be_bytes start_line);
         Core.SourceBytes (source_u64_be_bytes start_column);
         Core.SourceBytes (source_u64_be_bytes end_line);
         Core.SourceBytes (source_u64_be_bytes end_column);
         match signer with
         | Some score => score
         | None => Core.source_score_constructor 506 []
         end;
         Core.SourceInteger ordinal])
  | _, _, _, _, _, _, _ => None
  end.

Definition cost_family_score (type_id : nat)
    (children : list Core.source_exact_summary)
    : option Core.source_score :=
  match type_id with
  | 9 => option_map (Core.source_score_constructor 501)
      (selected_source_scores 1 children)
  | 10 => cost_atom_score children
  | 11 => cost_bound_score children
  | 12 => cost_region_score children
  | 13 => option_map (Core.source_score_constructor 508)
      (selected_source_scores 1 children)
  | _ => None
  end.

Definition source_cost_message (raw : Schema.raw_field)
    (type_id : nat) (program : Schema.generated_program)
    (children : list Core.source_exact_summary)
    : option Core.source_exact_summary :=
  let scan := Admission.source_scan_children program children in
  if Admission.source_raw_bytes_well_formed raw &&
      Admission.source_scan_valid scan &&
      Admission.source_required_pathmaps_present program scan &&
      (Schema.program_family program =? type_id)
  then option_map (fun score =>
    Core.SourceExactSummary
      (Core.TypedSourceMessage raw
        (map Core.source_summary_value children))
      (Core.SourceFamilyAnalysis
        (if type_id =? 13 then Some true else None)
        (Some score) false false false) None)
      (cost_family_score type_id children)
  else None.

Fixpoint source_cost_tree (parent : option Schema.generated_program)
    (subject : @tree Schema.raw_field) {struct subject}
    : option Core.source_exact_summary :=
  match subject with
  | Node raw children =>
      match Schema.raw_payload raw with
      | Schema.Scalar =>
          match parent, children with
          | Some program, FNil => source_scalar_leaf program raw
          | _, _ => None
          end
      | Schema.Message =>
          match cost_program raw with
          | Some (type_id, program) =>
              match source_cost_forest program children with
              | Some accepted =>
                  source_cost_message raw type_id program accepted
              | None => None
              end
          | None => None
          end
      | Schema.StringMap | Schema.PathMap => None
      end
  end
with source_cost_forest (program : Schema.generated_program)
    (subjects : @forest Schema.raw_field) {struct subjects}
    : option (list Core.source_exact_summary) :=
  match subjects with
  | FNil => Some []
  | FCons subject rest =>
      match source_cost_tree (Some program) subject,
          source_cost_forest program rest with
      | Some child, Some siblings => Some (child :: siblings)
      | _, _ => None
      end
  end.

Definition source_cost_root (subject : @tree Schema.raw_field)
    : option Core.source_exact_summary :=
  source_cost_tree None subject.

Lemma source_cost_absent_atom_score :
  cost_family_score 10 [] =
    Some (Core.source_score_constructor 505 []).
Proof. reflexivity. Qed.

Lemma source_cost_empty_signature_score :
  cost_family_score 9 [] =
    Some (Core.source_score_constructor 501 []).
Proof. reflexivity. Qed.

Lemma source_cost_empty_stack_score :
  cost_family_score 13 [] =
    Some (Core.source_score_constructor 508 []).
Proof. reflexivity. Qed.

Lemma source_cost_bound_retains_signed_index :
  forall children index,
    scalar_i32_or_zero 1 children = Some index ->
    cost_family_score 11 children =
      Some (Core.source_score_constructor 504
        [Core.SourceInteger (signed_source_word index)]).
Proof.
  intros children index decoded.
  change (cost_bound_score children =
    Some (Core.source_score_constructor 504
      [Core.SourceInteger (signed_source_word index)])).
  unfold cost_bound_score.
  now rewrite decoded.
Qed.

Lemma source_cost_signature_retains_atoms :
  forall children atoms,
    selected_source_scores 1 children = Some atoms ->
    cost_family_score 9 children =
      Some (Core.source_score_constructor 501 atoms).
Proof.
  intros children atoms collected.
  cbn [cost_family_score].
  now rewrite collected.
Qed.

Lemma source_cost_stack_retains_layers :
  forall children layers,
    selected_source_scores 1 children = Some layers ->
    cost_family_score 13 children =
      Some (Core.source_score_constructor 508 layers).
Proof.
  intros children layers collected.
  cbn [cost_family_score].
  now rewrite collected.
Qed.

Lemma source_cost_invalid_scan_refused :
  forall raw type_id program children,
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = false ->
    source_cost_message raw type_id program children = None.
Proof.
  intros raw type_id program children invalid.
  unfold source_cost_message.
  now rewrite invalid, andb_false_r.
Qed.

Lemma source_cost_invalid_body_refused :
  forall raw type_id program children,
    Admission.source_raw_bytes_well_formed raw = false ->
    source_cost_message raw type_id program children = None.
Proof.
  intros raw type_id program children invalid.
  unfold source_cost_message.
  now rewrite invalid.
Qed.

Lemma source_cost_accepted_has_exact_score :
  forall raw type_id program children summary,
    source_cost_message raw type_id program children = Some summary ->
    exists score,
      Core.source_summary_exact_score summary = Some score /\
      Core.source_summary_refused summary = false.
Proof.
  intros raw type_id program children summary accepted.
  unfold source_cost_message in accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Admission.source_scan_valid
        (Admission.source_scan_children program children) &&
      Admission.source_required_pathmaps_present program
        (Admission.source_scan_children program children) &&
      (Schema.program_family program =? type_id)) eqn:valid;
    [|discriminate].
  destruct (cost_family_score type_id children) as [score|]
    eqn:scored; [|discriminate].
  inversion accepted; subst summary.
  exists score. now split.
Qed.

Print Assumptions source_cost_absent_atom_score.
Print Assumptions source_cost_empty_signature_score.
Print Assumptions source_cost_empty_stack_score.
Print Assumptions source_cost_bound_retains_signed_index.
Print Assumptions source_cost_signature_retains_atoms.
Print Assumptions source_cost_stack_retains_layers.
Print Assumptions source_cost_invalid_scan_refused.
Print Assumptions source_cost_invalid_body_refused.
Print Assumptions source_cost_accepted_has_exact_score.
