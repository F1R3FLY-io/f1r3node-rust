From Stdlib Require Import List Bool Arith NArith Lia.
From StackSafePDA Require Import StackSafePDA
  RPB3DescriptorProjectionData RPB3GeneratedFieldRefinement
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleCost RPB3IndependentOracleProjection
  RPB3IndependentOracleProcessScalarDecoderBridge
  RPB3IndependentOracleCostScalarDecoderBridge.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Cost := RPB3IndependentOracleCost.
Module Projection := RPB3IndependentOracleProjection.
Module Decoder := RPB3IndependentOracleProcessScalarDecoderBridge.
Module CostDecoder := RPB3IndependentOracleCostScalarDecoderBridge.

Lemma valid_cost_scan_matches_each_child :
  forall program state children,
    Admission.source_scan_valid
      (fold_left (Admission.source_scan_step program) children state) = true ->
    Forall (fun child => exists descriptor,
      Schema.resolve_tag (Schema.program_fields program)
        (Core.source_summary_tag child) = Some descriptor /\
      Admission.source_child_matches_descriptor descriptor child = true)
      children.
Proof.
  intros program state children.
  revert state.
  induction children as [|child rest IH]; intros state final.
  - constructor.
  - cbn [fold_left] in final.
    assert (step_valid : Admission.source_scan_valid
      (Admission.source_scan_step program state child) = true).
    { destruct (Admission.source_scan_valid
        (Admission.source_scan_step program state child)) eqn:step;
        [reflexivity|].
      pose proof (Projection.source_scan_invalid_stays_invalid
        program rest (Admission.source_scan_step program state child)
        step) as invalid.
      rewrite invalid in final. discriminate. }
    assert (state_valid : Admission.source_scan_valid state = true).
    { destruct (Admission.source_scan_valid state) eqn:state_valid;
        [reflexivity|].
      rewrite (Admission.source_scan_step_invalid_sticky
        program state child state_valid) in step_valid.
      rewrite state_valid in step_valid. discriminate. }
    pose proof step_valid as checked.
    unfold Admission.source_scan_step in checked.
    rewrite state_valid in checked.
    destruct (Schema.resolve_tag (Schema.program_fields program)
        (Core.source_summary_tag child)) as [descriptor|]
      eqn:resolved; [|discriminate checked].
    destruct (Admission.source_child_matches_descriptor descriptor child &&
        (Schema.descriptor_slot descriptor <?
          Schema.program_slot_count program) &&
        Admission.source_slot_follows (Admission.source_scan_last_slot state)
          (Schema.descriptor_slot descriptor)
          (Schema.descriptor_repeated descriptor) &&
        (Schema.descriptor_repeated descriptor ||
          negb (existsb (Nat.eqb (Schema.descriptor_slot descriptor))
            (Admission.source_scan_seen_slots state))))
      eqn:guard; [|discriminate checked].
    apply andb_true_iff in guard as [prefix _].
    apply andb_true_iff in prefix as [prefix _].
    apply andb_true_iff in prefix as [matches _].
    constructor.
    + exists descriptor. split; assumption.
    + apply IH with
        (state := Admission.source_scan_step program state child).
      exact final.
Qed.

Lemma cost_forest_children_have_source_trees :
  forall program subjects children,
    Cost.source_cost_forest program subjects = Some children ->
    Forall (fun child => exists subject,
      Cost.source_cost_tree (Some program) subject = Some child) children.
Proof.
  intros program subjects.
  induction subjects as [|subject rest IH]; intros children accepted.
  - inversion accepted; constructor.
  - cbn [Cost.source_cost_forest] in accepted.
    destruct (Cost.source_cost_tree (Some program) subject) as [child|]
      eqn:head; [|discriminate].
    destruct (Cost.source_cost_forest program rest) as [tail|]
      eqn:suffix; [|discriminate].
    inversion accepted; subst children.
    constructor.
    + exists subject. exact head.
    + apply IH. reflexivity.
Qed.

Lemma cost_tree_child_shape :
  forall program subject child,
    Cost.source_cost_tree (Some program) subject = Some child ->
    (exists raw scalar,
      Schema.raw_payload raw = Schema.Scalar /\
      Core.source_summary_value child =
        Core.TypedSourceScalar raw scalar /\
      Cost.source_scalar_leaf program raw = Some child) \/
    (exists raw values,
      Schema.raw_payload raw = Schema.Message /\
      Core.source_summary_value child =
        Core.TypedSourceMessage raw values).
Proof.
  intros program [raw subjects] child accepted.
  cbn [Cost.source_cost_tree] in accepted.
  destruct (Schema.raw_payload raw) eqn:payload;
    try discriminate.
  - destruct subjects as [|subject rest]; [|discriminate].
    pose proof accepted as leaf.
    unfold Cost.source_scalar_leaf,
      Admission.source_typed_scalar in accepted.
    destruct (Admission.source_scalar_value_of_raw program raw)
      as [scalar|] eqn:decoded; [|discriminate].
    cbn [option_map] in accepted.
    inversion accepted; subst child.
    left. exists raw, scalar.
    repeat split; try assumption; reflexivity.
  - destruct (Cost.cost_program raw) as [[family nested_program]|]
      eqn:selected; [|discriminate].
    destruct (Cost.source_cost_forest nested_program subjects)
      as [nested_children|] eqn:parsed; [|discriminate].
    unfold Cost.source_cost_message in accepted.
    destruct (Admission.source_raw_bytes_well_formed raw &&
        Admission.source_scan_valid
          (Admission.source_scan_children nested_program nested_children) &&
        Admission.source_required_pathmaps_present nested_program
          (Admission.source_scan_children nested_program nested_children) &&
        (Schema.program_family nested_program =? family))
      eqn:guard; [|discriminate].
    destruct (Cost.cost_family_score family nested_children) as [score|]
      eqn:scored; [|discriminate].
    cbn [option_map] in accepted.
    inversion accepted; subst child.
    right. exists raw, (map Core.source_summary_value nested_children).
    split; [exact payload|reflexivity].
Qed.

Lemma matched_cost_child_payload :
  forall descriptor child,
    Admission.source_child_matches_descriptor descriptor child = true ->
    Schema.raw_payload (Core.source_summary_raw child) =
      Schema.descriptor_payload descriptor.
Proof.
  intros descriptor child matched.
  unfold Admission.source_child_matches_descriptor in matched.
  repeat rewrite andb_true_iff in matched.
  destruct matched as [[[[_ _] _] payload] _].
  destruct (Schema.raw_payload (Core.source_summary_raw child)),
    (Schema.descriptor_payload descriptor);
    cbn in payload; try discriminate; reflexivity.
Qed.

Lemma scalar_cost_summary_raw :
  forall child raw scalar,
    Core.source_summary_value child =
      Core.TypedSourceScalar raw scalar ->
    Core.source_summary_raw child = raw.
Proof.
  intros child raw scalar value.
  unfold Core.source_summary_raw.
  now rewrite value.
Qed.

Lemma message_cost_summary_raw :
  forall child raw values,
    Core.source_summary_value child =
      Core.TypedSourceMessage raw values ->
    Core.source_summary_raw child = raw.
Proof.
  intros child raw values value.
  unfold Core.source_summary_raw.
  now rewrite value.
Qed.

Lemma descriptor_scalar_cost_child_has_leaf :
  forall program subject child descriptor,
    Cost.source_cost_tree (Some program) subject = Some child ->
    Admission.source_child_matches_descriptor descriptor child = true ->
    Schema.descriptor_payload descriptor = Schema.Scalar ->
    exists raw scalar,
      Core.source_summary_value child =
        Core.TypedSourceScalar raw scalar /\
      Cost.source_scalar_leaf program raw = Some child /\
      Schema.raw_tag raw = Core.source_summary_tag child.
Proof.
  intros program subject child descriptor accepted matched scalar_descriptor.
  pose proof (matched_cost_child_payload descriptor child matched)
    as payload.
  rewrite scalar_descriptor in payload.
  destruct (cost_tree_child_shape program subject child accepted) as
    [[raw [scalar [_ [value leaf]]]]|
     [raw [values [message_payload value]]]].
  - exists raw, scalar. repeat split; try assumption.
    unfold Core.source_summary_tag.
    now rewrite (scalar_cost_summary_raw child raw scalar value).
  - rewrite (message_cost_summary_raw child raw values value) in payload.
    rewrite message_payload in payload. discriminate.
Qed.

Lemma cost_scalar_leaf_decodes_declared_kind :
  forall family program raw kind forbidden child,
    Schema.program_family program = family ->
    Admission.source_scalar_contract_for family (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract kind forbidden) ->
    Cost.source_scalar_leaf program raw = Some child ->
    exists scalar,
      Core.source_summary_value child =
        Core.TypedSourceScalar raw scalar /\
      Core.independently_decode_scalar kind forbidden
        (Schema.raw_body raw) = Some scalar.
Proof.
  intros family program raw kind forbidden child family_id contract leaf.
  unfold Cost.source_scalar_leaf, Admission.source_typed_scalar,
    Admission.source_scalar_value_of_raw in leaf.
  rewrite family_id, contract in leaf.
  destruct (Schema.raw_payload raw); try discriminate.
  destruct (Schema.raw_program_type_id raw) as [type_id|];
    try discriminate.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Schema.raw_tag raw)) as [descriptor|]; [|discriminate].
  cbn [Admission.source_contract_kind
    Admission.source_contract_default_forbidden] in leaf.
  destruct (Admission.source_scalar_wire kind) as [wire|];
    [|discriminate].
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Schema.wire_eqb (Schema.raw_wire raw) wire &&
      Schema.wire_eqb wire (Schema.descriptor_wire descriptor) &&
      Schema.payload_eqb (Schema.descriptor_payload descriptor)
        Schema.Scalar); [|discriminate].
  destruct (Core.independently_decode_scalar kind forbidden
      (Schema.raw_body raw)) as [scalar|] eqn:decoded;
    [|discriminate].
  cbn [option_map] in leaf.
  inversion leaf; subst child.
  exists scalar. split; reflexivity.
Qed.

Lemma accepted_cost_pbbytes_is_body :
  forall body scalar,
    Core.independently_decode_scalar Data.PBBytes false body =
      Some scalar ->
    scalar = Core.SourceBlob body.
Proof.
  intros body scalar decoded.
  cbn [Core.independently_decode_scalar] in decoded.
  destruct (Core.bytes_in_range body &&
      Core.source_nondefault false body); [|discriminate].
  now inversion decoded.
Qed.

Lemma accepted_cost_pbsint32_shape :
  forall body scalar,
    Core.independently_decode_scalar Data.PBSint32 true body =
      Some scalar ->
    exists value, scalar = Core.SourceI32 value.
Proof.
  intros body scalar decoded.
  cbn [Core.independently_decode_scalar] in decoded.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|discriminate].
  unfold Core.source_numeric_scalar in decoded.
  destruct (Core.source_numeric_nondefault true number);
    [|discriminate].
  destruct (N.leb number 4294967295); [|discriminate].
  eexists. now inversion decoded.
Qed.

Lemma accepted_cost_pbuint32_shape :
  forall body scalar,
    Core.independently_decode_scalar Data.PBUint32 true body =
      Some scalar ->
    exists value, scalar = Core.SourceU32 value.
Proof.
  intros body scalar decoded.
  cbn [Core.independently_decode_scalar] in decoded.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|discriminate].
  unfold Core.source_numeric_scalar in decoded.
  destruct (Core.source_numeric_nondefault true number);
    [|discriminate].
  destruct (N.leb number 4294967295); [|discriminate].
  eexists. now inversion decoded.
Qed.

Lemma accepted_cost_pbuint64_shape :
  forall body scalar,
    Core.independently_decode_scalar Data.PBUint64 true body =
      Some scalar ->
    exists value, scalar = Core.SourceU64 value.
Proof.
  intros body scalar decoded.
  cbn [Core.independently_decode_scalar] in decoded.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|discriminate].
  unfold Core.source_numeric_scalar in decoded.
  destruct (Core.source_numeric_nondefault true number);
    [|discriminate].
  eexists. now inversion decoded.
Qed.

Lemma generated_cost_atom_bytes_contract :
  forall tag,
    In tag [1; 2; 4] ->
    Admission.source_scalar_contract_for 10 tag =
      Some (Admission.SourceScalarContract Data.PBBytes false).
Proof.
  intros tag member.
  destruct member as [member|[member|[member|[]]]];
    subst tag; vm_compute; reflexivity.
Qed.

Lemma generated_cost_atom_bytes_descriptor :
  forall program tag descriptor,
    Schema.lookup_projected_program 10 [] [] = Some program ->
    In tag [1; 2; 4] ->
    Schema.resolve_tag (Schema.program_fields program) tag =
      Some descriptor ->
    Schema.descriptor_payload descriptor = Schema.Scalar.
Proof.
  intros program tag descriptor found member resolved.
  destruct member as [member|[member|[member|[]]]]; subst tag;
    vm_compute in found; inversion found; subst program;
    vm_compute in resolved; inversion resolved; reflexivity.
Qed.

Lemma generated_cost_atom_message_descriptor :
  forall program descriptor,
    Schema.lookup_projected_program 10 [] [] = Some program ->
    Schema.resolve_tag (Schema.program_fields program) 3 =
      Some descriptor ->
    Schema.descriptor_payload descriptor = Schema.Message.
Proof.
  intros program descriptor found resolved.
  vm_compute in found. inversion found; subst program.
  vm_compute in resolved. inversion resolved. reflexivity.
Qed.

Lemma generated_cost_bound_scalar_descriptor :
  forall program descriptor,
    Schema.lookup_projected_program 11 [] [] = Some program ->
    Schema.resolve_tag (Schema.program_fields program) 1 =
      Some descriptor ->
    Schema.descriptor_payload descriptor = Schema.Scalar.
Proof.
  intros program descriptor found resolved.
  vm_compute in found. inversion found; subst program.
  vm_compute in resolved. inversion resolved. reflexivity.
Qed.

Lemma generated_cost_region_u32_descriptor :
  forall program tag descriptor,
    Schema.lookup_projected_program 12 [] [] = Some program ->
    In tag [1; 7] ->
    Schema.resolve_tag (Schema.program_fields program) tag =
      Some descriptor ->
    Schema.descriptor_payload descriptor = Schema.Scalar.
Proof.
  intros program tag descriptor found member resolved.
  destruct member as [member|[member|[]]]; subst tag;
    vm_compute in found; inversion found; subst program;
    vm_compute in resolved; inversion resolved; reflexivity.
Qed.

Lemma generated_cost_region_u64_descriptor :
  forall program tag descriptor,
    Schema.lookup_projected_program 12 [] [] = Some program ->
    In tag [2; 3; 4; 5] ->
    Schema.resolve_tag (Schema.program_fields program) tag =
      Some descriptor ->
    Schema.descriptor_payload descriptor = Schema.Scalar.
Proof.
  intros program tag descriptor found member resolved.
  destruct member as [member|[member|[member|[member|[]]]]];
    subst tag; vm_compute in found; inversion found; subst program;
    vm_compute in resolved; inversion resolved; reflexivity.
Qed.

Lemma generated_cost_region_signer_descriptor :
  forall program descriptor,
    Schema.lookup_projected_program 12 [] [] = Some program ->
    Schema.resolve_tag (Schema.program_fields program) 6 =
      Some descriptor ->
    Schema.descriptor_payload descriptor = Schema.Message.
Proof.
  intros program descriptor found resolved.
  vm_compute in found. inversion found; subst program.
  vm_compute in resolved. inversion resolved. reflexivity.
Qed.

Lemma generated_cost_atom_child_corresponds :
  forall program subject child descriptor,
    Schema.lookup_projected_program 10 [] [] = Some program ->
    Cost.source_cost_tree (Some program) subject = Some child ->
    Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child) = Some descriptor ->
    Admission.source_child_matches_descriptor descriptor child = true ->
    Projection.cost_child_corresponds 10 child.
Proof.
  intros program subject child descriptor found accepted resolved matched.
  unfold Projection.cost_child_corresponds.
  repeat split.
  - intros _ selected.
    assert (member : In (Core.source_summary_tag child) [1; 2; 4])
      by (destruct selected as [selected|[selected|selected]];
          rewrite selected; cbn; auto).
    pose proof (generated_cost_atom_bytes_descriptor program
      (Core.source_summary_tag child) descriptor found member resolved)
      as scalar_descriptor.
    destruct (descriptor_scalar_cost_child_has_leaf program subject child
      descriptor accepted matched scalar_descriptor)
      as [raw [scalar [value [leaf tag]]]].
    pose proof (Decoder.generated_program_has_requested_family
      10 program found) as family.
    assert (contract : Admission.source_scalar_contract_for 10
      (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract Data.PBBytes false)).
    { rewrite tag. apply generated_cost_atom_bytes_contract.
      exact member. }
    destruct (cost_scalar_leaf_decodes_declared_kind 10 program raw
      Data.PBBytes false child family contract leaf)
      as [decoded [source_value parsed]].
    rewrite value in source_value.
    inversion source_value; subst decoded.
    apply accepted_cost_pbbytes_is_body in parsed.
    subst scalar.
    unfold Projection.bytes_child_binds.
    rewrite value.
    unfold Core.source_summary_body, Core.source_summary_raw.
    rewrite value. reflexivity.
  - intros wrong. discriminate.
  - intros wrong. discriminate.
  - intros wrong. discriminate.
  - intros _. unfold Projection.cost_atom_payload_corresponds.
    destruct (cost_tree_child_shape program subject child accepted) as
      [[raw [scalar [payload [value leaf]]]]|
       [raw [values [payload value]]]].
    + rewrite value.
      rewrite (scalar_cost_summary_raw child raw scalar value).
      exact payload.
    + rewrite value.
      rewrite (message_cost_summary_raw child raw values value).
      exact payload.
Qed.

Lemma generated_cost_bound_child_corresponds :
  forall program subject child descriptor,
    Schema.lookup_projected_program 11 [] [] = Some program ->
    Cost.source_cost_tree (Some program) subject = Some child ->
    Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child) = Some descriptor ->
    Admission.source_child_matches_descriptor descriptor child = true ->
    Projection.cost_child_corresponds 11 child.
Proof.
  intros program subject child descriptor found accepted resolved matched.
  unfold Projection.cost_child_corresponds.
  repeat split.
  - intros wrong. discriminate.
  - intros _ selected.
    pose proof (generated_cost_bound_scalar_descriptor program
      descriptor found ltac:(rewrite <- selected; exact resolved))
      as scalar_descriptor.
    destruct (descriptor_scalar_cost_child_has_leaf program subject child
      descriptor accepted matched scalar_descriptor)
      as [raw [scalar [value [leaf tag]]]].
    pose proof (Decoder.generated_program_has_requested_family
      11 program found) as family.
    assert (contract : Admission.source_scalar_contract_for 11
      (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract Data.PBSint32 true)).
    { rewrite tag, selected. exact CostDecoder.generated_cost_bound_i32_contract. }
    destruct (cost_scalar_leaf_decodes_declared_kind 11 program raw
      Data.PBSint32 true child family contract leaf)
      as [decoded [source_value parsed]].
    apply accepted_cost_pbsint32_shape in parsed as [number shape].
    subst decoded.
    apply (CostDecoder.generated_cost_bound_i32_child_binds
      program raw child number found).
    + rewrite tag. exact selected.
    + exact leaf.
    + exact source_value.
  - intros wrong. discriminate.
  - intros wrong. discriminate.
  - intros wrong. discriminate.
Qed.

Lemma generated_cost_region_child_corresponds :
  forall program subject child descriptor,
    Schema.lookup_projected_program 12 [] [] = Some program ->
    Cost.source_cost_tree (Some program) subject = Some child ->
    Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child) = Some descriptor ->
    Admission.source_child_matches_descriptor descriptor child = true ->
    Projection.cost_child_corresponds 12 child.
Proof.
  intros program subject child descriptor found accepted resolved matched.
  unfold Projection.cost_child_corresponds.
  repeat split.
  - intros wrong. discriminate.
  - intros wrong. discriminate.
  - intros _ selected.
    assert (member : In (Core.source_summary_tag child) [1; 7])
      by (destruct selected as [selected|selected];
          rewrite selected; cbn; auto).
    pose proof (generated_cost_region_u32_descriptor program
      (Core.source_summary_tag child) descriptor found member resolved)
      as scalar_descriptor.
    destruct (descriptor_scalar_cost_child_has_leaf program subject child
      descriptor accepted matched scalar_descriptor)
      as [raw [scalar [value [leaf tag]]]].
    pose proof (Decoder.generated_program_has_requested_family
      12 program found) as family.
    assert (contract : Admission.source_scalar_contract_for 12
      (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract Data.PBUint32 true)).
    { rewrite tag. apply CostDecoder.generated_cost_region_u32_contract.
      exact member. }
    destruct (cost_scalar_leaf_decodes_declared_kind 12 program raw
      Data.PBUint32 true child family contract leaf)
      as [decoded [source_value parsed]].
    apply accepted_cost_pbuint32_shape in parsed as [number shape].
    subst decoded.
    apply (CostDecoder.generated_cost_region_u32_child_binds
      program raw child number found).
    + rewrite tag. exact member.
    + exact leaf.
    + exact source_value.
  - intros _ selected.
    assert (member : In (Core.source_summary_tag child) [2; 3; 4; 5])
      by (destruct selected as
          [selected|[selected|[selected|selected]]];
          rewrite selected; cbn; auto).
    pose proof (generated_cost_region_u64_descriptor program
      (Core.source_summary_tag child) descriptor found member resolved)
      as scalar_descriptor.
    destruct (descriptor_scalar_cost_child_has_leaf program subject child
      descriptor accepted matched scalar_descriptor)
      as [raw [scalar [value [leaf tag]]]].
    pose proof (Decoder.generated_program_has_requested_family
      12 program found) as family.
    assert (contract : Admission.source_scalar_contract_for 12
      (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract Data.PBUint64 true)).
    { rewrite tag. apply CostDecoder.generated_cost_region_u64_contract.
      exact member. }
    destruct (cost_scalar_leaf_decodes_declared_kind 12 program raw
      Data.PBUint64 true child family contract leaf)
      as [decoded [source_value parsed]].
    apply accepted_cost_pbuint64_shape in parsed as [number shape].
    subst decoded.
    apply (CostDecoder.generated_cost_region_u64_child_binds
      program raw child number found).
    + rewrite tag. exact member.
    + exact leaf.
    + exact source_value.
  - intros wrong. discriminate.
Qed.

Lemma generated_cost_child_corresponds_from_tree :
  forall family program subject child descriptor,
    In family [9; 10; 11; 12; 13] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Cost.source_cost_tree (Some program) subject = Some child ->
    Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child) = Some descriptor ->
    Admission.source_child_matches_descriptor descriptor child = true ->
    Projection.cost_child_corresponds family child.
Proof.
  intros family program subject child descriptor member found accepted
    resolved matched.
  destruct member as [member|[member|[member|[member|[member|[]]]]]];
    subst family.
  - unfold Projection.cost_child_corresponds.
    repeat split; intros wrong; discriminate.
  - exact (generated_cost_atom_child_corresponds program subject child
      descriptor found accepted resolved matched).
  - exact (generated_cost_bound_child_corresponds program subject child
      descriptor found accepted resolved matched).
  - exact (generated_cost_region_child_corresponds program subject child
      descriptor found accepted resolved matched).
  - unfold Projection.cost_child_corresponds.
    repeat split; intros wrong; discriminate.
Qed.

Theorem generated_cost_forest_children_correspond :
  forall family program subjects children,
    In family [9; 10; 11; 12; 13] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Cost.source_cost_forest program subjects = Some children ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    Forall (Projection.cost_child_corresponds family) children.
Proof.
  intros family program subjects children member found provenance valid.
  pose proof (cost_forest_children_have_source_trees program
    subjects children provenance) as origins.
  unfold Admission.source_scan_children in valid.
  pose proof (valid_cost_scan_matches_each_child program
    Admission.source_empty_scan children valid) as descriptors.
  apply Forall_forall. intros child present.
  pose proof (proj1 (Forall_forall _ _) origins child present)
    as [subject accepted].
  pose proof (proj1 (Forall_forall _ _) descriptors child present)
    as [descriptor [resolved matched]].
  exact (generated_cost_child_corresponds_from_tree family program
    subject child descriptor member found accepted resolved matched).
Qed.

Lemma selected_cost_program_is_generated :
  forall raw family program,
    Cost.cost_program raw = Some (family, program) ->
    In family [9; 10; 11; 12; 13] /\
    Schema.lookup_projected_program family [] [] = Some program.
Proof.
  intros raw family program selected.
  unfold Cost.cost_program in selected.
  destruct (Schema.raw_payload raw); try discriminate.
  destruct (Schema.raw_program_type_id raw) as [observed|];
    [|discriminate].
  destruct (Cost.cost_family observed) eqn:in_range;
    [|discriminate].
  destruct (Schema.lookup_projected_program observed [] [])
    as [projected|] eqn:found; [|discriminate].
  inversion selected; subst family program.
  unfold Cost.cost_family in in_range.
  apply andb_true_iff in in_range as [lower upper].
  apply Nat.leb_le in lower. apply Nat.leb_le in upper.
  split; [cbn; lia|exact found].
Qed.

Theorem accepted_cost_message_children_correspond :
  forall raw family program subjects children summary,
    Cost.cost_program raw = Some (family, program) ->
    Cost.source_cost_forest program subjects = Some children ->
    Cost.source_cost_message raw family program children = Some summary ->
    Forall (Projection.cost_child_corresponds family) children.
Proof.
  intros raw family program subjects children summary selected provenance
    accepted.
  destruct (selected_cost_program_is_generated raw family program selected)
    as [member found].
  unfold Cost.source_cost_message in accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Admission.source_scan_valid
        (Admission.source_scan_children program children) &&
      Admission.source_required_pathmaps_present program
        (Admission.source_scan_children program children) &&
      (Schema.program_family program =? family))
    eqn:guard; [|discriminate].
  destruct (Cost.cost_family_score family children) as [score|];
    [|discriminate].
  repeat rewrite andb_true_iff in guard.
  assert (valid : Admission.source_scan_valid
    (Admission.source_scan_children program children) = true) by tauto.
  exact (generated_cost_forest_children_correspond family program subjects
    children member found provenance valid).
Qed.

Print Assumptions valid_cost_scan_matches_each_child.
Print Assumptions cost_forest_children_have_source_trees.
Print Assumptions cost_tree_child_shape.
Print Assumptions descriptor_scalar_cost_child_has_leaf.
Print Assumptions cost_scalar_leaf_decodes_declared_kind.
Print Assumptions accepted_cost_pbbytes_is_body.
Print Assumptions generated_cost_atom_bytes_descriptor.
Print Assumptions generated_cost_bound_scalar_descriptor.
Print Assumptions generated_cost_region_u32_descriptor.
Print Assumptions generated_cost_region_u64_descriptor.
Print Assumptions generated_cost_atom_child_corresponds.
Print Assumptions generated_cost_bound_child_corresponds.
Print Assumptions generated_cost_region_child_corresponds.
Print Assumptions generated_cost_forest_children_correspond.
Print Assumptions accepted_cost_message_children_correspond.
