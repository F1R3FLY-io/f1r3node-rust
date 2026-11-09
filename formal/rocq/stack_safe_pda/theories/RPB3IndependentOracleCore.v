From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import EPM1 RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module TargetScore := RPB3SemanticReducer.

Inductive source_score : Type :=
| SourceInteger (value : N)
| SourceString (bytes : list nat)
| SourceBytes (bytes : list nat)
| SourceNode (children : source_score_forest)
with source_score_forest : Type :=
| SourceNil
| SourceCons (first : source_score) (rest : source_score_forest).

Fixpoint source_score_forest_of_list (children : list source_score)
    : source_score_forest :=
  match children with
  | [] => SourceNil
  | first :: rest =>
      SourceCons first (source_score_forest_of_list rest)
  end.

Definition source_score_constructor (constructor : N)
    (children : list source_score) : source_score :=
  SourceNode (SourceCons (SourceInteger constructor)
    (source_score_forest_of_list children)).

Fixpoint project_score (score : source_score)
    : TargetScore.canonical_score :=
  match score with
  | SourceInteger value => TargetScore.ScoreInteger value
  | SourceString bytes => TargetScore.ScoreString bytes
  | SourceBytes bytes => TargetScore.ScoreBytes bytes
  | SourceNode children => TargetScore.ScoreNode (project_score_forest children)
  end
with project_score_forest (children : source_score_forest)
    : TargetScore.canonical_score_forest :=
  match children with
  | SourceNil => TargetScore.ScoreNil
  | SourceCons first rest =>
      TargetScore.ScoreCons (project_score first)
        (project_score_forest rest)
  end.

Lemma project_source_score_forest_of_list :
  forall children,
    project_score_forest (source_score_forest_of_list children) =
      TargetScore.score_forest_of_list (map project_score children).
Proof.
  induction children as [|first rest IH]; simpl;
    [reflexivity|].
  now rewrite IH.
Qed.

Lemma project_source_score_constructor :
  forall constructor children,
    project_score (source_score_constructor constructor children) =
      TargetScore.score_constructor constructor
        (map project_score children).
Proof.
  intros constructor children.
  unfold source_score_constructor, TargetScore.score_constructor.
  simpl.
  now rewrite project_source_score_forest_of_list.
Qed.

Inductive source_scalar_value : Type :=
| SourceBool (value : bool)
| SourceI32 (value : Z)
| SourceU32 (value : N)
| SourceI64 (value : Z)
| SourceU64 (value : N)
| SourceText (bytes : list nat)
| SourceBlob (bytes : list nat).

Inductive typed_source_value : Type :=
| TypedSourceScalar
    (raw : Schema.raw_field) (value : source_scalar_value)
| TypedSourceMessage
    (raw : Schema.raw_field) (children : list typed_source_value)
| TypedSourceStringMap
    (raw : Schema.raw_field) (key : list nat)
    (value : option (bool * typed_source_value))
| TypedSourcePathMap
    (raw : Schema.raw_field) (snapshot : EPM1.epm1_snapshot)
    (values : list (bool * typed_source_value)).

Definition source_value_raw (value : typed_source_value)
    : Schema.raw_field :=
  match value with
  | TypedSourceScalar raw _
  | TypedSourceMessage raw _
  | TypedSourceStringMap raw _ _
  | TypedSourcePathMap raw _ _ => raw
  end.

Record source_family_analysis := SourceFamilyAnalysis {
  source_family_complete : option bool;
  source_family_score : option source_score;
  source_family_refused : bool;
  source_family_bind_source_present : bool;
  source_family_case_required_present : bool
}.

Record source_exact_summary := SourceExactSummary {
  source_summary_value : typed_source_value;
  source_summary_analysis : source_family_analysis;
  source_summary_semantic_default : option bool
}.

Definition source_summary_raw (summary : source_exact_summary)
    : Schema.raw_field :=
  source_value_raw (source_summary_value summary).

Definition source_summary_tag (summary : source_exact_summary) : nat :=
  Schema.raw_tag (source_summary_raw summary).

Definition source_summary_body (summary : source_exact_summary)
    : list nat :=
  Schema.raw_body (source_summary_raw summary).

Definition source_summary_type_id (summary : source_exact_summary)
    : option nat :=
  Schema.raw_program_type_id (source_summary_raw summary).

Definition source_summary_exact_score (summary : source_exact_summary)
    : option source_score :=
  source_family_score (source_summary_analysis summary).

Definition source_summary_refused (summary : source_exact_summary)
    : bool :=
  source_family_refused (source_summary_analysis summary).

Definition bytes_in_range (bytes : list nat) : bool :=
  forallb (fun byte => byte <? 256) bytes.

Definition independently_decoded_u64 (body : list nat) : option N :=
  match EPM1.decode_varint body with
  | Some (value, []) =>
      let numeric := N.of_nat value in
      if N.leb numeric 18446744073709551615 then
        Some numeric
      else None
  | _ => None
  end.

Lemma independently_decoded_u64_has_canonical_full_body :
  forall body value,
    independently_decoded_u64 body = Some value ->
    exists decoded,
      EPM1.decode_varint body = Some (decoded, []) /\
      value = N.of_nat decoded /\
      (value <= 18446744073709551615)%N.
Proof.
  intros body value accepted.
  unfold independently_decoded_u64 in accepted.
  destruct (EPM1.decode_varint body) as [[decoded suffix]|]
    eqn:parsed; [|discriminate].
  destruct suffix as [|first rest]; [|discriminate].
  destruct (N.leb (N.of_nat decoded) 18446744073709551615)
    eqn:bound; [|discriminate].
  inversion accepted; subst value.
  exists decoded.
  repeat split; try assumption.
  now apply N.leb_le.
Qed.

Definition source_nondefault (forbidden : bool) (body : list nat)
    : bool :=
  negb forbidden || negb (Nat.eqb (length body) 0).

Definition source_numeric_nondefault
    (forbidden : bool) (value : N) : bool :=
  negb forbidden || negb (N.eqb value 0).

Fixpoint source_little_endian (body : list nat) : N :=
  match body with
  | [] => 0
  | first :: rest =>
      (N.of_nat first + 256 * source_little_endian rest)%N
  end.

Definition source_signed_32 (value : N) : Z :=
  if N.ltb value 2147483648 then Z.of_N value
  else (Z.of_N value - 4294967296)%Z.

Definition source_signed_64 (value : N) : Z :=
  if N.ltb value 9223372036854775808 then Z.of_N value
  else (Z.of_N value - 18446744073709551616)%Z.

Definition source_zigzag (value : N) : Z :=
  if N.even value then Z.of_N (value / 2)%N
  else (- Z.of_N (value / 2 + 1)%N)%Z.

Definition source_numeric_scalar
    (kind : Data.protobuf_kind) (forbidden : bool)
    (value : N) : option source_scalar_value :=
  if source_numeric_nondefault forbidden value then
    match kind with
    | Data.PBBool =>
        if N.leb value 1 then Some (SourceBool (N.eqb value 1))
        else None
    | Data.PBInt32 =>
        if N.leb value 2147483647 ||
            N.leb 18446744071562067968 value
        then Some (SourceI32 (source_signed_64 value))
        else None
    | Data.PBSint32 =>
        if N.leb value 4294967295
        then Some (SourceI32 (source_zigzag value))
        else None
    | Data.PBUint32 =>
        if N.leb value 4294967295 then Some (SourceU32 value)
        else None
    | Data.PBInt64 => Some (SourceI64 (source_signed_64 value))
    | Data.PBSint64 => Some (SourceI64 (source_zigzag value))
    | Data.PBUint64 => Some (SourceU64 value)
    | _ => None
    end
  else None.

Definition source_fixed_scalar
    (kind : Data.protobuf_kind) (forbidden : bool)
    (body : list nat) : option source_scalar_value :=
  if bytes_in_range body then
    let value := source_little_endian body in
    match kind with
    | Data.PBSfixed32 =>
        if (length body =? 4) &&
            (negb forbidden || negb (N.eqb value 0))
        then Some (SourceI32 (source_signed_32 value)) else None
    | Data.PBFixed32 =>
        if (length body =? 4) &&
            (negb forbidden || negb (N.eqb value 0))
        then Some (SourceU32 value) else None
    | Data.PBFloat =>
        if (length body =? 4) &&
            (negb forbidden ||
              negb (N.eqb value 0 || N.eqb value 2147483648))
        then Some (SourceU32 value) else None
    | Data.PBSfixed64 =>
        if (length body =? 8) &&
            (negb forbidden || negb (N.eqb value 0))
        then Some (SourceI64 (source_signed_64 value)) else None
    | Data.PBFixed64 =>
        if (length body =? 8) &&
            (negb forbidden || negb (N.eqb value 0))
        then Some (SourceU64 value) else None
    | Data.PBDouble =>
        if (length body =? 8) &&
            (negb forbidden ||
              negb (N.eqb value 0 ||
                N.eqb value 9223372036854775808))
        then Some (SourceU64 value) else None
    | _ => None
    end
  else None.

Definition independently_decode_scalar
    (kind : Data.protobuf_kind) (forbidden : bool)
    (body : list nat) : option source_scalar_value :=
  match kind with
  | Data.PBBool | Data.PBInt32 | Data.PBSint32 | Data.PBUint32
  | Data.PBInt64 | Data.PBSint64 | Data.PBUint64 =>
      match independently_decoded_u64 body with
      | Some value => source_numeric_scalar kind forbidden value
      | None => None
      end
  | Data.PBSfixed32 | Data.PBFixed32 | Data.PBFloat
  | Data.PBSfixed64 | Data.PBFixed64 | Data.PBDouble =>
      source_fixed_scalar kind forbidden body
  | Data.PBString | Data.PBRepeatedString =>
      if bytes_in_range body &&
          Source.indexed_scan_utf8_accepts body &&
          source_nondefault forbidden body
      then Some (SourceText body) else None
  | Data.PBBytes | Data.PBRepeatedBytes =>
      if bytes_in_range body && source_nondefault forbidden body
      then Some (SourceBlob body) else None
  | _ => None
  end.

Print Assumptions project_source_score_forest_of_list.
Print Assumptions project_source_score_constructor.
Print Assumptions independently_decoded_u64_has_canonical_full_body.
