From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleCost RPB3IndependentOracleProjection
  RPB3IndependentOracleRecursive RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessScalarDecoderBridge
  RPB3IndependentOracleCostScalarDecoderBridge
  RPB3IndependentOracleCostChildCorrespondence
  RPB3IndependentOracleCostVerdictBridge
  RPB3IndependentOracleProcessChildProvenance.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Cost := RPB3IndependentOracleCost.
Module Oracle := RPB3IndependentOracleRecursive.
Module Projection := RPB3IndependentOracleProjection.
Module Decoder := RPB3IndependentOracleProcessScalarDecoderBridge.
Module CostDecoder := RPB3IndependentOracleCostScalarDecoderBridge.
Module CostChildren := RPB3IndependentOracleCostChildCorrespondence.
Module CostVerdict := RPB3IndependentOracleCostVerdictBridge.
Module ProcessChild := RPB3IndependentOracleProcessChildProvenance.
Module RecursiveProjection := RPB3IndependentOracleRecursiveProjection.

(** Bridge from the oracle's message step to the Cost forest results.

    [Oracle.source_evaluate_message_step] evaluates a Cost message's decoded
    source nodes with [Oracle.source_evaluate_node].  The forest theorem
    [CostChildren.generated_cost_forest_children_correspond] covers children
    produced by [Cost.source_cost_forest], and
    [CostVerdict.cost_family_score_projects_totally_for_actual_children]
    requires, for any list of children, that the family correspondence holds
    and that every child is plain.  This theory proves those two facts for the
    oracle's children: the family correspondence holds and every child is a
    scalar leaf or a typed message.

    The proof replaces the Cost forest provenance with three facts about the
    evaluator:

    - the scalar branch of [Oracle.source_evaluate_node] is definitionally the
      leaf [Cost.source_scalar_leaf], so a scalar child carries the same leaf
      fact the forest proof uses;
    - the message branch yields a typed message by the evaluator's own
      definition, for every fuel;
    - the auxiliary branches are unreachable for Cost families, because the
      generated table declares no string-map or path-map field for families 9
      through 13.  This is the finite certificate
      [finite_cost_programs_have_only_plain_fields].

    An oracle message child cannot in general be rebuilt as a
    [Cost.source_cost_tree] subject: it is built by
    [Oracle.source_message_summary], whose score may be absent, whereas
    [Cost.source_cost_message] accepts only with a present family score.  The
    forest proofs use only the shape, leaf and descriptor facts, so the family
    lemmas below are stated over the shape predicate
    [oracle_cost_child_shape] instead of over cost trees. *)

(** The conclusion of [CostChildren.cost_tree_child_shape], stated without a
    cost tree, so that it can be established for evaluator children. *)
Definition oracle_cost_child_shape (program : Schema.generated_program)
    (child : Core.source_exact_summary) : Prop :=
  (exists raw scalar,
    Schema.raw_payload raw = Schema.Scalar /\
    Core.source_summary_value child = Core.TypedSourceScalar raw scalar /\
    Cost.source_scalar_leaf program raw = Some child) \/
  (exists raw values,
    Schema.raw_payload raw = Schema.Message /\
    Core.source_summary_value child = Core.TypedSourceMessage raw values).

(** Finite certificate over the generated table: the projected programs for
    families 9 through 13 exist and declare only scalar and message fields. *)
Definition generated_field_is_plain (field : Schema.field_descriptor) : bool :=
  match Schema.descriptor_payload field with
  | Schema.Scalar | Schema.Message => true
  | Schema.StringMap | Schema.PathMap => false
  end.

Definition cost_family_fields_are_plain (family : nat) : bool :=
  match Schema.lookup_projected_program family [] [] with
  | Some program =>
      forallb generated_field_is_plain (Schema.program_fields program)
  | None => false
  end.

Definition cost_programs_have_only_plain_fields : bool :=
  forallb cost_family_fields_are_plain [9; 10; 11; 12; 13].

Theorem finite_cost_programs_have_only_plain_fields :
  cost_programs_have_only_plain_fields = true.
Proof. vm_compute. reflexivity. Qed.

Lemma generated_cost_field_is_plain :
  forall family program field,
    In family [9; 10; 11; 12; 13] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    In field (Schema.program_fields program) ->
    generated_field_is_plain field = true.
Proof.
  intros family program field member found field_member.
  pose proof finite_cost_programs_have_only_plain_fields as certificate.
  unfold cost_programs_have_only_plain_fields in certificate.
  rewrite forallb_forall in certificate.
  specialize (certificate family member).
  unfold cost_family_fields_are_plain in certificate.
  rewrite found in certificate.
  change (forallb generated_field_is_plain
    (Schema.program_fields program) = true) in certificate.
  rewrite forallb_forall in certificate.
  exact (certificate field field_member).
Qed.

(** A descriptor selected for a span is a member of the program's fields. *)
Lemma source_descriptor_field_is_member :
  forall program span field,
    Source.source_descriptor program span = Some field ->
    In field (Schema.program_fields program).
Proof.
  intros program span field selected.
  unfold Source.source_descriptor in selected.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Source.Wire.span_number span)) as [candidate|] eqn:resolved;
    [|discriminate].
  destruct (Schema.wire_eqb (Source.wire_kind_of_span span)
      (Schema.descriptor_wire candidate)); [|discriminate].
  inversion selected; subst candidate.
  eapply ProcessChild.resolved_generated_field_is_member; exact resolved.
Qed.

(** The auxiliary branch of [Oracle.source_evaluate_node] only succeeds for
    string-map and path-map descriptors. *)
Lemma evaluated_auxiliary_node_has_auxiliary_payload :
  forall recurse hash_fn bytes program span field child,
    Oracle.source_evaluate_node recurse hash_fn bytes program
      (Source.SourceAuxiliary span field) = Some child ->
    Schema.descriptor_payload field = Schema.StringMap \/
    Schema.descriptor_payload field = Schema.PathMap.
Proof.
  intros recurse hash_fn bytes program span field child evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source bytes
      (Source.SourceAuxiliary span field)) as [raw|];
    [|discriminate evaluated].
  destruct (Schema.descriptor_payload field) eqn:payload;
    [ discriminate evaluated
    | discriminate evaluated
    | left; reflexivity
    | right; reflexivity ].
Qed.

(** Every evaluator child of a Cost-family node has the plain Cost shape. *)
Lemma evaluated_cost_node_has_plain_shape :
  forall fuel hash_fn family program body node child,
    In family [9; 10; 11; 12; 13] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.source_descriptor program (Source.source_span node) =
      Some (Source.source_field node) ->
    Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program node =
      Some child ->
    oracle_cost_child_shape program child.
Proof.
  intros fuel hash_fn family program body node child member found selected
    evaluated.
  unfold oracle_cost_child_shape.
  destruct node as [span field|span field nested|span field].
  - left.
    destruct (ProcessChild.accepted_source_scalar_node_has_typed_value
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program
      span field child evaluated)
      as [raw [scalar [converted [value payload]]]].
    exists raw, scalar.
    split; [exact payload|split; [exact value|]].
    (* The scalar branch of the node evaluator is the Cost leaf, by unfolding
       the two definitions. *)
    unfold Oracle.source_evaluate_node in evaluated.
    rewrite converted in evaluated.
    exact evaluated.
  - right.
    destruct (ProcessChild.accepted_source_message_node_has_typed_value
      fuel hash_fn body program span field nested child evaluated)
      as [raw [values [_ [value payload]]]].
    exists raw, values.
    split; assumption.
  - exfalso.
    cbn [Source.source_span Source.source_field] in selected.
    pose proof (source_descriptor_field_is_member program span field selected)
      as field_member.
    pose proof (generated_cost_field_is_plain family program field member
      found field_member) as plain.
    unfold generated_field_is_plain in plain.
    destruct (evaluated_auxiliary_node_has_auxiliary_payload
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program span
      field child evaluated) as [string_map|path_map].
    + rewrite string_map in plain. discriminate plain.
    + rewrite path_map in plain. discriminate plain.
Qed.

(** The shape form of [CostChildren.descriptor_scalar_cost_child_has_leaf]. *)
Lemma descriptor_scalar_oracle_child_has_leaf :
  forall program child descriptor,
    oracle_cost_child_shape program child ->
    Admission.source_child_matches_descriptor descriptor child = true ->
    Schema.descriptor_payload descriptor = Schema.Scalar ->
    exists raw scalar,
      Core.source_summary_value child =
        Core.TypedSourceScalar raw scalar /\
      Cost.source_scalar_leaf program raw = Some child /\
      Schema.raw_tag raw = Core.source_summary_tag child.
Proof.
  intros program child descriptor child_shape matched scalar_descriptor.
  pose proof (CostChildren.matched_cost_child_payload descriptor child matched)
    as payload.
  rewrite scalar_descriptor in payload.
  destruct child_shape as
    [[raw [scalar [_ [value leaf]]]]|
     [raw [values [message_payload value]]]].
  - exists raw, scalar. repeat split; try assumption.
    unfold Core.source_summary_tag.
    now rewrite (CostChildren.scalar_cost_summary_raw child raw scalar value).
  - rewrite (CostChildren.message_cost_summary_raw child raw values value)
      in payload.
    rewrite message_payload in payload. discriminate.
Qed.

(** The shape form of [CostChildren.generated_cost_atom_child_corresponds]. *)
Lemma generated_oracle_atom_child_corresponds :
  forall program child descriptor,
    Schema.lookup_projected_program 10 [] [] = Some program ->
    oracle_cost_child_shape program child ->
    Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child) = Some descriptor ->
    Admission.source_child_matches_descriptor descriptor child = true ->
    Projection.cost_child_corresponds 10 child.
Proof.
  intros program child descriptor found child_shape resolved matched.
  unfold Projection.cost_child_corresponds.
  repeat split.
  - intros _ selected.
    assert (member : In (Core.source_summary_tag child) [1; 2; 4])
      by (destruct selected as [selected|[selected|selected]];
          rewrite selected; cbn; auto).
    pose proof (CostChildren.generated_cost_atom_bytes_descriptor program
      (Core.source_summary_tag child) descriptor found member resolved)
      as scalar_descriptor.
    destruct (descriptor_scalar_oracle_child_has_leaf program child
      descriptor child_shape matched scalar_descriptor)
      as [raw [scalar [value [leaf tag]]]].
    pose proof (Decoder.generated_program_has_requested_family
      10 program found) as family.
    assert (contract : Admission.source_scalar_contract_for 10
      (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract Data.PBBytes false)).
    { rewrite tag. apply CostChildren.generated_cost_atom_bytes_contract.
      exact member. }
    destruct (CostChildren.cost_scalar_leaf_decodes_declared_kind 10 program
      raw Data.PBBytes false child family contract leaf)
      as [decoded [source_value parsed]].
    rewrite value in source_value.
    inversion source_value; subst decoded.
    apply CostChildren.accepted_cost_pbbytes_is_body in parsed.
    subst scalar.
    unfold Projection.bytes_child_binds.
    rewrite value.
    unfold Core.source_summary_body, Core.source_summary_raw.
    rewrite value. reflexivity.
  - intros wrong. discriminate.
  - intros wrong. discriminate.
  - intros wrong. discriminate.
  - intros _. unfold Projection.cost_atom_payload_corresponds.
    destruct child_shape as
      [[raw [scalar [payload [value leaf]]]]|
       [raw [values [payload value]]]].
    + rewrite value.
      rewrite (CostChildren.scalar_cost_summary_raw child raw scalar value).
      exact payload.
    + rewrite value.
      rewrite (CostChildren.message_cost_summary_raw child raw values value).
      exact payload.
Qed.

(** The shape form of [CostChildren.generated_cost_bound_child_corresponds]. *)
Lemma generated_oracle_bound_child_corresponds :
  forall program child descriptor,
    Schema.lookup_projected_program 11 [] [] = Some program ->
    oracle_cost_child_shape program child ->
    Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child) = Some descriptor ->
    Admission.source_child_matches_descriptor descriptor child = true ->
    Projection.cost_child_corresponds 11 child.
Proof.
  intros program child descriptor found child_shape resolved matched.
  unfold Projection.cost_child_corresponds.
  repeat split.
  - intros wrong. discriminate.
  - intros _ selected.
    pose proof (CostChildren.generated_cost_bound_scalar_descriptor program
      descriptor found ltac:(rewrite <- selected; exact resolved))
      as scalar_descriptor.
    destruct (descriptor_scalar_oracle_child_has_leaf program child
      descriptor child_shape matched scalar_descriptor)
      as [raw [scalar [value [leaf tag]]]].
    pose proof (Decoder.generated_program_has_requested_family
      11 program found) as family.
    assert (contract : Admission.source_scalar_contract_for 11
      (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract Data.PBSint32 true)).
    { rewrite tag, selected.
      exact CostDecoder.generated_cost_bound_i32_contract. }
    destruct (CostChildren.cost_scalar_leaf_decodes_declared_kind 11 program
      raw Data.PBSint32 true child family contract leaf)
      as [decoded [source_value parsed]].
    apply CostChildren.accepted_cost_pbsint32_shape in parsed
      as [number number_shape].
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

(** The shape form of [CostChildren.generated_cost_region_child_corresponds]. *)
Lemma generated_oracle_region_child_corresponds :
  forall program child descriptor,
    Schema.lookup_projected_program 12 [] [] = Some program ->
    oracle_cost_child_shape program child ->
    Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child) = Some descriptor ->
    Admission.source_child_matches_descriptor descriptor child = true ->
    Projection.cost_child_corresponds 12 child.
Proof.
  intros program child descriptor found child_shape resolved matched.
  unfold Projection.cost_child_corresponds.
  repeat split.
  - intros wrong. discriminate.
  - intros wrong. discriminate.
  - intros _ selected.
    assert (member : In (Core.source_summary_tag child) [1; 7])
      by (destruct selected as [selected|selected];
          rewrite selected; cbn; auto).
    pose proof (CostChildren.generated_cost_region_u32_descriptor program
      (Core.source_summary_tag child) descriptor found member resolved)
      as scalar_descriptor.
    destruct (descriptor_scalar_oracle_child_has_leaf program child
      descriptor child_shape matched scalar_descriptor)
      as [raw [scalar [value [leaf tag]]]].
    pose proof (Decoder.generated_program_has_requested_family
      12 program found) as family.
    assert (contract : Admission.source_scalar_contract_for 12
      (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract Data.PBUint32 true)).
    { rewrite tag. apply CostDecoder.generated_cost_region_u32_contract.
      exact member. }
    destruct (CostChildren.cost_scalar_leaf_decodes_declared_kind 12 program
      raw Data.PBUint32 true child family contract leaf)
      as [decoded [source_value parsed]].
    apply CostChildren.accepted_cost_pbuint32_shape in parsed
      as [number number_shape].
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
    pose proof (CostChildren.generated_cost_region_u64_descriptor program
      (Core.source_summary_tag child) descriptor found member resolved)
      as scalar_descriptor.
    destruct (descriptor_scalar_oracle_child_has_leaf program child
      descriptor child_shape matched scalar_descriptor)
      as [raw [scalar [value [leaf tag]]]].
    pose proof (Decoder.generated_program_has_requested_family
      12 program found) as family.
    assert (contract : Admission.source_scalar_contract_for 12
      (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract Data.PBUint64 true)).
    { rewrite tag. apply CostDecoder.generated_cost_region_u64_contract.
      exact member. }
    destruct (CostChildren.cost_scalar_leaf_decodes_declared_kind 12 program
      raw Data.PBUint64 true child family contract leaf)
      as [decoded [source_value parsed]].
    apply CostChildren.accepted_cost_pbuint64_shape in parsed
      as [number number_shape].
    subst decoded.
    apply (CostDecoder.generated_cost_region_u64_child_binds
      program raw child number found).
    + rewrite tag. exact member.
    + exact leaf.
    + exact source_value.
  - intros wrong. discriminate.
Qed.

(** The shape form of [CostChildren.generated_cost_child_corresponds_from_tree].
    Families 9 and 13 impose no scalar obligation, so their correspondence is
    vacuous; families 10, 11 and 12 follow from the leaf facts above. *)
Lemma generated_oracle_child_corresponds :
  forall family program child descriptor,
    In family [9; 10; 11; 12; 13] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    oracle_cost_child_shape program child ->
    Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child) = Some descriptor ->
    Admission.source_child_matches_descriptor descriptor child = true ->
    Projection.cost_child_corresponds family child.
Proof.
  intros family program child descriptor member found child_shape resolved
    matched.
  destruct member as [member|[member|[member|[member|[member|[]]]]]];
    subst family.
  - unfold Projection.cost_child_corresponds.
    repeat split; intros wrong; discriminate.
  - exact (generated_oracle_atom_child_corresponds program child descriptor
      found child_shape resolved matched).
  - exact (generated_oracle_bound_child_corresponds program child descriptor
      found child_shape resolved matched).
  - exact (generated_oracle_region_child_corresponds program child descriptor
      found child_shape resolved matched).
  - unfold Projection.cost_child_corresponds.
    repeat split; intros wrong; discriminate.
Qed.

(** A right-hand predicate implied by a positional relation holds throughout
    the right list. *)
Lemma forall2_right_satisfies :
  forall (A B : Type) (relation : A -> B -> Prop) (satisfied : B -> Prop)
      nodes results,
    Forall2 relation nodes results ->
    (forall node result, relation node result -> satisfied result) ->
    Forall satisfied results.
Proof.
  intros A B relation satisfied nodes results aligned implied.
  induction aligned as [|node result nodes results related rest IH];
    constructor.
  - exact (implied node result related).
  - exact IH.
Qed.

(** The oracle's children of a generated Cost message correspond to the
    family and are plain, so
    [CostVerdict.cost_family_score_projects_totally_for_actual_children] applies
    to them.  [fuel] is arbitrary: a child that exists at all exists by a
    successful evaluation of its node, whatever fuel the recursion was given. *)
Theorem generated_cost_message_children_correspond :
  forall fuel hash_fn family program body nodes children,
    In family [9; 10; 11; 12; 13] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.decode_region (S (length body)) body program 0 (length body) = Some nodes ->
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program) nodes)
      = Some children ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    Forall (Projection.cost_child_corresponds family) children /\
    Forall CostVerdict.cost_plain_child children.
Proof.
  intros fuel hash_fn family program body nodes children member found decoded
    sequenced valid.
  assert (shapes : Forall (oracle_cost_child_shape program) children).
  { pose proof (RecursiveProjection.selected_source_evaluator_outputs_are_positional
      Source.source_tree Core.source_exact_summary
      (Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program)
      nodes children sequenced) as positional.
    pose proof (Source.decoded_region_nodes_match_generated_source
      (S (length body)) body program 0 (length body) nodes decoded)
      as generated.
    pose proof (ProcessChild.generated_selected_children_are_aligned program
      (Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program)
      nodes children generated positional) as aligned.
    apply (forall2_right_satisfies _ _ _ _ nodes children aligned).
    intros node child [matched evaluated].
    destruct matched as [selected _].
    exact (evaluated_cost_node_has_plain_shape fuel hash_fn family program
      body node child member found selected evaluated). }
  unfold Admission.source_scan_children in valid.
  pose proof (CostChildren.valid_cost_scan_matches_each_child program
    Admission.source_empty_scan children valid) as descriptors.
  split.
  - apply Forall_forall. intros child present.
    pose proof (proj1 (Forall_forall _ _) shapes child present) as child_shape.
    destruct (proj1 (Forall_forall _ _) descriptors child present)
      as [descriptor [resolved matched]].
    exact (generated_oracle_child_corresponds family program child descriptor
      member found child_shape resolved matched).
  - apply Forall_forall. intros child present.
    destruct (proj1 (Forall_forall _ _) shapes child present) as
      [[raw [scalar [_ [value _]]]]|
       [raw [values [_ value]]]].
    + unfold CostVerdict.cost_plain_child. now rewrite value.
    + unfold CostVerdict.cost_plain_child. now rewrite value.
Qed.

Print Assumptions finite_cost_programs_have_only_plain_fields.
Print Assumptions generated_cost_field_is_plain.
Print Assumptions source_descriptor_field_is_member.
Print Assumptions evaluated_auxiliary_node_has_auxiliary_payload.
Print Assumptions evaluated_cost_node_has_plain_shape.
Print Assumptions descriptor_scalar_oracle_child_has_leaf.
Print Assumptions generated_oracle_atom_child_corresponds.
Print Assumptions generated_oracle_bound_child_corresponds.
Print Assumptions generated_oracle_region_child_corresponds.
Print Assumptions generated_oracle_child_corresponds.
Print Assumptions forall2_right_satisfies.
Print Assumptions generated_cost_message_children_correspond.
