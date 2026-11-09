(** * The Expr arm correspondence on the children the oracle actually produces

    [Target.exact_expr_score] reads the semantic summary projected from an
    independent-oracle child, and [Process.source_expr_score] reads the
    oracle's own typed value of the same child.  This theory compares the two
    readers on the children that the oracle's decoder and evaluator actually
    produce for the Expr family (24), the relation
    [Correspondence.process_actual_children].

    Every arm of the generated Expr message is proved, one lemma per arm
    group:

    - the ground scalars [GBool] (tag 1), the text arms [GString] and [GUri]
      (tags 3 and 4) and the byte arms [GByteArray] and [GBigInt] (tags 25
      and 35), whose typed value and raw body coincide;
    - the zigzag arm [GInt] (tag 2), whose typed value and score word agree
      through [ScalarBridge.full_u64_decoder_agreement];
    - the fixed64 bit-word arm [GDouble] (tag 34);
    - the nested-ground arms [GBigRat] (tag 36) and [GFixedPoint] (tag 37),
      whose parent decodes the child body again, handed off to
      [RPB3IndependentOracleExprNestedGroundScoreBridge];
    - every other message-carrying arm, which both readers read from the
      child's exact score, and an auxiliary node, which the Expr family does
      not declare and which both readers refuse.

    [expr_arm_enumeration] checks this arm list against the generated table. *)

From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import EPM1 RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3SourceScalarReduction RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleProcess
  RPB3IndependentOracleRecursive RPB3IndependentOracleProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleProcessScalarDecoderBridge
  RPB3IndependentOracleProcessChildCorrespondence
  RPB3IndependentOracleExprNestedGroundScoreBridge.

Import ListNotations.

(* [RPB3IndependentOracleExprNestedGroundScoreBridge] and
   [RPB3IndependentOracleProcessChildProvenance] are referenced by their full
   names below: binding them with a module alias costs well over a minute of
   elaboration on every compile, while the references are few. *)

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Target := RPB3SemanticReducer.
Module Scalar := RPB3SourceScalarReduction.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Process := RPB3IndependentOracleProcess.
Module Projection := RPB3IndependentOracleProjection.
Module Oracle := RPB3IndependentOracleRecursive.
Module ScalarBridge := RPB3IndependentOracleProcessScalarDecoderBridge.
Module Correspondence := RPB3IndependentOracleProcessChildCorrespondence.

(** ** The two readers on a scalar arm

    [target_expr_scalar_score] is the scalar branch of [Target.exact_expr_score]
    exposed as a function of the tag and body, and [source_expr_scalar_score]
    is the scalar branch of [Process.source_expr_score] exposed as a function
    of the tag and typed scalar. *)

Definition target_expr_scalar_score (tag : nat) (body : list nat)
    : option Target.canonical_score :=
  match tag with
  | 1 =>
      match body with
      | [0] => Some (Target.score_constructor 1 [Target.ScoreInteger 1])
      | [1] => Some (Target.score_constructor 1 [Target.ScoreInteger 0])
      | _ => None
      end
  | 2 =>
      option_map (fun encoded => Target.score_constructor 2
        [Target.ScoreInteger (Target.signed_score_word
          (Scalar.zigzag_signed encoded))])
        (Scalar.compact_varint_value body)
  | 3 => Some (Target.score_constructor 3 [Target.ScoreString body])
  | 4 => Some (Target.score_constructor 4 [Target.ScoreString body])
  | 25 => Some (Target.score_constructor 116 [Target.ScoreBytes body])
  | 34 =>
      match Scalar.scalar_semantic_from_source
          (Scalar.ScalarRule 34 Data.PBFixed64 false) body with
      | Some (Scalar.ScalarU64 bits) =>
          Some (Target.score_constructor 14 [Target.ScoreInteger bits])
      | _ => None
      end
  | 35 => Some (Target.score_constructor 15 [Target.ScoreBytes body])
  | _ => None
  end.

Definition source_expr_scalar_score (tag : nat)
    (scalar : Core.source_scalar_value) : option Core.source_score :=
  match scalar with
  | Core.SourceBool value =>
      if tag =? 1
      then Some (Core.source_score_constructor 1
        [Process.source_bool_word (negb value)]) else None
  | Core.SourceI64 value =>
      if tag =? 2
      then Some (Core.source_score_constructor 2
        [Core.SourceInteger (Process.source_signed_word value)]) else None
  | Core.SourceU64 bits =>
      if tag =? 34
      then Some (Core.source_score_constructor 14
        [Core.SourceInteger bits]) else None
  | Core.SourceText value =>
      if tag =? 3
      then Some (Core.source_score_constructor 3
        [Core.SourceString value])
      else if tag =? 4
      then Some (Core.source_score_constructor 4
        [Core.SourceString value]) else None
  | Core.SourceBlob value =>
      if tag =? 25
      then Some (Core.source_score_constructor 116
        [Core.SourceBytes value])
      else if tag =? 35
      then Some (Core.source_score_constructor 15
        [Core.SourceBytes value]) else None
  | _ => None
  end.

Lemma target_expr_score_of_scalar_summary :
  forall summary,
    Target.sem_payload summary = Schema.Scalar ->
    Target.exact_expr_score [summary] =
      target_expr_scalar_score (Target.sem_tag summary)
        (Target.sem_body summary).
Proof.
  intros summary payload.
  unfold Target.exact_expr_score, target_expr_scalar_score.
  rewrite payload.
  reflexivity.
Qed.

Lemma source_expr_score_of_scalar_summary :
  forall child raw scalar,
    Core.source_summary_value child = Core.TypedSourceScalar raw scalar ->
    Process.source_expr_score [child] =
      source_expr_scalar_score (Schema.raw_tag raw) scalar.
Proof.
  intros child raw scalar typed.
  unfold Process.source_expr_score, source_expr_scalar_score.
  change (Core.source_summary_tag child)
    with (Schema.raw_tag (Core.source_value_raw
      (Core.source_summary_value child))).
  rewrite typed.
  cbn [Core.source_value_raw].
  destruct scalar; reflexivity.
Qed.

Lemma target_expr_score_of_scalar_child :
  forall child raw scalar,
    Core.source_summary_value child = Core.TypedSourceScalar raw scalar ->
    Schema.raw_payload raw = Schema.Scalar ->
    Target.exact_expr_score [Projection.project_source_summary child] =
      target_expr_scalar_score (Schema.raw_tag raw) (Schema.raw_body raw).
Proof.
  intros child raw scalar typed payload.
  rewrite target_expr_score_of_scalar_summary.
  - change (Target.sem_tag (Projection.project_source_summary child))
      with (Schema.raw_tag (Core.source_summary_raw child)).
    change (Target.sem_body (Projection.project_source_summary child))
      with (Schema.raw_body (Core.source_summary_raw child)).
    unfold Core.source_summary_raw.
    rewrite typed.
    reflexivity.
  - change (Target.sem_payload (Projection.project_source_summary child))
      with (Schema.raw_payload (Core.source_summary_raw child)).
    unfold Core.source_summary_raw.
    rewrite typed.
    exact payload.
Qed.

(** ** Arm group: the boolean and text and byte ground values *)

(** A canonical varint whose value is below 128 is one byte long. *)
Lemma decoded_small_varint_is_singleton :
  forall body value,
    EPM1.decode_varint body = Some (value, []) ->
    value < 128 ->
    body = [value].
Proof.
  intros [|head tail] value parsed small.
  - rewrite EPM1.decode_varint_nil in parsed. discriminate parsed.
  - rewrite EPM1.decode_varint_cons in parsed.
    destruct (head <? 128) eqn:head_small.
    + inversion parsed; subst; reflexivity.
    + destruct (head <? 256) eqn:physical; [|discriminate parsed].
      destruct (EPM1.decode_varint tail) as [[quotient suffix]|] eqn:rest;
        [|discriminate parsed].
      destruct (quotient =? 0) eqn:nonzero; [discriminate parsed|].
      apply Nat.eqb_neq in nonzero.
      assert (reconstructed : value = head - 128 + 128 * quotient)
        by now inversion parsed.
      exfalso.
      lia.
Qed.

(** The default-allowed bool arm decodes exactly the one-byte bodies [[0]] and
    [[1]]: [Core.independently_decode_scalar] checks the decoded value and the
    canonical varint has no other spelling of zero or one. *)
Lemma decoded_gbool_value_and_body :
  forall body scalar,
    Core.independently_decode_scalar Data.PBBool false body = Some scalar ->
    (scalar = Core.SourceBool false /\ body = [0]) \/
    (scalar = Core.SourceBool true /\ body = [1]).
Proof.
  intros body scalar decoded.
  unfold Core.independently_decode_scalar, Core.source_numeric_scalar,
    Core.source_numeric_nondefault in decoded.
  destruct (Core.independently_decoded_u64 body) as [value|] eqn:parsed;
    [|discriminate decoded].
  cbn [negb orb] in decoded.
  destruct (N.leb value 1) eqn:small; [|discriminate decoded].
  injection decoded as same_scalar.
  destruct (Core.independently_decoded_u64_has_canonical_full_body
    body value parsed) as [word [varint [same_value bounded]]].
  apply N.leb_le in small.
  assert (word_small : word <= 1) by lia.
  pose proof (decoded_small_varint_is_singleton body word varint
    ltac:(lia)) as same_body.
  subst value.
  destruct word as [|[|larger]].
  - left. split; [rewrite <- same_scalar; reflexivity|exact same_body].
  - right. split; [rewrite <- same_scalar; reflexivity|exact same_body].
  - exfalso. lia.
Qed.

Lemma expr_gbool_arm_scores_agree :
  forall body scalar,
    Core.independently_decode_scalar Data.PBBool false body = Some scalar ->
    target_expr_scalar_score 1 body =
      option_map Core.project_score (source_expr_scalar_score 1 scalar).
Proof.
  intros body scalar decoded.
  destruct (decoded_gbool_value_and_body body scalar decoded)
    as [[same_scalar same_body]|[same_scalar same_body]];
    subst scalar body; reflexivity.
Qed.

(** Eliminate the guards of a decoder equation; the failing branch of every
    guard is the decoder's [None]. *)
Ltac reduce_decoder_guards decoded :=
  cbv beta iota zeta in decoded;
  lazymatch type of decoded with
  | (if ?condition then _ else _) = _ =>
      destruct condition; [reduce_decoder_guards decoded | discriminate decoded]
  | _ => idtac
  end.

Lemma decoded_gtext_is_body :
  forall body scalar,
    Core.independently_decode_scalar Data.PBString false body = Some scalar ->
    scalar = Core.SourceText body.
Proof.
  intros body scalar decoded.
  cbv beta iota zeta delta [Core.independently_decode_scalar] in decoded.
  reduce_decoder_guards decoded.
  injection decoded as same_scalar.
  exact (eq_sym same_scalar).
Qed.

Lemma decoded_gbytes_is_body :
  forall body scalar,
    Core.independently_decode_scalar Data.PBBytes false body = Some scalar ->
    scalar = Core.SourceBlob body.
Proof.
  intros body scalar decoded.
  cbv beta iota zeta delta [Core.independently_decode_scalar] in decoded.
  reduce_decoder_guards decoded.
  injection decoded as same_scalar.
  exact (eq_sym same_scalar).
Qed.

Lemma expr_text_arm_scores_agree :
  forall tag body scalar,
    tag = 3 \/ tag = 4 ->
    Core.independently_decode_scalar Data.PBString false body = Some scalar ->
    target_expr_scalar_score tag body =
      option_map Core.project_score (source_expr_scalar_score tag scalar).
Proof.
  intros tag body scalar which decoded.
  pose proof (decoded_gtext_is_body body scalar decoded) as same_scalar.
  subst scalar.
  destruct which as [same_tag|same_tag]; subst tag; reflexivity.
Qed.

Lemma expr_bytes_arm_scores_agree :
  forall tag body scalar,
    tag = 25 \/ tag = 35 ->
    Core.independently_decode_scalar Data.PBBytes false body = Some scalar ->
    target_expr_scalar_score tag body =
      option_map Core.project_score (source_expr_scalar_score tag scalar).
Proof.
  intros tag body scalar which decoded.
  pose proof (decoded_gbytes_is_body body scalar decoded) as same_scalar.
  subst scalar.
  destruct which as [same_tag|same_tag]; subst tag; reflexivity.
Qed.

(** ** Arm group: the zigzag word

    The generated [GInt] arm is a default-allowed [sint64]: the oracle reads
    the canonical unsigned varint and applies its own zigzag decoder, and the
    target applies [Scalar.zigzag_signed] to the canonical varint read by the
    same decoder ([ScalarBridge.full_u64_decoder_agreement]).  The two zigzag
    readers and the two score-word readers are the same function. *)

Lemma decoded_gint_is_zigzag_value :
  forall body scalar,
    Core.independently_decode_scalar Data.PBSint64 false body = Some scalar ->
    exists value,
      Core.independently_decoded_u64 body = Some value /\
      scalar = Core.SourceI64 (Core.source_zigzag value).
Proof.
  intros body scalar decoded.
  unfold Core.independently_decode_scalar, Core.source_numeric_scalar,
    Core.source_numeric_nondefault in decoded.
  destruct (Core.independently_decoded_u64 body) as [value|] eqn:parsed;
    [|discriminate decoded].
  cbn [negb orb] in decoded.
  injection decoded as same_scalar.
  exists value.
  split; [reflexivity|exact (eq_sym same_scalar)].
Qed.

Lemma expr_gint_arm_scores_agree :
  forall body scalar,
    Core.independently_decode_scalar Data.PBSint64 false body = Some scalar ->
    target_expr_scalar_score 2 body =
      option_map Core.project_score (source_expr_scalar_score 2 scalar).
Proof.
  intros body scalar decoded.
  destruct (decoded_gint_is_zigzag_value body scalar decoded)
    as [value [parsed same_scalar]].
  subst scalar.
  unfold target_expr_scalar_score, source_expr_scalar_score.
  cbv beta iota zeta.
  rewrite <- ScalarBridge.full_u64_decoder_agreement, parsed.
  reflexivity.
Qed.

(** ** Arm group: the fixed64 bit word

    The generated [GDouble] arm is a default-allowed [fixed64].  The two
    readers compute the same little-endian bit word: [Core.source_little_endian]
    and [Scalar.little_endian_value] are the same fixpoint, so the concluding
    [reflexivity] of [expr_gdouble_arm_scores_agree] identifies them. *)

Lemma decoded_gdouble_is_little_endian :
  forall body scalar,
    Core.independently_decode_scalar Data.PBFixed64 false body = Some scalar ->
    scalar = Core.SourceU64 (Core.source_little_endian body) /\
    length body = 8.
Proof.
  intros body scalar decoded.
  cbv beta iota zeta delta
    [Core.independently_decode_scalar Core.source_fixed_scalar] in decoded.
  destruct (Core.bytes_in_range body); [|discriminate decoded].
  destruct (length body =? 8) eqn:eight.
  - cbn [andb orb negb] in decoded.
    injection decoded as same_scalar.
    split; [exact (eq_sym same_scalar)|].
    apply Nat.eqb_eq.
    exact eight.
  - cbn [andb orb negb] in decoded.
    discriminate decoded.
Qed.

Lemma target_gdouble_semantic_value :
  forall body,
    length body = 8 ->
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule 34 Data.PBFixed64 false) body =
      Some (Scalar.ScalarU64 (Scalar.little_endian_value body)).
Proof.
  intros body eight.
  unfold Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid,
    Scalar.fixed_scalar_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  rewrite eight.
  reflexivity.
Qed.

Lemma expr_gdouble_arm_scores_agree :
  forall body scalar,
    Core.independently_decode_scalar Data.PBFixed64 false body = Some scalar ->
    target_expr_scalar_score 34 body =
      option_map Core.project_score (source_expr_scalar_score 34 scalar).
Proof.
  intros body scalar decoded.
  destruct (decoded_gdouble_is_little_endian body scalar decoded)
    as [same_scalar eight].
  subst scalar.
  unfold target_expr_scalar_score, source_expr_scalar_score.
  cbv beta iota zeta.
  rewrite (target_gdouble_semantic_value body eight).
  reflexivity.
Qed.

(** ** The generated arms of the Expr family

    The generated Expr program has seven scalar arms (each default-allowed,
    with the kinds named by [expr_contract_row]), twenty-nine message-carrying
    arms among which are [GBigRat] and [GFixedPoint], and no map field.  The
    enumeration below is a closed check over the generated table; the table
    entries are the rows that the scalar arm lemmas above discharge. *)

Definition expr_field_tags_with_payload (payload : Schema.payload_kind)
    : list nat :=
  match Schema.lookup_projected_program 24 [] [] with
  | Some program =>
      map Schema.descriptor_tag
        (filter (fun field =>
          Schema.payload_eqb (Schema.descriptor_payload field) payload)
          (Schema.program_fields program))
  | None => []
  end.

Example expr_arm_enumeration :
  expr_field_tags_with_payload Schema.Scalar = [1; 2; 3; 4; 25; 34; 35] /\
  expr_field_tags_with_payload Schema.Message =
    [5; 6; 7; 8; 9; 10; 11; 12; 13; 14; 15; 16; 17; 18; 19; 20; 21; 22; 23;
     24; 32; 33; 27; 28; 29; 30; 31; 36; 37] /\
  expr_field_tags_with_payload Schema.StringMap = [] /\
  expr_field_tags_with_payload Schema.PathMap = [].
Proof. vm_compute. repeat split; reflexivity. Qed.

Definition expr_contract_row (tag : nat)
    (contract : Admission.source_scalar_contract) : bool :=
  negb (Admission.source_contract_default_forbidden contract) &&
  match Admission.source_contract_kind contract with
  | Data.PBBool => tag =? 1
  | Data.PBSint64 => tag =? 2
  | Data.PBString => (tag =? 3) || (tag =? 4)
  | Data.PBBytes => (tag =? 25) || (tag =? 35)
  | Data.PBFixed64 => tag =? 34
  | _ => false
  end.

Definition expr_fields_contract_checked : bool :=
  match Schema.lookup_projected_program 24 [] [] with
  | Some program =>
      forallb (fun field =>
        match Admission.source_scalar_contract_for 24
            (Schema.descriptor_tag field) with
        | Some contract =>
            expr_contract_row (Schema.descriptor_tag field) contract
        | None => true
        end) (Schema.program_fields program)
  | None => false
  end.

Lemma expr_fields_contract_checked_true : expr_fields_contract_checked = true.
Proof. vm_compute. reflexivity. Qed.

Lemma expr_field_contract_row_holds :
  forall program field contract,
    Schema.lookup_projected_program 24 [] [] = Some program ->
    In field (Schema.program_fields program) ->
    Admission.source_scalar_contract_for 24 (Schema.descriptor_tag field) =
      Some contract ->
    expr_contract_row (Schema.descriptor_tag field) contract = true.
Proof.
  intros program field contract looked_up member found.
  pose proof expr_fields_contract_checked_true as checked.
  unfold expr_fields_contract_checked in checked.
  rewrite looked_up in checked.
  rewrite forallb_forall in checked.
  specialize (checked field member).
  cbv beta in checked.
  rewrite found in checked.
  exact checked.
Qed.

Lemma resolved_generated_field_has_requested_tag :
  forall fields tag field,
    Schema.resolve_tag fields tag = Some field ->
    Schema.descriptor_tag field = tag.
Proof.
  induction fields as [|head rest IH]; intros tag field resolved;
    cbn [Schema.resolve_tag] in resolved; [discriminate resolved|].
  destruct (Schema.descriptor_tag head =? tag) eqn:same.
  - injection resolved as same_field. subst field.
    exact (proj1 (Nat.eqb_eq _ _) same).
  - exact (IH tag field resolved).
Qed.

(** The admission gate of an accepted scalar also exposes the descriptor the
    tag resolved to, so the tag is one of the generated field tags. *)
Lemma expr_scalar_value_gate :
  forall program raw scalar,
    Admission.source_scalar_value_of_raw program raw = Some scalar ->
    exists descriptor contract,
      Schema.resolve_tag (Schema.program_fields program)
        (Schema.raw_tag raw) = Some descriptor /\
      Admission.source_scalar_contract_for (Schema.program_family program)
        (Schema.raw_tag raw) = Some contract /\
      Core.independently_decode_scalar (Admission.source_contract_kind contract)
        (Admission.source_contract_default_forbidden contract)
        (Schema.raw_body raw) = Some scalar.
Proof.
  intros program raw scalar accepted.
  unfold Admission.source_scalar_value_of_raw in accepted.
  destruct (Schema.raw_payload raw); try discriminate accepted.
  destruct (Schema.raw_program_type_id raw); try discriminate accepted.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Schema.raw_tag raw)) as [descriptor|] eqn:resolved;
    try discriminate accepted.
  destruct (Admission.source_scalar_contract_for (Schema.program_family program)
      (Schema.raw_tag raw)) as [contract|] eqn:contract_found;
    try discriminate accepted.
  destruct (Admission.source_scalar_wire
      (Admission.source_contract_kind contract)) as [wire|];
    try discriminate accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Schema.wire_eqb (Schema.raw_wire raw) wire &&
      Schema.wire_eqb wire (Schema.descriptor_wire descriptor) &&
      Schema.payload_eqb (Schema.descriptor_payload descriptor) Schema.Scalar)
    eqn:guarded; [|discriminate accepted].
  exists descriptor, contract.
  repeat split; try exact resolved; try exact contract_found; try exact accepted.
Qed.

Lemma expr_scalar_arm_dispatch :
  forall tag contract body scalar,
    expr_contract_row tag contract = true ->
    Core.independently_decode_scalar (Admission.source_contract_kind contract)
      (Admission.source_contract_default_forbidden contract) body =
      Some scalar ->
    target_expr_scalar_score tag body =
      option_map Core.project_score (source_expr_scalar_score tag scalar).
Proof.
  intros tag [kind forbidden] body scalar row decoded.
  unfold expr_contract_row in row.
  cbn [Admission.source_contract_kind
    Admission.source_contract_default_forbidden] in row, decoded.
  apply andb_true_iff in row as [not_forbidden row].
  destruct forbidden; [discriminate not_forbidden|].
  destruct kind; try discriminate row.
  - apply Nat.eqb_eq in row. subst tag.
    exact (expr_gbool_arm_scores_agree body scalar decoded).
  - apply Nat.eqb_eq in row. subst tag.
    exact (expr_gint_arm_scores_agree body scalar decoded).
  - apply Nat.eqb_eq in row. subst tag.
    exact (expr_gdouble_arm_scores_agree body scalar decoded).
  - apply orb_true_iff in row.
    destruct row as [row|row]; apply Nat.eqb_eq in row; subst tag.
    + exact (expr_text_arm_scores_agree 3 body scalar (or_introl eq_refl)
        decoded).
    + exact (expr_text_arm_scores_agree 4 body scalar (or_intror eq_refl)
        decoded).
  - apply orb_true_iff in row.
    destruct row as [row|row]; apply Nat.eqb_eq in row; subst tag.
    + exact (expr_bytes_arm_scores_agree 25 body scalar (or_introl eq_refl)
        decoded).
    + exact (expr_bytes_arm_scores_agree 35 body scalar (or_intror eq_refl)
        decoded).
Qed.

(** Every scalar child of an Expr message is read at a generated Expr arm by
    its gated decoder, and both readers agree on it. *)
Lemma expr_actual_scalar_child_corresponds :
  forall program child raw scalar,
    Schema.lookup_projected_program 24 [] [] = Some program ->
    Core.source_summary_value child = Core.TypedSourceScalar raw scalar ->
    Schema.raw_payload raw = Schema.Scalar ->
    Admission.source_scalar_value_of_raw program raw = Some scalar ->
    Target.exact_expr_score [Projection.project_source_summary child] =
    option_map Core.project_score (Process.source_expr_score [child]).
Proof.
  intros program child raw scalar looked_up typed payload accepted.
  destruct (expr_scalar_value_gate program raw scalar accepted)
    as [descriptor [contract [resolved [contract_found decoded]]]].
  pose proof (ScalarBridge.generated_program_has_requested_family 24 program
    looked_up) as family.
  rewrite family in contract_found.
  pose proof
    (RPB3IndependentOracleProcessChildProvenance.resolved_generated_field_is_member
      _ _ _ resolved)
    as member.
  pose proof (resolved_generated_field_has_requested_tag _ _ _ resolved)
    as same_tag.
  rewrite <- same_tag in contract_found.
  pose proof (expr_field_contract_row_holds program descriptor contract
    looked_up member contract_found) as row.
  rewrite same_tag in row.
  rewrite (target_expr_score_of_scalar_child child raw scalar typed payload).
  rewrite (source_expr_score_of_scalar_summary child raw scalar typed).
  exact (expr_scalar_arm_dispatch _ _ _ _ row decoded).
Qed.

(** ** Arm group: the nested-ground arms

    [GBigRat] (tag 36) and [GFixedPoint] (tag 37) carry a message child whose
    Expr parent decodes the child body again; the handoff is already proved.
    A message node needs positive fuel, so fuel zero is impossible here. *)

Lemma expr_nested_ground_arm_corresponds :
  forall fuel hash_fn body program span field nested child,
    Schema.lookup_projected_program 24 [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    (Schema.descriptor_tag field = 36 \/ Schema.descriptor_tag field = 37) ->
    Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body program (Source.SourceMessage span field nested) =
      Some child ->
    Target.exact_expr_score [Projection.project_source_summary child] =
    option_map Core.project_score (Process.source_expr_score [child]).
Proof.
  intros [|fuel] hash_fn body program span field nested child looked_up selected
    which evaluated.
  - unfold Oracle.source_evaluate_node in evaluated.
    destruct (Source.raw_field_of_source body
      (Source.SourceMessage span field nested)) as [raw|];
      discriminate evaluated.
  - destruct which as [is_bigrat|is_fixed].
    + exact (RPB3IndependentOracleExprNestedGroundScoreBridge.actual_generated_expr_bigrat_parent_score_projects
        fuel hash_fn body program span field nested child looked_up selected
        is_bigrat evaluated).
    + exact (RPB3IndependentOracleExprNestedGroundScoreBridge.actual_generated_expr_fixed_point_parent_score_projects
        fuel hash_fn body program span field nested child looked_up selected
        is_fixed evaluated).
Qed.

(** ** Arm group: the other message-carrying arms

    Both readers read the exact score of the child.  Each reader selects on
    tags 36 and 37 only, so the remaining tags reduce after peeling the
    numeric pattern. *)

Lemma target_expr_score_of_other_message_summary :
  forall summary,
    Target.sem_payload summary = Schema.Message ->
    Target.sem_tag summary <> 36 ->
    Target.sem_tag summary <> 37 ->
    Target.exact_expr_score [summary] =
      match Target.sem_score_evidence summary with
      | Target.ExactScore score => Some score
      | Target.ScoreNotDerived => None
      end.
Proof.
  intros summary payload not_bigrat not_fixed.
  unfold Target.exact_expr_score.
  rewrite payload.
  revert not_bigrat not_fixed.
  generalize (Target.sem_tag summary) as tag.
  intros tag not_bigrat not_fixed.
  do 36 (destruct tag as [|tag]; [reflexivity|]).
  destruct tag as [|[|tag]].
  - exact (False_ind _ (not_bigrat eq_refl)).
  - exact (False_ind _ (not_fixed eq_refl)).
  - reflexivity.
Qed.

Lemma source_expr_score_of_other_message_child :
  forall child raw values,
    Core.source_summary_value child = Core.TypedSourceMessage raw values ->
    Core.source_summary_tag child <> 36 ->
    Core.source_summary_tag child <> 37 ->
    Process.source_expr_score [child] = Core.source_summary_exact_score child.
Proof.
  intros child raw values typed not_bigrat not_fixed.
  unfold Process.source_expr_score.
  rewrite typed.
  revert not_bigrat not_fixed.
  generalize (Core.source_summary_tag child) as tag.
  intros tag not_bigrat not_fixed.
  do 36 (destruct tag as [|tag]; [reflexivity|]).
  destruct tag as [|[|tag]].
  - exact (False_ind _ (not_bigrat eq_refl)).
  - exact (False_ind _ (not_fixed eq_refl)).
  - reflexivity.
Qed.

Lemma expr_other_message_arm_reads_exact_score :
  forall child raw values,
    Core.source_summary_value child = Core.TypedSourceMessage raw values ->
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_tag raw <> 36 ->
    Schema.raw_tag raw <> 37 ->
    Target.exact_expr_score [Projection.project_source_summary child] =
    option_map Core.project_score (Process.source_expr_score [child]).
Proof.
  intros child raw values typed payload not_bigrat not_fixed.
  assert (tag_same : Core.source_summary_tag child = Schema.raw_tag raw).
  { unfold Core.source_summary_tag, Core.source_summary_raw.
    rewrite typed.
    reflexivity. }
  assert (projected_payload :
    Target.sem_payload (Projection.project_source_summary child) =
      Schema.Message).
  { change (Target.sem_payload (Projection.project_source_summary child))
      with (Schema.raw_payload (Core.source_summary_raw child)).
    unfold Core.source_summary_raw.
    rewrite typed.
    exact payload. }
  assert (projected_tag :
    Target.sem_tag (Projection.project_source_summary child) =
      Schema.raw_tag raw).
  { change (Target.sem_tag (Projection.project_source_summary child))
      with (Schema.raw_tag (Core.source_summary_raw child)).
    unfold Core.source_summary_raw.
    rewrite typed.
    reflexivity. }
  assert (target_not_bigrat :
    Target.sem_tag (Projection.project_source_summary child) <> 36)
    by (rewrite projected_tag; exact not_bigrat).
  assert (target_not_fixed :
    Target.sem_tag (Projection.project_source_summary child) <> 37)
    by (rewrite projected_tag; exact not_fixed).
  assert (source_not_bigrat : Core.source_summary_tag child <> 36)
    by (rewrite tag_same; exact not_bigrat).
  assert (source_not_fixed : Core.source_summary_tag child <> 37)
    by (rewrite tag_same; exact not_fixed).
  rewrite (target_expr_score_of_other_message_summary
    (Projection.project_source_summary child) projected_payload
    target_not_bigrat target_not_fixed).
  rewrite (source_expr_score_of_other_message_child child raw values typed
    source_not_bigrat source_not_fixed).
  change (Target.sem_score_evidence (Projection.project_source_summary child))
    with (Projection.project_source_evidence
      (Core.source_summary_exact_score child)).
  unfold Projection.project_source_evidence.
  destruct (Core.source_summary_exact_score child); reflexivity.
Qed.

Lemma expr_other_message_arm_corresponds :
  forall fuel hash_fn body program span field nested child,
    Schema.lookup_projected_program 24 [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    Schema.descriptor_tag field <> 36 ->
    Schema.descriptor_tag field <> 37 ->
    Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body program (Source.SourceMessage span field nested) =
      Some child ->
    Target.exact_expr_score [Projection.project_source_summary child] =
    option_map Core.project_score (Process.source_expr_score [child]).
Proof.
  intros fuel hash_fn body program span field nested child looked_up selected
    not_bigrat not_fixed evaluated.
  destruct (RPB3IndependentOracleProcessChildProvenance.accepted_generated_message_node_has_descriptor_provenance
    fuel hash_fn body 24 program span field nested child looked_up selected
    evaluated)
    as [raw [values [child_type
      [_ [typed [retained [raw_tag [_ [payload [_ _]]]]]]]]]].
  rewrite <- raw_tag in not_bigrat, not_fixed.
  exact (expr_other_message_arm_reads_exact_score child raw values typed payload
    not_bigrat not_fixed).
Qed.

Lemma expr_message_node_corresponds :
  forall fuel hash_fn body program span field nested child,
    Schema.lookup_projected_program 24 [] [] = Some program ->
    Source.source_descriptor program span = Some field ->
    Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body program (Source.SourceMessage span field nested) =
      Some child ->
    Target.exact_expr_score [Projection.project_source_summary child] =
    option_map Core.project_score (Process.source_expr_score [child]).
Proof.
  intros fuel hash_fn body program span field nested child looked_up selected
    evaluated.
  destruct (Nat.eq_dec (Schema.descriptor_tag field) 36)
    as [is_bigrat|not_bigrat].
  { exact (expr_nested_ground_arm_corresponds fuel hash_fn body program span
      field nested child looked_up selected (or_introl is_bigrat) evaluated). }
  destruct (Nat.eq_dec (Schema.descriptor_tag field) 37)
    as [is_fixed|not_fixed].
  { exact (expr_nested_ground_arm_corresponds fuel hash_fn body program span
      field nested child looked_up selected (or_intror is_fixed) evaluated). }
  exact (expr_other_message_arm_corresponds fuel hash_fn body program span
    field nested child looked_up selected not_bigrat not_fixed evaluated).
Qed.

(** ** The auxiliary node

    The generated Expr program declares no map field
    ([expr_arm_enumeration]), but the assembling theorem quantifies over every
    decoded node, so the auxiliary shape is handled by the reduction used in
    [Correspondence]: a successful auxiliary evaluation yields a typed map
    value, on which both readers return [None]. *)

Ltac finish_auxiliary_node evaluated :=
  cbv beta iota zeta in evaluated;
  lazymatch type of evaluated with
  | Some _ = Some _ => idtac
  | None = Some _ => discriminate evaluated
  | (match ?scrutinee with _ => _ end) = Some _ =>
      destruct scrutinee; finish_auxiliary_node evaluated
  end.

Lemma evaluated_auxiliary_child_is_unscored :
  forall recurse hash_fn body program span field child,
    Oracle.source_evaluate_node recurse hash_fn body program
      (Source.SourceAuxiliary span field) = Some child ->
    Target.exact_expr_score [Projection.project_source_summary child] = None /\
    Process.source_expr_score [child] = None.
Proof.
  intros recurse hash_fn body program span field child evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source body (Source.SourceAuxiliary span field))
    as [raw|] eqn:converted; [|discriminate evaluated].
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    body (Source.SourceAuxiliary span field) raw converted)
    as [_ [_ [raw_payload_is_descriptor _]]].
  cbn [Source.source_field] in raw_payload_is_descriptor.
  destruct (Schema.descriptor_payload field) eqn:descriptor_kind;
    [discriminate evaluated|discriminate evaluated| |].
  all: finish_auxiliary_node evaluated.
  all: injection evaluated as same_child; subst child.
  all: split;
    [ unfold Target.exact_expr_score, Projection.project_source_summary,
        Core.source_summary_raw;
      cbn [Core.source_value_raw Core.source_summary_value Target.sem_payload];
      rewrite raw_payload_is_descriptor;
      reflexivity
    | reflexivity ].
Qed.

(** ** Assembly *)

Theorem process_actual_children_expr_arm_corresponds :
  forall fuel hash_fn program body children child,
    Correspondence.process_actual_children fuel hash_fn 24 program body children ->
    In child children ->
    Target.exact_expr_score [Projection.project_source_summary child] =
    option_map Core.project_score (Process.source_expr_score [child]).
Proof.
  intros fuel hash_fn program body children child actual member.
  unfold Correspondence.process_actual_children in actual.
  destruct actual as [_ [looked_up [_ [nodes [decoded sequenced]]]]].
  destruct (Correspondence.child_of_sequenced_nodes_has_node _ _
    (Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body program) nodes children child sequenced member)
    as [node [node_member evaluated]].
  pose proof (Source.decoded_region_nodes_match_generated_source
    (S (length body)) body program 0 (length body) nodes decoded) as generated.
  rewrite Forall_forall in generated.
  destruct (generated node node_member) as [selected _].
  destruct node as [span field|span field nested|span field];
    cbn [Source.source_span Source.source_field] in selected.
  - destruct (Correspondence.evaluated_scalar_node_is_gated _ hash_fn body
      program span field child evaluated)
      as [raw [scalar [typed [_ [payload accepted]]]]].
    exact (expr_actual_scalar_child_corresponds program child raw scalar
      looked_up typed payload accepted).
  - exact (expr_message_node_corresponds fuel hash_fn body program span field
      nested child looked_up selected evaluated).
  - destruct (evaluated_auxiliary_child_is_unscored _ hash_fn body program span
      field child evaluated) as [target_none source_none].
    rewrite target_none, source_none.
    reflexivity.
Qed.

Print Assumptions expr_arm_enumeration.
Print Assumptions target_expr_score_of_scalar_summary.
Print Assumptions source_expr_score_of_scalar_summary.
Print Assumptions target_expr_score_of_scalar_child.
Print Assumptions decoded_small_varint_is_singleton.
Print Assumptions decoded_gbool_value_and_body.
Print Assumptions expr_gbool_arm_scores_agree.
Print Assumptions decoded_gtext_is_body.
Print Assumptions decoded_gbytes_is_body.
Print Assumptions expr_text_arm_scores_agree.
Print Assumptions expr_bytes_arm_scores_agree.
Print Assumptions decoded_gint_is_zigzag_value.
Print Assumptions expr_gint_arm_scores_agree.
Print Assumptions decoded_gdouble_is_little_endian.
Print Assumptions target_gdouble_semantic_value.
Print Assumptions expr_gdouble_arm_scores_agree.
Print Assumptions expr_fields_contract_checked_true.
Print Assumptions expr_field_contract_row_holds.
Print Assumptions resolved_generated_field_has_requested_tag.
Print Assumptions expr_scalar_value_gate.
Print Assumptions expr_scalar_arm_dispatch.
Print Assumptions expr_actual_scalar_child_corresponds.
Print Assumptions expr_nested_ground_arm_corresponds.
Print Assumptions target_expr_score_of_other_message_summary.
Print Assumptions source_expr_score_of_other_message_child.
Print Assumptions expr_other_message_arm_reads_exact_score.
Print Assumptions expr_other_message_arm_corresponds.
Print Assumptions expr_message_node_corresponds.
Print Assumptions evaluated_auxiliary_child_is_unscored.
Print Assumptions process_actual_children_expr_arm_corresponds.
