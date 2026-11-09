From Stdlib Require Import List Bool Arith NArith ZArith.
From StackSafePDA Require Import StackSafePDA RPB3WireCursor
  RPB3GeneratedFieldRefinement RPB3DescriptorProjectionData
  RPB3RawRegionTree.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Wire := RPB3WireCursor.

Record scalar_rule := ScalarRule {
  scalar_tag : nat;
  scalar_kind : Data.protobuf_kind;
  scalar_default_forbidden : bool
}.

Definition slot_scalar_rules (slot : Data.projection_slot)
    : list scalar_rule :=
  match Data.slot_arms slot with
  | [] =>
      match Data.slot_wire slot,
          Schema.projected_slot_payload slot with
      | Some _, Schema.Scalar =>
          [ScalarRule (Data.slot_min_tag slot)
            (Data.slot_protobuf_kind slot)
            (match Data.slot_default slot with
             | Data.DefaultForbidden => true
             | _ => false
             end)]
      | _, _ => []
      end
  | arms =>
      flat_map (fun arm =>
        match Data.arm_wire arm, Data.arm_payload arm with
        | Some _, Data.PKMessage => []
        | Some _, _ =>
            [ScalarRule (Data.arm_tag arm)
              (Data.arm_protobuf_kind arm) false]
        | None, _ => []
        end) arms
  end.

Definition generated_scalar_rule (program : Schema.generated_program)
    (tag : nat) : option scalar_rule :=
  match find (fun row => Data.program_type_id row =?
      Schema.program_family program) Data.generated_programs with
  | None => None
  | Some row =>
      find (fun rule => scalar_tag rule =? tag)
        (flat_map slot_scalar_rules (Data.program_slots row))
  end.

Definition scalar_rule_wire (kind : Data.protobuf_kind)
    : option Schema.wire_kind :=
  match kind with
  | Data.PBBool | Data.PBInt32 | Data.PBSint32 | Data.PBUint32
  | Data.PBInt64 | Data.PBSint64 | Data.PBUint64 =>
      Some Schema.Varint
  | Data.PBSfixed32 | Data.PBFixed32 | Data.PBFloat =>
      Some Schema.Fixed32
  | Data.PBSfixed64 | Data.PBFixed64 | Data.PBDouble =>
      Some Schema.Fixed64
  | Data.PBString | Data.PBRepeatedString
  | Data.PBBytes | Data.PBRepeatedBytes =>
      Some Schema.LengthDelimited
  | _ => None
  end.

Definition projected_scalar_field_is_covered
    (row : Data.projection_program)
    (field : Schema.field_descriptor) : bool :=
  match Schema.descriptor_payload field with
  | Schema.Scalar =>
      match generated_scalar_rule
          (Schema.project_generated_program row [] [])
          (Schema.descriptor_tag field) with
      | Some rule =>
          match scalar_rule_wire (scalar_kind rule) with
          | Some wire =>
              Schema.wire_eqb wire (Schema.descriptor_wire field)
          | None => false
          end
      | None => false
      end
  | _ => true
  end.

Definition all_projected_scalar_fields_are_covered : bool :=
  forallb (fun row =>
    forallb (projected_scalar_field_is_covered row)
      (Schema.program_fields
        (Schema.project_generated_program row [] [])))
    Data.generated_programs.

Theorem finite_generated_scalar_projection_is_covered :
  all_projected_scalar_fields_are_covered = true.
Proof. vm_compute. reflexivity. Qed.

Lemma found_generated_row_is_member_and_matches :
  forall type_id rows row,
    find (fun candidate => Data.program_type_id candidate =? type_id)
      rows = Some row ->
    In row rows /\ Data.program_type_id row = type_id.
Proof.
  intros type_id rows.
  induction rows as [|first rest IH]; intros row selected;
    simpl in selected; [discriminate|].
  destruct (Data.program_type_id first =? type_id)
    eqn:matches.
  - inversion selected; subst row.
    split; [left; reflexivity|].
    now apply Nat.eqb_eq.
  - destruct (IH row selected) as [member same_id].
    split; [right; exact member|exact same_id].
Qed.

Theorem every_generated_scalar_descriptor_selects_a_rule :
  forall type_id program field,
    Schema.lookup_projected_program type_id [] [] = Some program ->
    In field (Schema.program_fields program) ->
    Schema.descriptor_payload field = Schema.Scalar ->
    exists rule wire,
      generated_scalar_rule program (Schema.descriptor_tag field) =
        Some rule /\
      scalar_rule_wire (scalar_kind rule) = Some wire /\
      Schema.wire_eqb wire (Schema.descriptor_wire field) = true.
Proof.
  intros type_id program field looked_up member scalar.
  unfold Schema.lookup_projected_program in looked_up.
  destruct (find (fun row => Data.program_type_id row =? type_id)
      Data.generated_programs) as [row|] eqn:selected;
    [|discriminate].
  inversion looked_up; subst program; clear looked_up.
  destruct (found_generated_row_is_member_and_matches
    type_id Data.generated_programs row selected)
    as [row_member same_id].
  subst type_id.
  pose proof finite_generated_scalar_projection_is_covered
    as covered.
  unfold all_projected_scalar_fields_are_covered in covered.
  rewrite forallb_forall in covered.
  specialize (covered row row_member).
  rewrite forallb_forall in covered.
  specialize (covered field member).
  unfold projected_scalar_field_is_covered in covered.
  rewrite scalar in covered.
  destruct (generated_scalar_rule
      (Schema.project_generated_program row [] [])
      (Schema.descriptor_tag field)) as [rule|] eqn:found;
    [|discriminate].
  destruct (scalar_rule_wire (scalar_kind rule))
    as [wire|] eqn:wire_found; [|discriminate].
  exists rule, wire.
  now repeat split.
Qed.

Definition compact_varint_value (body : list nat) : option N :=
  match Wire.canonical_varint body (length body) 0 with
  | Some (value, stop) =>
      if stop =? length body then Some value else None
  | None => None
  end.

Definition numeric_scalar_valid (kind : Data.protobuf_kind)
    (forbidden : bool) (value : N) : bool :=
  let nondefault := negb forbidden || negb (N.eqb value 0) in
  match kind with
  | Data.PBBool => N.leb value 1 && nondefault
  | Data.PBInt32 =>
      (N.leb value 2147483647 ||
        N.leb 18446744071562067968 value) && nondefault
  | Data.PBSint32 | Data.PBUint32 =>
      N.leb value 4294967295 && nondefault
  | Data.PBInt64 | Data.PBSint64 | Data.PBUint64 => nondefault
  | _ => false
  end.

Fixpoint nat_list_eqb (left right : list nat) : bool :=
  match left, right with
  | [], [] => true
  | first :: rest, second :: suffix =>
      (first =? second) && nat_list_eqb rest suffix
  | _, _ => false
  end.

Definition fixed_scalar_valid (kind : Data.protobuf_kind)
    (forbidden : bool) (body : list nat) : bool :=
  let nonzero := negb (forallb (Nat.eqb 0) body) in
  match kind with
  | Data.PBSfixed32 | Data.PBFixed32 =>
      (length body =? 4) && (negb forbidden || nonzero)
  | Data.PBSfixed64 | Data.PBFixed64 =>
      (length body =? 8) && (negb forbidden || nonzero)
  | Data.PBFloat =>
      (length body =? 4) &&
      (negb forbidden ||
        negb (nat_list_eqb body [0; 0; 0; 0] ||
          nat_list_eqb body [0; 0; 0; 128]))
  | Data.PBDouble =>
      (length body =? 8) &&
      (negb forbidden ||
        negb (nat_list_eqb body [0; 0; 0; 0; 0; 0; 0; 0] ||
          nat_list_eqb body [0; 0; 0; 0; 0; 0; 0; 128]))
  | _ => false
  end.

Definition scalar_body_valid (rule : scalar_rule)
    (body : list nat) : bool :=
  let forbidden := scalar_default_forbidden rule in
  match scalar_kind rule with
  | Data.PBBool | Data.PBInt32 | Data.PBSint32 | Data.PBUint32
  | Data.PBInt64 | Data.PBSint64 | Data.PBUint64 =>
      match compact_varint_value body with
      | Some value => numeric_scalar_valid (scalar_kind rule)
          forbidden value
      | None => false
      end
  | Data.PBSfixed32 | Data.PBFixed32 | Data.PBFloat
  | Data.PBSfixed64 | Data.PBFixed64 | Data.PBDouble =>
      fixed_scalar_valid (scalar_kind rule) forbidden body
  | Data.PBString | Data.PBRepeatedString =>
      Source.utf8_bytes body &&
        (negb forbidden || negb (Nat.eqb (length body) 0))
  | Data.PBBytes | Data.PBRepeatedBytes =>
      negb forbidden || negb (Nat.eqb (length body) 0)
  | _ => false
  end.

Inductive scalar_semantic_value :=
| ScalarBool (value : bool)
| ScalarI32 (value : Z)
| ScalarU32 (value : N)
| ScalarI64 (value : Z)
| ScalarU64 (value : N)
| ScalarString (value : list nat)
| ScalarBytes (value : list nat).

Fixpoint little_endian_value (body : list nat) : N :=
  match body with
  | [] => 0
  | byte :: rest =>
      (N.of_nat byte + 256 * little_endian_value rest)%N
  end.

Definition signed_32 (value : N) : Z :=
  if N.ltb value 2147483648 then Z.of_N value
  else (Z.of_N value - 4294967296)%Z.

Definition signed_64 (value : N) : Z :=
  if N.ltb value 9223372036854775808 then Z.of_N value
  else (Z.of_N value - 18446744073709551616)%Z.

Definition zigzag_signed (value : N) : Z :=
  if N.even value then Z.of_N (value / 2)%N
  else (- Z.of_N (value / 2 + 1)%N)%Z.

Definition scalar_semantic_from_source (rule : scalar_rule)
    (body : list nat) : option scalar_semantic_value :=
  if scalar_body_valid rule body then
    match scalar_kind rule with
    | Data.PBBool =>
        option_map (fun value => ScalarBool (negb (N.eqb value 0)))
          (compact_varint_value body)
    | Data.PBInt32 =>
        option_map (fun value => ScalarI32 (signed_64 value))
          (compact_varint_value body)
    | Data.PBSint32 =>
        option_map (fun value => ScalarI32 (zigzag_signed value))
          (compact_varint_value body)
    | Data.PBUint32 =>
        option_map (fun value => ScalarU32 value)
          (compact_varint_value body)
    | Data.PBInt64 =>
        option_map (fun value => ScalarI64 (signed_64 value))
          (compact_varint_value body)
    | Data.PBSint64 =>
        option_map (fun value => ScalarI64 (zigzag_signed value))
          (compact_varint_value body)
    | Data.PBUint64 =>
        option_map (fun value => ScalarU64 value)
          (compact_varint_value body)
    | Data.PBSfixed32 =>
        Some (ScalarI32 (signed_32 (little_endian_value body)))
    | Data.PBFixed32 | Data.PBFloat =>
        Some (ScalarU32 (little_endian_value body))
    | Data.PBSfixed64 =>
        Some (ScalarI64 (signed_64 (little_endian_value body)))
    | Data.PBFixed64 | Data.PBDouble =>
        Some (ScalarU64 (little_endian_value body))
    | Data.PBString | Data.PBRepeatedString =>
        Some (ScalarString body)
    | Data.PBBytes | Data.PBRepeatedBytes =>
        Some (ScalarBytes body)
    | _ => None
    end
  else None.

Definition source_scalar_semantic (bytes : list nat)
    (program : Schema.generated_program) (node : Source.source_tree)
    : option scalar_semantic_value :=
  match generated_scalar_rule program
      (Wire.span_number (Source.source_span node)) with
  | Some rule =>
      scalar_semantic_from_source rule (Source.source_body bytes node)
  | None => None
  end.

Theorem scalar_semantic_value_is_rejected_with_invalid_body :
  forall rule body,
    scalar_body_valid rule body = false ->
    scalar_semantic_from_source rule body = None.
Proof.
  intros rule body invalid.
  unfold scalar_semantic_from_source.
  now rewrite invalid.
Qed.

Theorem accepted_scalar_body_has_semantic_value :
  forall rule body,
    scalar_body_valid rule body = true ->
    exists value,
      scalar_semantic_from_source rule body = Some value.
Proof.
  intros [tag kind forbidden] body valid.
  unfold scalar_semantic_from_source.
  rewrite valid.
  unfold scalar_body_valid in valid.
  destruct kind; simpl in valid |- *;
    try (destruct (compact_varint_value body) as [number|]
      eqn:parsed; [eexists; reflexivity|discriminate]);
    try (eexists; reflexivity);
    try discriminate.
Qed.

Example generated_par_connective_semantic_true :
  scalar_semantic_from_source (ScalarRule 10 Data.PBBool true) [1] =
    Some (ScalarBool true).
Proof. reflexivity. Qed.

Example generated_oneof_zero_bool_is_preserved :
  scalar_semantic_from_source (ScalarRule 1 Data.PBBool false) [0] =
    Some (ScalarBool false).
Proof. reflexivity. Qed.

Example signed_int32_alias_refuses_before_projection :
  scalar_semantic_from_source (ScalarRule 1 Data.PBInt32 false)
    [128; 128; 128; 128; 8] = None.
Proof. reflexivity. Qed.

Definition source_scalar_raw (bytes : list nat)
    (program : Schema.generated_program) (node : Source.source_tree)
    : option Schema.raw_field :=
  match node with
  | Source.SourceScalar span _ =>
      match Source.raw_field_of_source bytes node,
          Source.source_descriptor program span,
          generated_scalar_rule program (Wire.span_number span) with
      | Some raw, Some field, Some rule =>
          if Schema.payload_eqb (Schema.descriptor_payload field)
              Schema.Scalar &&
              scalar_body_valid rule (Schema.raw_body raw) &&
              Schema.valid_raw_body raw then
            Some raw
          else None
      | _, _, _ => None
      end
  | _ => None
  end.

Theorem accepted_source_scalar_reuses_raw_tree_leaf :
  forall fuel bytes program node raw,
    source_scalar_raw bytes program node = Some raw ->
    Source.raw_tree_of_source (S fuel) bytes node =
      Some (Node raw FNil).
Proof.
  intros fuel bytes program node raw reduced.
  unfold source_scalar_raw in reduced.
  destruct node as [span field|span field children|span field];
    cbn [Source.source_span] in reduced; try discriminate.
  destruct (Source.raw_field_of_source bytes
      (Source.SourceScalar span field)) as [existing|]
    eqn:converted; [|discriminate].
  destruct (Source.source_descriptor program span) as [selected|];
    [|discriminate].
  destruct (generated_scalar_rule program (Wire.span_number span))
    as [rule|]; [|discriminate].
  destruct (Schema.payload_eqb (Schema.descriptor_payload selected)
      Schema.Scalar && scalar_body_valid rule
      (Schema.raw_body existing) && Schema.valid_raw_body existing);
    [|discriminate].
  inversion reduced; subst raw.
  cbn [Source.raw_tree_of_source].
  now rewrite converted.
Qed.

Theorem accepted_source_scalar_has_semantic_projection :
  forall bytes program node raw,
    source_scalar_raw bytes program node = Some raw ->
    exists value,
      source_scalar_semantic bytes program node = Some value.
Proof.
  intros bytes program node raw reduced.
  unfold source_scalar_raw in reduced.
  destruct node as [span field|span field children|span field];
    cbn [Source.source_span] in reduced; try discriminate.
  destruct (Source.raw_field_of_source bytes
      (Source.SourceScalar span field)) as [source_raw|]
    eqn:converted; [|discriminate].
  destruct (Source.source_descriptor program span) as [selected|]
    eqn:descriptor; [|discriminate].
  destruct (generated_scalar_rule program (Wire.span_number span))
    as [rule|] eqn:rule_found; [|discriminate].
  destruct (Schema.payload_eqb (Schema.descriptor_payload selected)
      Schema.Scalar && scalar_body_valid rule
      (Schema.raw_body source_raw) &&
      Schema.valid_raw_body source_raw)
    eqn:accepted; [|discriminate].
  apply andb_true_iff in accepted as [guard _].
  apply andb_true_iff in guard as [_ grammar].
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    bytes (Source.SourceScalar span field) source_raw converted)
    as [_ [_ [_ [original _]]]].
  rewrite original in grammar.
  destruct (accepted_scalar_body_has_semantic_value rule
    (Source.source_body bytes (Source.SourceScalar span field)) grammar)
    as [value projected].
  exists value.
  unfold source_scalar_semantic.
  simpl.
  now rewrite rule_found.
Qed.

Theorem source_scalar_raw_has_exact_body_and_reference_leaf :
  forall fuel bytes program region_start region_end nodes node raw,
    Schema.lookup_projected_program (Schema.program_family program)
      [] [] = Some program ->
    Source.decode_region fuel bytes program region_start region_end =
      Some nodes ->
    In node nodes ->
    source_scalar_raw bytes program node = Some raw ->
    Source.source_descriptor program (Source.source_span node) =
      Some (Source.source_field node) /\
    Schema.raw_body raw = Source.source_body bytes node /\
    Schema.summary_body
      (Schema.typed_reference (Node raw FNil)) =
        Source.source_body bytes node /\
    Schema.summary_valid
      (Schema.typed_reference (Node raw FNil)) = true.
Proof.
  intros fuel bytes program region_start region_end nodes node raw
    generated_program decoded member reduced.
  pose proof (Source.decoded_region_nodes_match_generated_source
    fuel bytes program region_start region_end nodes decoded)
    as generated.
  rewrite Forall_forall in generated.
  destruct (generated node member) as [stored _].
  unfold source_scalar_raw in reduced.
  destruct node as [span field|span field children|span field];
    cbn [Source.source_span] in reduced; try discriminate.
  destruct (Source.raw_field_of_source bytes
      (Source.SourceScalar span field)) as [source_raw|]
    eqn:converted; [|discriminate].
  destruct (Source.source_descriptor program span) as [selected|]
    eqn:descriptor; [|discriminate].
  destruct (generated_scalar_rule program (Wire.span_number span))
    as [rule|] eqn:rule_found; [|discriminate].
  destruct (Schema.payload_eqb (Schema.descriptor_payload selected)
      Schema.Scalar && scalar_body_valid rule
      (Schema.raw_body source_raw) &&
      Schema.valid_raw_body source_raw)
    eqn:accepted; [|discriminate].
  apply andb_true_iff in accepted as [_ raw_valid].
  inversion reduced; subst raw; clear reduced.
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    bytes (Source.SourceScalar span field) source_raw converted)
    as [_ [_ [_ [original _]]]].
  split; [exact stored|].
  split; [exact original|].
  split; [exact original|].
  unfold Source.raw_field_of_source in converted.
  simpl in converted.
  destruct (Schema.descriptor_payload field);
    simpl in converted; try discriminate.
  inversion converted; subst source_raw.
  simpl in raw_valid |- *.
  now rewrite raw_valid.
Qed.

Theorem source_scalar_invalid_body_refuses_raw_reduction :
  forall bytes program span field rule,
    Source.source_descriptor program span = Some field ->
    generated_scalar_rule program (Wire.span_number span) = Some rule ->
    scalar_body_valid rule
      (Source.source_body bytes (Source.SourceScalar span field)) = false ->
    source_scalar_raw bytes program
      (Source.SourceScalar span field) = None.
Proof.
  intros bytes program span field rule descriptor selected invalid.
  unfold source_scalar_raw.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceScalar span field)) as [raw|]
    eqn:converted; [|reflexivity].
  rewrite descriptor, selected.
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    bytes (Source.SourceScalar span field) raw converted)
    as [_ [_ [_ [original _]]]].
  rewrite original, invalid.
  destruct (Schema.payload_eqb (Schema.descriptor_payload field)
    Schema.Scalar); reflexivity.
Qed.

Print Assumptions source_scalar_raw_has_exact_body_and_reference_leaf.
Print Assumptions accepted_source_scalar_reuses_raw_tree_leaf.
Print Assumptions source_scalar_invalid_body_refuses_raw_reduction.
Print Assumptions finite_generated_scalar_projection_is_covered.
Print Assumptions every_generated_scalar_descriptor_selects_a_rule.
Print Assumptions scalar_semantic_value_is_rejected_with_invalid_body.
Print Assumptions accepted_scalar_body_has_semantic_value.
Print Assumptions accepted_source_scalar_has_semantic_projection.
