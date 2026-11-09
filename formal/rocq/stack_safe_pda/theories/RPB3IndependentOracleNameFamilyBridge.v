From Stdlib Require Import List Bool Arith NArith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3SourceScalarReduction RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleName
  RPB3IndependentOracleProjection RPB3IndependentOracleNameProjection
  RPB3IndependentOracleNameDecoderBridge RPB3IndependentOracleRecursive
  RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Target := RPB3SemanticReducer.
Module Scalar := RPB3SourceScalarReduction.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Name := RPB3IndependentOracleName.
Module Projection := RPB3IndependentOracleProjection.
Module NameProjection := RPB3IndependentOracleNameProjection.
Module NameBridge := RPB3IndependentOracleNameDecoderBridge.
Module Oracle := RPB3IndependentOracleRecursive.
Module RecursiveProjection := RPB3IndependentOracleRecursiveProjection.
Module ProcessChild := RPB3IndependentOracleProcessChildProvenance.

(** A decoded node and the child the oracle evaluated from it.  The first
    conjunct is the descriptor the decoder selected; the second is the
    node's evaluation, in the shape produced by
    [ProcessChild.generated_selected_children_are_aligned]. *)
Definition aligned_child (fuel : nat) (hash_fn : list nat -> nat)
    (body : list nat) (program : Schema.generated_program)
    (node : Source.source_tree) (child : Core.source_exact_summary) : Prop :=
  Source.source_node_matches_generated_descriptor program node /\
  Oracle.source_evaluate_node
    (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program node =
    Some child.

Lemma wire_eqb_true_same :
  forall wire other, Schema.wire_eqb wire other = true -> wire = other.
Proof. intros [] [] same; try discriminate same; reflexivity. Qed.

Lemma payload_eqb_true_same :
  forall payload other,
    Schema.payload_eqb payload other = true -> payload = other.
Proof. intros [] [] same; try discriminate same; reflexivity. Qed.

Lemma existsb_eqb_member :
  forall tag tags, existsb (Nat.eqb tag) tags = true -> In tag tags.
Proof.
  intros tag tags found.
  apply existsb_exists in found as [candidate [member same]].
  apply Nat.eqb_eq in same.
  subst candidate.
  exact member.
Qed.

Lemma forall2_right_forall :
  forall (A B : Type) (relation : A -> B -> Prop) (predicate : B -> Prop)
      lefts rights,
    (forall left_value right_value,
      relation left_value right_value -> predicate right_value) ->
    Forall2 relation lefts rights -> Forall predicate rights.
Proof.
  intros A B relation predicate lefts rights implied related.
  induction related as [|left_value right_value lefts' rights' head rest IH].
  - constructor.
  - constructor.
    + exact (implied left_value right_value head).
    + exact IH.
Qed.

Lemma rev_empty_is_empty :
  forall (A : Type) (values : list A), rev values = [] -> values = [].
Proof.
  intros A values reversed.
  destruct values as [|head tail]; [reflexivity|].
  exfalso.
  cbn [rev] in reversed.
  destruct (rev tail) as [|first rest]; discriminate reversed.
Qed.

Lemma source_descriptor_field_member :
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

Lemma looked_up_program_family :
  forall type_id program,
    Schema.lookup_projected_program type_id [] [] = Some program ->
    Schema.program_family program = type_id.
Proof.
  intros type_id program found.
  unfold Schema.lookup_projected_program in found.
  destruct (find (fun row => Data.program_type_id row =? type_id)
      Data.generated_programs) as [row|] eqn:selected;
    [|discriminate].
  inversion found; subst program.
  apply find_some in selected as [_ family].
  cbn [Schema.program_family Schema.project_generated_program].
  now apply Nat.eqb_eq in family.
Qed.

(** Every summary the node evaluator returns carries the raw field computed
    for that node, including the two auxiliary-map branches. *)
Lemma evaluated_node_retains_source_raw :
  forall fuel hash_fn body program node child,
    Oracle.source_evaluate_node
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program node =
      Some child ->
    Source.raw_field_of_source body node =
      Some (Core.source_summary_raw child).
Proof.
  intros fuel hash_fn body program node child evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source body node) as [child_raw|] eqn:converted;
    [|discriminate].
  destruct node as [span field|span field nested|span field].
  - destruct (Admission.source_typed_scalar program child_raw)
      as [value|] eqn:typed; [|discriminate].
    inversion evaluated; subst child.
    change (Some child_raw = Some (Core.source_value_raw value)).
    rewrite (RecursiveProjection.accepted_source_typed_scalar_retains_raw
      program child_raw value typed).
    reflexivity.
  - rewrite (RecursiveProjection.source_evaluate_message_retains_raw
      fuel hash_fn child_raw child evaluated).
    reflexivity.
  - destruct (Schema.descriptor_payload field) eqn:field_kind;
      try discriminate evaluated;
      repeat (match type of evaluated with
        | (match ?scrutinee with _ => _ end) = _ => destruct scrutinee
        end; try discriminate evaluated);
      inversion evaluated; subst child;
      reflexivity.
Qed.

(** The summary of a scalar node, with its decoded value. *)
Lemma evaluated_scalar_node_shape :
  forall recurse hash_fn body program span field child,
    Oracle.source_evaluate_node recurse hash_fn body program
      (Source.SourceScalar span field) = Some child ->
    exists raw scalar,
      Source.raw_field_of_source body (Source.SourceScalar span field) =
        Some raw /\
      Admission.source_scalar_value_of_raw program raw = Some scalar /\
      Admission.source_typed_scalar program raw =
        Some (Core.TypedSourceScalar raw scalar) /\
      child = Core.SourceExactSummary (Core.TypedSourceScalar raw scalar)
        (Core.SourceFamilyAnalysis None None false false false) None.
Proof.
  intros recurse hash_fn body program span field child evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source body (Source.SourceScalar span field))
    as [raw|] eqn:converted; [|discriminate].
  destruct (Admission.source_typed_scalar program raw)
    as [value|] eqn:typed; [|discriminate].
  inversion evaluated; subst child.
  assert (shape : exists scalar,
      Admission.source_scalar_value_of_raw program raw = Some scalar /\
      value = Core.TypedSourceScalar raw scalar).
  { unfold Admission.source_typed_scalar in typed.
    destruct (Admission.source_scalar_value_of_raw program raw)
      as [scalar|] eqn:decoded; [|discriminate].
    inversion typed; subst value.
    exists scalar.
    split; reflexivity. }
  destruct shape as [scalar [decoded same_value]].
  subst value.
  exists raw, scalar.
  split; [first [reflexivity|exact converted]|].
  split; [exact decoded|].
  split; [exact typed|].
  reflexivity.
Qed.

(** The independent scalar decoder is applied with the kind and default policy
    of the contract found for the raw field's tag. *)
Lemma scalar_value_decodes_with_contract :
  forall program raw scalar kind forbidden,
    Admission.source_scalar_value_of_raw program raw = Some scalar ->
    Admission.source_scalar_contract_for (Schema.program_family program)
      (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract kind forbidden) ->
    Core.independently_decode_scalar kind forbidden (Schema.raw_body raw) =
      Some scalar.
Proof.
  intros program raw scalar kind forbidden accepted contract.
  unfold Admission.source_scalar_value_of_raw in accepted.
  rewrite contract in accepted.
  destruct (Schema.raw_payload raw); try discriminate accepted.
  destruct (Schema.raw_program_type_id raw); [discriminate accepted|].
  destruct (Schema.resolve_tag (Schema.program_fields program)
    (Schema.raw_tag raw)) as [descriptor|]; [|discriminate accepted].
  cbn [Admission.source_contract_kind
    Admission.source_contract_default_forbidden] in accepted.
  destruct (Admission.source_scalar_wire kind) as [wire|];
    [|discriminate accepted].
  destruct (Admission.source_raw_bytes_well_formed raw &&
    Schema.wire_eqb (Schema.raw_wire raw) wire &&
    Schema.wire_eqb wire (Schema.descriptor_wire descriptor) &&
    Schema.payload_eqb (Schema.descriptor_payload descriptor) Schema.Scalar);
    [exact accepted|discriminate accepted].
Qed.

Lemma independent_pbbool_is_bool :
  forall forbidden body scalar,
    Core.independently_decode_scalar Data.PBBool forbidden body =
      Some scalar ->
    exists value, scalar = Core.SourceBool value.
Proof.
  intros forbidden body scalar accepted.
  cbn [Core.independently_decode_scalar] in accepted.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|discriminate accepted].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault in accepted.
  cbv beta iota zeta in accepted.
  destruct (negb forbidden || negb (N.eqb number 0));
    [|discriminate accepted].
  destruct (N.leb number 1); [|discriminate accepted].
  injection accepted as same_scalar.
  exists (N.eqb number 1).
  symmetry. exact same_scalar.
Qed.

(** With the default forbidden, an accepted [bool] is the one-byte encoding of
    [true]: this is what the target's body test observes. *)
Lemma independent_forbidden_bool_is_true_one_byte :
  forall body scalar,
    Core.independently_decode_scalar Data.PBBool true body = Some scalar ->
    scalar = Core.SourceBool true /\ body = [1].
Proof.
  intros body scalar accepted.
  cbn [Core.independently_decode_scalar] in accepted.
  destruct (Core.independently_decoded_u64 body) as [number|] eqn:decoded;
    [|discriminate accepted].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault in accepted.
  cbn [negb orb] in accepted.
  destruct (N.eqb number 0) eqn:zero; cbn [negb] in accepted;
    [discriminate accepted|].
  destruct (N.leb number 1) eqn:bounded; [|discriminate accepted].
  injection accepted as same_scalar.
  subst scalar.
  assert (nonzero : number <> 0%N).
  { intro same_zero. rewrite same_zero in zero. discriminate zero. }
  apply N.leb_le in bounded.
  assert (number_is_one : number = 1%N) by lia.
  subst number.
  split; [reflexivity|].
  apply Core.independently_decoded_u64_has_canonical_full_body in decoded
    as [natural [parsed [numeric _]]].
  assert (natural_is_one : natural = 1) by lia.
  subst natural.
  exact (NameBridge.small_full_varint_one_byte body 1 parsed ltac:(lia)).
Qed.

Lemma independent_forbidden_bytes_is_nonempty_body :
  forall body scalar,
    Core.independently_decode_scalar Data.PBBytes true body = Some scalar ->
    scalar = Core.SourceBlob body /\ length body <> 0.
Proof.
  intros body scalar accepted.
  cbn [Core.independently_decode_scalar] in accepted.
  unfold Core.source_nondefault in accepted.
  destruct (Core.bytes_in_range body && (negb true || negb (Nat.eqb (length body) 0)))
    eqn:guard; [|discriminate accepted].
  injection accepted as same_scalar.
  subst scalar.
  split; [reflexivity|].
  apply andb_true_iff in guard as [_ nondefault].
  cbn [negb orb] in nondefault.
  intro empty.
  rewrite empty in nondefault.
  cbn in nondefault.
  discriminate nondefault.
Qed.

Lemma target_forbidden_bytes_semantic :
  forall body,
    length body <> 0 ->
    Scalar.scalar_semantic_from_source (Scalar.ScalarRule 1 Data.PBBytes true) body =
      Some (Scalar.ScalarBytes body).
Proof.
  intros body nonempty.
  assert (valid : Scalar.scalar_body_valid (Scalar.ScalarRule 1 Data.PBBytes true) body = true).
  { unfold Scalar.scalar_body_valid.
    cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden negb orb].
    destruct (Nat.eqb (length body) 0) eqn:length_zero; [|reflexivity].
    exfalso.
    apply Nat.eqb_eq in length_zero.
    exact (nonempty length_zero). }
  unfold Scalar.scalar_semantic_from_source.
  rewrite valid.
  reflexivity.
Qed.

(** A region's spans are empty exactly when the region itself is empty. *)
Lemma reference_remaining_empty_region :
  forall fuel bytes region_end offset last ordered,
    Source.Wire.reference_remaining fuel bytes region_end offset last ordered =
      Source.Wire.Parsed [] ->
    offset = region_end.
Proof.
  intros fuel bytes region_end offset last ordered parsed.
  destruct fuel as [|smaller];
    cbn [Source.Wire.reference_remaining] in parsed;
    destruct (offset =? region_end) eqn:finished;
    try (apply Nat.eqb_eq; exact finished);
    destruct (Source.Wire.checked_field bytes region_end offset last ordered)
      as [field|]; try discriminate parsed.
  destruct (Source.Wire.reference_remaining smaller bytes region_end
    (Source.Wire.span_end field) (Source.Wire.span_number field) ordered);
    discriminate parsed.
Qed.

Lemma decode_source_fold_tracks_length :
  forall program decode_child spans initial final,
    fold_left (Source.decode_source_step program decode_child) spans
      (Some initial) = Some final ->
    length (Source.decode_reversed final) =
      length spans + length (Source.decode_reversed initial).
Proof.
  intros program decode_child spans.
  induction spans as [|span rest IH]; intros initial final folded.
  - cbn [fold_left] in folded.
    injection folded as same.
    subst final.
    reflexivity.
  - cbn [fold_left] in folded.
    destruct (Source.decode_source_step program decode_child
        (Some initial) span) as [next|] eqn:stepped.
    + rewrite (IH next final folded).
      unfold Source.decode_source_step in stepped.
      destruct (Source.source_descriptor program span) as [field|]
        eqn:resolved; [|discriminate stepped].
      destruct (Source.slot_admissible program initial field);
        [|discriminate stepped].
      destruct (Source.generated_source_node decode_child span field)
        as [node|] eqn:generated; [|discriminate stepped].
      inversion stepped; subst next.
      cbn [Source.decode_reversed length].
      lia.
    + rewrite (Source.fold_left_none_stays_none _ _
        (Source.decode_source_step program decode_child) rest
        (fun member => eq_refl)) in folded.
      discriminate folded.
Qed.

(** A decode returning no nodes consumed no bytes: each node comes from a span
    and each span advances the cursor. *)
Lemma decoded_empty_region_has_no_bytes :
  forall fuel bytes program start finish,
    Source.decode_region fuel bytes program start finish = Some [] ->
    start = finish.
Proof.
  intros [|smaller] bytes program start finish decoded;
    simpl in decoded; try discriminate decoded.
  destruct (Source.Wire.worklist_region bytes start finish false)
    as [spans| |] eqn:framed; try discriminate decoded.
  set (decode_child := fun (child_program : Schema.generated_program)
    (first last : nat) =>
      Source.decode_region smaller bytes child_program first last).
  change (option_map (fun state => rev (Source.decode_reversed state))
    (fold_left (Source.decode_source_step program decode_child) spans
      (Some (Source.DecodeAccumulator None [] []))) = Some []) in decoded.
  destruct (fold_left (Source.decode_source_step program decode_child) spans
    (Some (Source.DecodeAccumulator None [] []))) as [final|] eqn:folded;
    [|discriminate decoded].
  cbn [option_map] in decoded.
  injection decoded as empty_reversed.
  pose proof (decode_source_fold_tracks_length program decode_child spans
    (Source.DecodeAccumulator None [] []) final folded) as lengths.
  rewrite (rev_empty_is_empty _ _ empty_reversed) in lengths.
  cbn [length Source.decode_reversed] in lengths.
  assert (no_spans : spans = []).
  { destruct spans as [|span tail]; [reflexivity|].
    exfalso.
    cbn [length] in lengths.
    lia. }
  subst spans.
  assert (reference :
    Source.Wire.reference_region bytes start finish false =
      Source.Wire.Parsed []).
  { rewrite <- Source.Wire.worklist_and_reference_agree_on_every_finite_region.
    exact framed. }
  unfold Source.Wire.reference_region in reference.
  destruct (Source.Wire.region_ok bytes start finish);
    [|discriminate reference].
  cbv beta iota zeta in reference.
  exact (reference_remaining_empty_region _ _ _ _ _ _ reference).
Qed.

(** The score evidence of a family is the projection of its source score. *)
Lemma exact_or_unknown_of_projected :
  forall target source,
    target = option_map Core.project_score source ->
    Target.exact_or_unknown target = Projection.project_source_evidence source.
Proof.
  intros target source same.
  subst target.
  destruct source; reflexivity.
Qed.

(** ** The default-forbidden connective flags of families 31 and 32 *)

Definition connective_flag_child_true (tag : nat)
    (child : Core.source_exact_summary) : Prop :=
  Core.source_summary_tag child = tag ->
  Core.source_summary_value child =
    Core.TypedSourceScalar (Core.source_summary_raw child) (Core.SourceBool true) /\
  Schema.raw_payload (Core.source_summary_raw child) = Schema.Scalar /\
  Schema.raw_body (Core.source_summary_raw child) = [1].

Lemma scalar_true_or_absent_agrees_of_flag_children :
  forall tag children,
    Forall (connective_flag_child_true tag) children ->
    Target.scalar_true_or_absent tag
      (map Projection.project_source_summary children) =
    Name.source_bool_or_absent tag children.
Proof.
  intros tag children flags.
  rewrite Forall_forall in flags.
  unfold Target.scalar_true_or_absent, Name.source_bool_or_absent,
    Name.source_child_at.
  rewrite <- Projection.projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? tag) children)
    as [child|] eqn:found.
  - apply find_some in found as [member same_tag].
    apply Nat.eqb_eq in same_tag.
    destruct (flags child member same_tag) as [value [payload body]].
    cbn [option_map].
    change (Target.sem_payload (Projection.project_source_summary child))
      with (Schema.raw_payload (Core.source_summary_raw child)).
    change (Target.sem_body (Projection.project_source_summary child))
      with (Schema.raw_body (Core.source_summary_raw child)).
    rewrite payload, body, value.
    reflexivity.
  - reflexivity.
Qed.

(** The tag of a default-forbidden flag is a scalar field of the family's
    generated program. *)
Definition flag_tag_is_scalar (tag : nat)
    (field : Schema.field_descriptor) : bool :=
  negb (Schema.descriptor_tag field =? tag) ||
    Schema.payload_eqb (Schema.descriptor_payload field) Schema.Scalar.

Definition flag_tag_scalar_in_family (family tag : nat) : bool :=
  match Schema.lookup_projected_program family [] [] with
  | None => false
  | Some program =>
      forallb (flag_tag_is_scalar tag) (Schema.program_fields program)
  end.

Lemma epathmap_flag_tag_is_scalar : flag_tag_scalar_in_family 31 4 = true.
Proof. vm_compute. reflexivity. Qed.

Lemma ezipper_flag_tag_is_scalar : flag_tag_scalar_in_family 32 5 = true.
Proof. vm_compute. reflexivity. Qed.

Lemma epathmap_flag_contract :
  Admission.source_scalar_contract_for 31 4 =
    Some (Admission.SourceScalarContract Data.PBBool true).
Proof. vm_compute. reflexivity. Qed.

Lemma ezipper_flag_contract :
  Admission.source_scalar_contract_for 32 5 =
    Some (Admission.SourceScalarContract Data.PBBool true).
Proof. vm_compute. reflexivity. Qed.

Lemma generated_flag_tag_field_is_scalar :
  forall family tag program field,
    flag_tag_scalar_in_family family tag = true ->
    Schema.lookup_projected_program family [] [] = Some program ->
    In field (Schema.program_fields program) ->
    Schema.descriptor_tag field = tag ->
    Schema.descriptor_payload field = Schema.Scalar.
Proof.
  intros family tag program field certificate looked_up member same_tag.
  unfold flag_tag_scalar_in_family in certificate.
  rewrite looked_up in certificate.
  change (forallb (flag_tag_is_scalar tag) (Schema.program_fields program) =
    true) in certificate.
  rewrite forallb_forall in certificate.
  specialize (certificate field member).
  unfold flag_tag_is_scalar in certificate.
  rewrite same_tag, Nat.eqb_refl in certificate.
  destruct (Schema.descriptor_payload field);
    cbn in certificate; try discriminate certificate; reflexivity.
Qed.

(** A child of a default-forbidden flag tag can only be a decoded [true]. *)
Lemma forbidden_flag_child_is_true :
  forall fuel hash_fn body program family tag node child,
    Schema.lookup_projected_program family [] [] = Some program ->
    flag_tag_scalar_in_family family tag = true ->
    Admission.source_scalar_contract_for family tag =
      Some (Admission.SourceScalarContract Data.PBBool true) ->
    aligned_child fuel hash_fn body program node child ->
    connective_flag_child_true tag child.
Proof.
  intros fuel hash_fn body program family tag node child looked_up certificate
    contract [[selected _] evaluated] same_tag.
  pose proof (evaluated_node_retains_source_raw fuel hash_fn body program node
    child evaluated) as converted.
  destruct (Source.raw_field_of_source_preserves_wire_and_body body node
    (Core.source_summary_raw child) converted) as [raw_tag_is _].
  destruct (Source.resolved_source_descriptor_has_actual_tag_and_wire program
    (Source.source_span node) (Source.source_field node) selected)
    as [field_tag _].
  assert (field_scalar :
    Schema.descriptor_payload (Source.source_field node) = Schema.Scalar).
  { apply (generated_flag_tag_field_is_scalar family tag program
      (Source.source_field node) certificate looked_up).
    - exact (source_descriptor_field_member program (Source.source_span node)
        (Source.source_field node) selected).
    - rewrite field_tag, <- raw_tag_is. exact same_tag. }
  destruct node as [span field|span field nested|span field].
  - destruct (evaluated_scalar_node_shape _ hash_fn body program span field
      child evaluated) as [raw [scalar [same_raw [decoded [typed shape]]]]].
    assert (raw_is : Core.source_summary_raw child = raw)
      by (rewrite shape; reflexivity).
    assert (value_is : Core.source_summary_value child =
        Core.TypedSourceScalar raw scalar) by (rewrite shape; reflexivity).
    assert (tag_is : Schema.raw_tag raw = tag).
    { rewrite <- raw_is. exact same_tag. }
    assert (contract_at_tag :
      Admission.source_scalar_contract_for (Schema.program_family program)
        (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract Data.PBBool true)).
    { rewrite (looked_up_program_family family program looked_up), tag_is.
      exact contract. }
    destruct (independent_forbidden_bool_is_true_one_byte (Schema.raw_body raw)
      scalar (scalar_value_decodes_with_contract program raw scalar Data.PBBool
        true decoded contract_at_tag)) as [same_scalar body_one].
    subst scalar.
    destruct (ProcessChild.converted_scalar_raw_has_scalar_shape body span
      field raw same_raw) as [raw_payload _].
    rewrite raw_is, value_is.
    exact (conj eq_refl (conj raw_payload body_one)).
  - exfalso.
    cbn [Source.source_field] in field_scalar.
    unfold Source.raw_field_of_source in converted.
    cbn [Source.source_span Source.source_field] in converted.
    rewrite field_scalar in converted.
    discriminate converted.
  - exfalso.
    cbn [Source.source_field] in field_scalar.
    unfold Source.raw_field_of_source in converted.
    cbn [Source.source_span Source.source_field] in converted.
    rewrite field_scalar in converted.
    discriminate converted.
Qed.

Lemma flag_tag_children_agree :
  forall fuel hash_fn body program family tag nodes children,
    Schema.lookup_projected_program family [] [] = Some program ->
    flag_tag_scalar_in_family family tag = true ->
    Admission.source_scalar_contract_for family tag =
      Some (Admission.SourceScalarContract Data.PBBool true) ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Target.scalar_true_or_absent tag
      (map Projection.project_source_summary children) =
    Name.source_bool_or_absent tag children.
Proof.
  intros fuel hash_fn body program family tag nodes children looked_up
    certificate contract aligned.
  apply scalar_true_or_absent_agrees_of_flag_children.
  eapply forall2_right_forall; [|exact aligned].
  intros node child pair.
  exact (forbidden_flag_child_is_true fuel hash_fn body program family tag
    node child looked_up certificate contract pair).
Qed.

Lemma epathmap_family_projects :
  forall fuel hash_fn body program nodes children,
    Schema.lookup_projected_program 31 [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Target.score_evidence_for 31 (Some true)
      (map Projection.project_source_summary children) =
    Projection.project_source_evidence
      (Name.source_name_family_score 31 (Some true) children).
Proof.
  intros fuel hash_fn body program nodes children looked_up aligned.
  pose proof (flag_tag_children_agree fuel hash_fn body program 31 4 nodes
    children looked_up epathmap_flag_tag_is_scalar epathmap_flag_contract
    aligned) as bool_agreement.
  change (Target.exact_or_unknown (Target.exact_epathmap_score
      (map Projection.project_source_summary children)) =
    Projection.project_source_evidence (Name.source_epathmap_score children)).
  exact (exact_or_unknown_of_projected _ _
    (NameProjection.projected_epathmap_score children bool_agreement)).
Qed.

Lemma ezipper_family_projects :
  forall fuel hash_fn body program nodes children,
    Schema.lookup_projected_program 32 [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Target.score_evidence_for 32 (Some true)
      (map Projection.project_source_summary children) =
    Projection.project_source_evidence
      (Name.source_name_family_score 32 (Some true) children).
Proof.
  intros fuel hash_fn body program nodes children looked_up aligned.
  pose proof (flag_tag_children_agree fuel hash_fn body program 32 5 nodes
    children looked_up ezipper_flag_tag_is_scalar ezipper_flag_contract
    aligned) as bool_agreement.
  change (Target.exact_or_unknown (Target.exact_ezipper_score
      (map Projection.project_source_summary children)) =
    Projection.project_source_evidence (Name.source_ezipper_score children)).
  exact (exact_or_unknown_of_projected _ _
    (NameProjection.projected_ezipper_score children bool_agreement)).
Qed.

(** ** The connective family, 55 *)

(** Every field of the generated connective program is either a boolean
    varint arm among tags 5 through 9, or a length-delimited message arm. *)
Definition connective_field_is_classified
    (field : Schema.field_descriptor) : bool :=
  match Schema.descriptor_payload field with
  | Schema.Scalar =>
      Schema.wire_eqb (Schema.descriptor_wire field) Schema.Varint &&
      existsb (Nat.eqb (Schema.descriptor_tag field)) [5; 6; 7; 8; 9]
  | Schema.Message =>
      Schema.wire_eqb (Schema.descriptor_wire field) Schema.LengthDelimited
  | Schema.StringMap | Schema.PathMap => false
  end.

Definition connective_fields_classified : bool :=
  match Schema.lookup_projected_program 55 [] [] with
  | None => false
  | Some program =>
      forallb connective_field_is_classified (Schema.program_fields program)
  end.

Lemma connective_fields_classified_true :
  connective_fields_classified = true.
Proof. vm_compute. reflexivity. Qed.

Lemma generated_connective_field_is_classified :
  forall program field,
    Schema.lookup_projected_program 55 [] [] = Some program ->
    In field (Schema.program_fields program) ->
    connective_field_is_classified field = true.
Proof.
  intros program field looked_up member.
  pose proof connective_fields_classified_true as certificate.
  unfold connective_fields_classified in certificate.
  rewrite looked_up in certificate.
  change (forallb connective_field_is_classified
    (Schema.program_fields program) = true) in certificate.
  rewrite forallb_forall in certificate.
  exact (certificate field member).
Qed.

Lemma connective_scalar_constructors_agree :
  forall tag,
    In tag [5; 6; 7; 8; 9] ->
    Name.source_connective_scalar_constructor tag =
    Target.connective_scalar_constructor tag.
Proof.
  intros tag member.
  destruct member as [same|[same|[same|[same|[same|[]]]]]];
    subst tag; vm_compute; reflexivity.
Qed.

(** What the target's connective reducer observes of a decoded child.  The
    scalar case records the arm tag and the target decoder's verdict. *)
Definition connective_child_total (child : Core.source_exact_summary) : Prop :=
  match Core.source_summary_value child with
  | Core.TypedSourceScalar _ (Core.SourceBool value) =>
      Schema.raw_wire (Core.source_summary_raw child) = Schema.Varint /\
      Schema.raw_payload (Core.source_summary_raw child) = Schema.Scalar /\
      In (Core.source_summary_tag child) [5; 6; 7; 8; 9] /\
      Scalar.scalar_semantic_from_source
        (Scalar.ScalarRule (Core.source_summary_tag child) Data.PBBool false)
        (Core.source_summary_body child) = Some (Scalar.ScalarBool value)
  | Core.TypedSourceMessage _ _ =>
      Schema.raw_wire (Core.source_summary_raw child) =
        Schema.LengthDelimited /\
      Schema.raw_payload (Core.source_summary_raw child) = Schema.Message
  | _ => False
  end.

Lemma connective_child_is_total :
  forall fuel hash_fn body program node child,
    Schema.lookup_projected_program 55 [] [] = Some program ->
    aligned_child fuel hash_fn body program node child ->
    connective_child_total child.
Proof.
  intros fuel hash_fn body program node child looked_up
    [[selected _] evaluated].
  pose proof (generated_connective_field_is_classified program
    (Source.source_field node) looked_up
    (source_descriptor_field_member program (Source.source_span node)
      (Source.source_field node) selected)) as classified.
  destruct (Source.resolved_source_descriptor_has_actual_tag_and_wire program
    (Source.source_span node) (Source.source_field node) selected)
    as [field_tag field_wire].
  destruct node as [span field|span field nested|span field];
    cbn [Source.source_span Source.source_field]
      in classified, field_tag, field_wire.
  - (* A scalar node of the connective decodes to a boolean arm. *)
    destruct (evaluated_scalar_node_shape _ hash_fn body program span field
      child evaluated) as [raw [scalar [same_raw [decoded [typed shape]]]]].
    assert (raw_is : Core.source_summary_raw child = raw)
      by (rewrite shape; reflexivity).
    assert (value_is : Core.source_summary_value child =
        Core.TypedSourceScalar raw scalar) by (rewrite shape; reflexivity).
    destruct (ProcessChild.converted_scalar_raw_has_scalar_shape body span
      field raw same_raw) as [_ [_ field_payload]].
    destruct (Source.raw_field_of_source_preserves_wire_and_body body
      (Source.SourceScalar span field) raw same_raw) as [raw_tag_is _].
    cbn [Source.source_span] in raw_tag_is.
    unfold connective_field_is_classified in classified.
    rewrite field_payload in classified.
    cbv beta iota zeta in classified.
    apply andb_true_iff in classified as [_ tag_member].
    assert (member : In (Schema.raw_tag raw) [5; 6; 7; 8; 9]).
    { rewrite raw_tag_is, <- field_tag.
      exact (existsb_eqb_member _ _ tag_member). }
    assert (contract_at_tag :
      Admission.source_scalar_contract_for (Schema.program_family program)
        (Schema.raw_tag raw) =
      Some (Admission.SourceScalarContract Data.PBBool false)).
    { rewrite (looked_up_program_family 55 program looked_up).
      exact (NameBridge.generated_connective_bool_contract _ member). }
    destruct (independent_pbbool_is_bool false (Schema.raw_body raw) scalar
      (scalar_value_decodes_with_contract program raw scalar Data.PBBool false
        decoded contract_at_tag)) as [value same_scalar].
    subst scalar.
    destruct (NameBridge.generated_connective_bool_decoder_bridge program raw
      value looked_up member typed) as [wire [payload semantic]].
    unfold connective_child_total.
    rewrite value_is.
    change (Schema.raw_wire (Core.source_summary_raw child) = Schema.Varint /\
      Schema.raw_payload (Core.source_summary_raw child) = Schema.Scalar /\
      In (Schema.raw_tag (Core.source_summary_raw child)) [5; 6; 7; 8; 9] /\
      Scalar.scalar_semantic_from_source
        (Scalar.ScalarRule (Schema.raw_tag (Core.source_summary_raw child))
          Data.PBBool false)
        (Schema.raw_body (Core.source_summary_raw child)) =
        Some (Scalar.ScalarBool value)).
    rewrite raw_is.
    exact (conj wire (conj payload (conj member semantic))).
  - (* A message node carries a length-delimited, typed message raw. *)
    destruct (ProcessChild.accepted_source_message_node_has_typed_value fuel
      hash_fn body program span field nested child evaluated)
      as [raw [grandchildren [same_raw [typed_message raw_message]]]].
    destruct (Source.raw_field_of_source_preserves_wire_and_body body
      (Source.SourceMessage span field nested) raw same_raw)
      as [_ [raw_wire_is [raw_payload_is _]]].
    cbn [Source.source_span Source.source_field]
      in raw_wire_is, raw_payload_is.
    assert (field_payload : Schema.descriptor_payload field = Schema.Message).
    { rewrite <- raw_payload_is. exact raw_message. }
    unfold connective_field_is_classified in classified.
    rewrite field_payload in classified.
    cbv beta iota zeta in classified.
    assert (wire_length : Schema.raw_wire raw = Schema.LengthDelimited).
    { rewrite raw_wire_is, (wire_eqb_true_same _ _ field_wire).
      exact (wire_eqb_true_same _ _ classified). }
    assert (raw_is : Core.source_summary_raw child = raw).
    { unfold Core.source_summary_raw. rewrite typed_message. reflexivity. }
    unfold connective_child_total.
    rewrite typed_message.
    change (Schema.raw_wire (Core.source_summary_raw child) =
        Schema.LengthDelimited /\
      Schema.raw_payload (Core.source_summary_raw child) = Schema.Message).
    rewrite raw_is.
    exact (conj wire_length raw_message).
  - (* The classified program has no auxiliary payload kinds. *)
    exfalso.
    pose proof (evaluated_node_retains_source_raw fuel hash_fn body program
      (Source.SourceAuxiliary span field) child evaluated) as converted.
    unfold Source.raw_field_of_source in converted.
    cbn [Source.source_span Source.source_field] in converted.
    unfold connective_field_is_classified in classified.
    destruct (Schema.descriptor_payload field);
      [discriminate converted|discriminate converted
      |discriminate classified|discriminate classified].
Qed.

Lemma connective_children_are_total :
  forall fuel hash_fn body program nodes children,
    Schema.lookup_projected_program 55 [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Forall connective_child_total children.
Proof.
  intros fuel hash_fn body program nodes children looked_up aligned.
  eapply forall2_right_forall; [|exact aligned].
  intros node child pair.
  exact (connective_child_is_total fuel hash_fn body program node child
    looked_up pair).
Qed.

Lemma connective_single_child_projects_totally :
  forall child,
    connective_child_total child ->
    Target.exact_connective_score
      (map Projection.project_source_summary [child]) =
    option_map Core.project_score (Name.source_connective_score [child]).
Proof.
  intros child facts.
  unfold Target.exact_connective_score, Name.source_connective_score.
  cbn [map].
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  change (Target.sem_wire (Projection.project_source_summary child))
    with (Schema.raw_wire (Core.source_summary_raw child)).
  change (Target.sem_payload (Projection.project_source_summary child))
    with (Schema.raw_payload (Core.source_summary_raw child)).
  change (Target.sem_body (Projection.project_source_summary child))
    with (Core.source_summary_body child).
  change (Target.sem_definitely_refused
      (Projection.project_source_summary child))
    with (Core.source_summary_refused child).
  change (Target.sem_type_id (Projection.project_source_summary child))
    with (match Core.source_summary_type_id child with
      | Some type_id => type_id | None => 0 end).
  change (Target.sem_score_evidence
      (Projection.project_source_summary child))
    with (Projection.project_source_evidence
      (Core.source_summary_exact_score child)).
  unfold connective_child_total in facts.
  destruct (Core.source_summary_refused child) eqn:refused.
  - destruct (Schema.raw_wire (Core.source_summary_raw child));
      reflexivity.
  - destruct (Core.source_summary_value child) as
      [raw scalar|raw msgs|raw key value|raw snapshot values]
      eqn:source_value.
    + destruct scalar as [value|value|value|value|value|bytes|bytes];
        try (exfalso; exact facts).
      cbv beta iota zeta in facts.
      destruct facts as [wire [payload [member decoder]]].
      rewrite wire, payload,
        <- (connective_scalar_constructors_agree _ member), decoder.
      destruct (Name.source_connective_scalar_constructor
        (Core.source_summary_tag child)) as [constructor|];
        reflexivity.
    + cbv beta iota zeta in facts.
      destruct facts as [wire payload].
      rewrite wire, payload.
      destruct (Core.source_summary_type_id child)
        as [[|[|observed]]|];
        destruct (Core.source_summary_tag child =? 3);
        destruct (Core.source_summary_exact_score child) as [nested|];
        reflexivity.
    + exfalso. exact facts.
    + exfalso. exact facts.
Qed.

Lemma connective_score_projects_totally :
  forall children,
    Forall connective_child_total children ->
    Target.exact_connective_score
      (map Projection.project_source_summary children) =
    option_map Core.project_score (Name.source_connective_score children).
Proof.
  intros children total.
  destruct children as [|child [|second rest]].
  - reflexivity.
  - inversion total as [|? ? head_total ?]; subst.
    exact (connective_single_child_projects_totally child head_total).
  - reflexivity.
Qed.

Lemma connective_family_projects :
  forall children,
    Forall connective_child_total children ->
    Target.score_evidence_for 55 (Some true)
      (map Projection.project_source_summary children) =
    Projection.project_source_evidence
      (Name.source_name_family_score 55 (Some true) children).
Proof.
  intros children total.
  change (Target.exact_or_unknown (Target.exact_connective_score
      (map Projection.project_source_summary children)) =
    Projection.project_source_evidence (Name.source_connective_score children)).
  exact (exact_or_unknown_of_projected _ _
    (connective_score_projects_totally children total)).
Qed.

(** ** The unforgeable-name family, 60 *)

(** Every generated field of the unforgeable program is a length-delimited
    message arm whose child type is the one in the source contract. *)
Definition unforgeable_field_is_arm
    (field : Schema.field_descriptor) : bool :=
  Schema.payload_eqb (Schema.descriptor_payload field) Schema.Message &&
  Schema.wire_eqb (Schema.descriptor_wire field) Schema.LengthDelimited &&
  existsb (Nat.eqb (Schema.descriptor_tag field)) [1; 2; 3; 4] &&
  match Name.source_unforgeable_contract (Schema.descriptor_tag field),
      Schema.descriptor_child_type_id field with
  | Some (expected, _, _), Some observed => expected =? observed
  | _, _ => false
  end.

Definition unforgeable_fields_are_arms : bool :=
  match Schema.lookup_projected_program 60 [] [] with
  | None => false
  | Some program =>
      forallb unforgeable_field_is_arm (Schema.program_fields program)
  end.

Lemma unforgeable_fields_are_arms_true : unforgeable_fields_are_arms = true.
Proof. vm_compute. reflexivity. Qed.

Lemma generated_unforgeable_field_is_arm :
  forall program field,
    Schema.lookup_projected_program 60 [] [] = Some program ->
    In field (Schema.program_fields program) ->
    unforgeable_field_is_arm field = true.
Proof.
  intros program field looked_up member.
  pose proof unforgeable_fields_are_arms_true as certificate.
  unfold unforgeable_fields_are_arms in certificate.
  rewrite looked_up in certificate.
  change (forallb unforgeable_field_is_arm
    (Schema.program_fields program) = true) in certificate.
  rewrite forallb_forall in certificate.
  exact (certificate field member).
Qed.

(** The four arm tags share one fixed contract in source and target, and the
    three arms carrying a leaf body name a program among types 61 to 63. *)
Lemma generated_unforgeable_arm_is_fixed :
  forall tag,
    In tag [1; 2; 3; 4] ->
    exists type_id constructor bytes_required,
      Name.source_unforgeable_contract tag =
        Some (type_id, constructor, bytes_required) /\
      Target.unforgeable_arm_contract tag =
        Some (type_id, constructor, bytes_required) /\
      (bytes_required = true -> In type_id [61; 62; 63]).
Proof.
  intros tag member.
  destruct member as [same|[same|[same|[same|[]]]]]; subst tag.
  - exists 61, 5%N, true.
    split; [vm_compute; reflexivity|].
    split; [reflexivity|].
    intros _. left. reflexivity.
  - exists 62, 11%N, true.
    split; [vm_compute; reflexivity|].
    split; [reflexivity|].
    intros _. right. left. reflexivity.
  - exists 63, 10%N, true.
    split; [vm_compute; reflexivity|].
    split; [reflexivity|].
    intros _. right. right. left. reflexivity.
  - exists 64, 12%N, false.
    split; [vm_compute; reflexivity|].
    split; [reflexivity|].
    intro impossible. discriminate impossible.
Qed.

(** Programs 61, 62 and 63 are one required bytes field at tag 1. *)
Lemma leaf_program_facts :
  forall type_id program,
    In type_id [61; 62; 63] ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    (forall field, In field (Schema.program_fields program) ->
      Schema.descriptor_tag field = 1 /\
      Schema.descriptor_payload field = Schema.Scalar) /\
    Scalar.generated_scalar_rule program 1 =
      Some (Scalar.ScalarRule 1 Data.PBBytes true) /\
    Admission.source_scalar_contract_for (Schema.program_family program) 1 =
      Some (Admission.SourceScalarContract Data.PBBytes true).
Proof.
  intros type_id program member looked_up.
  destruct member as [same|[same|[same|[]]]]; subst type_id;
    vm_compute in looked_up;
    injection looked_up as same_program; subst program;
    (split; [|split; vm_compute; reflexivity]);
    intros field field_member;
    simpl in field_member;
    destruct field_member as [same_field|[]];
    subst field;
    split; reflexivity.
Qed.

Lemma forall2_head_inv :
  forall (A B : Type) (relation : A -> B -> Prop) left_value right_value
      lefts rights,
    Forall2 relation (left_value :: lefts) (right_value :: rights) ->
    relation left_value right_value.
Proof.
  intros A B relation left_value right_value lefts rights related.
  inversion related; subst; assumption.
Qed.

Lemma forall2_same_length :
  forall (A B : Type) (relation : A -> B -> Prop) lefts rights,
    Forall2 relation lefts rights -> length lefts = length rights.
Proof.
  intros A B relation lefts rights related.
  induction related as [|left_value right_value lefts' rights' head rest IH].
  - reflexivity.
  - cbn [length]. rewrite IH. reflexivity.
Qed.

(** A leaf body decodes the same way through the source oracle and through the
    target's reduction: empty when the body is empty, otherwise one bytes
    field at tag 1 whose body is the non-empty value both sides return. *)
Lemma unforgeable_leaf_bytes_agree :
  forall fuel hash_fn raw child type_id,
    In type_id [61; 62; 63] ->
    Schema.raw_program_type_id raw = Some type_id ->
    Oracle.source_evaluate_message fuel hash_fn raw = Some child ->
    Target.exact_unforgeable_leaf_bytes type_id (Schema.raw_body raw) =
    Name.source_unforgeable_leaf_bytes child.
Proof.
  intros [|fuel] hash_fn raw child type_id member program_type evaluated;
    [discriminate evaluated|].
  destruct (ProcessChild.accepted_independent_message_has_generated_children
    fuel hash_fn raw child evaluated)
    as [found_type [program [nodes [children [payload_message [found_program
      [looked_up [decoded [summary aligned]]]]]]]]].
  assert (same_type : found_type = type_id) by congruence.
  subst found_type.
  destruct (leaf_program_facts type_id program member looked_up)
    as [leaf_fields [leaf_rule leaf_contract]].
  assert (child_value : Core.source_summary_value child =
      Core.TypedSourceMessage raw (map Core.source_summary_value children))
    by (rewrite summary; reflexivity).
  pose proof (forall2_same_length _ _ _ _ _ aligned) as same_length.
  unfold Target.exact_unforgeable_leaf_bytes,
    Name.source_unforgeable_leaf_bytes.
  rewrite looked_up.
  cbv beta iota zeta.
  rewrite decoded, child_value.
  destruct nodes as [|node [|second more]].
  - (* An empty decode leaves no bytes to account for. *)
    destruct children as [|first_child others];
      [|cbn [length] in same_length; discriminate same_length].
    pose proof (decoded_empty_region_has_no_bytes _ _ _ _ _ decoded) as empty.
    cbn [map].
    rewrite <- empty.
    reflexivity.
  - (* One node: it can only be the scalar bytes field. *)
    destruct children as [|first_child [|second_child others]];
      [cbn [length] in same_length; discriminate same_length
      |
      |cbn [length] in same_length; discriminate same_length].
    destruct (forall2_head_inv _ _ _ _ _ _ _ aligned)
      as [[selected _] evaluated_child].
    destruct node as [span field|span field nested|span field].
    + cbn [Source.source_span Source.source_field] in selected.
      destruct (evaluated_scalar_node_shape _ hash_fn (Schema.raw_body raw)
        program span field first_child evaluated_child)
        as [raw_gc [scalar [same_raw [decoded_scalar [typed shape]]]]].
      destruct (leaf_fields field (source_descriptor_field_member program span
        field selected)) as [field_tag_one field_scalar].
      destruct (Source.raw_field_of_source_preserves_wire_and_body
        (Schema.raw_body raw) (Source.SourceScalar span field) raw_gc same_raw)
        as [raw_tag_is [_ [_ [raw_body_is _]]]].
      cbn [Source.source_span] in raw_tag_is.
      destruct (Source.resolved_source_descriptor_has_actual_tag_and_wire
        program span field selected) as [field_tag _].
      assert (raw_tag_one : Schema.raw_tag raw_gc = 1).
      { rewrite raw_tag_is, <- field_tag. exact field_tag_one. }
      assert (contract_at_tag :
        Admission.source_scalar_contract_for (Schema.program_family program)
          (Schema.raw_tag raw_gc) =
        Some (Admission.SourceScalarContract Data.PBBytes true)).
      { rewrite raw_tag_one. exact leaf_contract. }
      destruct (independent_forbidden_bytes_is_nonempty_body
        (Schema.raw_body raw_gc) scalar
        (scalar_value_decodes_with_contract program raw_gc scalar Data.PBBytes
          true decoded_scalar contract_at_tag)) as [same_scalar nonempty].
      subst scalar.
      pose proof (target_forbidden_bytes_semantic (Schema.raw_body raw_gc)
        nonempty) as leaf_semantic.
      subst first_child.
      cbn [map Core.source_summary_value].
      rewrite field_tag_one, leaf_rule.
      cbv beta iota zeta.
      rewrite <- raw_body_is, leaf_semantic, raw_tag_one.
      reflexivity.
    + exfalso.
      cbn [Source.source_span Source.source_field] in selected.
      destruct (leaf_fields field (source_descriptor_field_member program span
        field selected)) as [_ field_scalar].
      pose proof (evaluated_node_retains_source_raw fuel hash_fn
        (Schema.raw_body raw) program (Source.SourceMessage span field nested)
        first_child evaluated_child) as converted.
      unfold Source.raw_field_of_source in converted.
      cbn [Source.source_span Source.source_field] in converted.
      rewrite field_scalar in converted.
      discriminate converted.
    + exfalso.
      cbn [Source.source_span Source.source_field] in selected.
      destruct (leaf_fields field (source_descriptor_field_member program span
        field selected)) as [_ field_scalar].
      pose proof (evaluated_node_retains_source_raw fuel hash_fn
        (Schema.raw_body raw) program (Source.SourceAuxiliary span field)
        first_child evaluated_child) as converted.
      unfold Source.raw_field_of_source in converted.
      cbn [Source.source_span Source.source_field] in converted.
      rewrite field_scalar in converted.
      discriminate converted.
  - (* Two or more nodes are refused on both sides. *)
    destruct children as [|first_child [|second_child others]];
      [cbn [length] in same_length; discriminate same_length
      |cbn [length] in same_length; discriminate same_length
      |].
    destruct node as [span field|span field nested|span field];
      cbn [map Core.source_summary_value];
      destruct (Core.source_summary_value first_child) as [r s| | |];
      try reflexivity;
      destruct s; reflexivity.
Qed.

(** What the target's unforgeable reducer observes of a decoded child: the
    arm contract at its tag, its typed message shape, and the agreement of
    the leaf body whenever the arm requires one. *)
Definition unforgeable_child_total
    (child : Core.source_exact_summary) : Prop :=
  exists type_id constructor bytes_required,
    Name.source_unforgeable_contract (Core.source_summary_tag child) =
      Some (type_id, constructor, bytes_required) /\
    Target.unforgeable_arm_contract (Core.source_summary_tag child) =
      Some (type_id, constructor, bytes_required) /\
    Core.source_summary_type_id child = Some type_id /\
    Schema.raw_wire (Core.source_summary_raw child) =
      Schema.LengthDelimited /\
    Schema.raw_payload (Core.source_summary_raw child) = Schema.Message /\
    (bytes_required = true ->
      Target.exact_unforgeable_leaf_bytes type_id
        (Core.source_summary_body child) =
      Name.source_unforgeable_leaf_bytes child).

Lemma unforgeable_child_is_total :
  forall fuel hash_fn body program node child,
    Schema.lookup_projected_program 60 [] [] = Some program ->
    aligned_child fuel hash_fn body program node child ->
    unforgeable_child_total child.
Proof.
  intros fuel hash_fn body program node child looked_up
    [[selected _] evaluated].
  pose proof (generated_unforgeable_field_is_arm program
    (Source.source_field node) looked_up
    (source_descriptor_field_member program (Source.source_span node)
      (Source.source_field node) selected)) as arm.
  destruct (Source.resolved_source_descriptor_has_actual_tag_and_wire program
    (Source.source_span node) (Source.source_field node) selected)
    as [field_tag field_wire].
  unfold unforgeable_field_is_arm in arm.
  repeat rewrite andb_true_iff in arm.
  destruct arm as [[[payload_ok wire_ok] tag_ok] type_ok].
  apply existsb_eqb_member in tag_ok.
  destruct (generated_unforgeable_arm_is_fixed _ tag_ok)
    as [type_id [constructor [bytes_required
      [source_contract [target_contract bytes_leaf]]]]].
  rewrite source_contract in type_ok.
  cbv beta iota zeta in type_ok.
  destruct (Schema.descriptor_child_type_id (Source.source_field node))
    as [observed|] eqn:field_child_type; [|discriminate type_ok].
  apply Nat.eqb_eq in type_ok.
  subst observed.
  pose proof (evaluated_node_retains_source_raw fuel hash_fn body program node
    child evaluated) as converted.
  destruct (Source.raw_field_of_source_preserves_wire_and_body body node
    (Core.source_summary_raw child) converted)
    as [raw_tag_is [raw_wire_is [raw_payload_is _]]].
  assert (tag_is : Core.source_summary_tag child =
      Schema.descriptor_tag (Source.source_field node)).
  { change (Schema.raw_tag (Core.source_summary_raw child) =
      Schema.descriptor_tag (Source.source_field node)).
    rewrite raw_tag_is. symmetry. exact field_tag. }
  assert (wire_is : Schema.raw_wire (Core.source_summary_raw child) =
      Schema.LengthDelimited).
  { rewrite raw_wire_is, (wire_eqb_true_same _ _ field_wire).
    exact (wire_eqb_true_same _ _ wire_ok). }
  assert (payload_is : Schema.raw_payload (Core.source_summary_raw child) =
      Schema.Message).
  { rewrite raw_payload_is. exact (payload_eqb_true_same _ _ payload_ok). }
  destruct node as [span field|span field nested|span field].
  - (* An arm is never carried by a scalar node. *)
    exfalso.
    destruct (ProcessChild.converted_scalar_raw_has_scalar_shape body span
      field (Core.source_summary_raw child) converted) as [raw_scalar _].
    rewrite payload_is in raw_scalar.
    discriminate raw_scalar.
  - (* The message arm itself. *)
    assert (same_type : Core.source_summary_type_id child = Some type_id).
    { change (Schema.raw_program_type_id (Core.source_summary_raw child) =
        Some type_id).
      rewrite (Source.raw_message_field_uses_generated_child_type body span
        field nested (Core.source_summary_raw child) converted).
      exact field_child_type. }
    assert (evaluated_message : Oracle.source_evaluate_message fuel hash_fn
        (Core.source_summary_raw child) = Some child).
    { unfold Oracle.source_evaluate_node in evaluated.
      rewrite converted in evaluated.
      exact evaluated. }
    unfold unforgeable_child_total.
    exists type_id, constructor, bytes_required.
    rewrite tag_is.
    split; [exact source_contract|].
    split; [exact target_contract|].
    split; [exact same_type|].
    split; [exact wire_is|].
    split; [exact payload_is|].
    intro required.
    exact (unforgeable_leaf_bytes_agree fuel hash_fn
      (Core.source_summary_raw child) child type_id (bytes_leaf required)
      same_type evaluated_message).
  - (* An arm is never carried by an auxiliary node. *)
    exfalso.
    cbn [Source.source_field] in payload_ok.
    pose proof (payload_eqb_true_same _ _ payload_ok) as field_payload.
    unfold Source.raw_field_of_source in converted.
    cbn [Source.source_span Source.source_field] in converted.
    rewrite field_payload in converted.
    discriminate converted.
Qed.

Lemma unforgeable_children_are_total :
  forall fuel hash_fn body program nodes children,
    Schema.lookup_projected_program 60 [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Forall unforgeable_child_total children.
Proof.
  intros fuel hash_fn body program nodes children looked_up aligned.
  eapply forall2_right_forall; [|exact aligned].
  intros node child pair.
  exact (unforgeable_child_is_total fuel hash_fn body program node child
    looked_up pair).
Qed.

Lemma unforgeable_single_child_projects_totally :
  forall child,
    unforgeable_child_total child ->
    Target.exact_unforgeable_score
      (map Projection.project_source_summary [child]) =
    option_map Core.project_score (Name.source_unforgeable_score [child]).
Proof.
  intros child facts.
  destruct facts as [type_id [constructor [bytes_required
    [source_contract [target_contract [source_type [wire [payload leaf]]]]]]]].
  unfold Target.exact_unforgeable_score, Name.source_unforgeable_score.
  cbn [map].
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  change (Target.sem_wire (Projection.project_source_summary child))
    with (Schema.raw_wire (Core.source_summary_raw child)).
  change (Target.sem_payload (Projection.project_source_summary child))
    with (Schema.raw_payload (Core.source_summary_raw child)).
  change (Target.sem_body (Projection.project_source_summary child))
    with (Core.source_summary_body child).
  change (Target.sem_definitely_refused
      (Projection.project_source_summary child))
    with (Core.source_summary_refused child).
  change (Target.sem_type_id (Projection.project_source_summary child))
    with (match Core.source_summary_type_id child with
      | Some type_id => type_id | None => 0 end).
  rewrite target_contract, source_contract, source_type, wire, payload.
  destruct (Core.source_summary_refused child) eqn:refused.
  - destruct bytes_required; reflexivity.
  - destruct bytes_required.
    + cbv beta iota zeta.
      rewrite Nat.eqb_refl.
      cbn [andb Schema.wire_eqb Schema.payload_eqb].
      rewrite (leaf eq_refl).
      destruct (Name.source_unforgeable_leaf_bytes child) as [bytes|];
        reflexivity.
    + cbv beta iota zeta.
      rewrite Nat.eqb_refl.
      cbn [andb Schema.wire_eqb Schema.payload_eqb].
      destruct (length (Core.source_summary_body child) =? 0);
        reflexivity.
Qed.

Lemma unforgeable_score_projects_totally :
  forall children,
    Forall unforgeable_child_total children ->
    Target.exact_unforgeable_score
      (map Projection.project_source_summary children) =
    option_map Core.project_score (Name.source_unforgeable_score children).
Proof.
  intros children total.
  destruct children as [|child [|second rest]].
  - reflexivity.
  - inversion total as [|? ? head_total ?]; subst.
    exact (unforgeable_single_child_projects_totally child head_total).
  - reflexivity.
Qed.

Lemma unforgeable_family_projects :
  forall children,
    Forall unforgeable_child_total children ->
    Target.score_evidence_for 60 (Some true)
      (map Projection.project_source_summary children) =
    Projection.project_source_evidence
      (Name.source_name_family_score 60 (Some true) children).
Proof.
  intros children total.
  change (Target.exact_or_unknown (Target.exact_unforgeable_score
      (map Projection.project_source_summary children)) =
    Projection.project_source_evidence
      (Name.source_unforgeable_score children)).
  exact (exact_or_unknown_of_projected _ _
    (unforgeable_score_projects_totally children total)).
Qed.

(** ** The bridge

    The decoded nodes of a family's generated program are aligned with the
    children the oracle selected from them, and on those children the target's
    family reducer and the source's family score agree: for the four name
    families this is not only agreement on success but equality of the
    projected evidence.  The scan-validity premise of the statement is not
    needed, because no score of these families consults the source scan.  No
    restriction on [complete] is needed either: outside [Some true] both sides
    are [ScoreNotDerived] for these four families. *)
Theorem generated_name_family_score_projects_totally :
  forall fuel hash_fn family program body nodes children complete,
    In family [31; 32; 55; 60] ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.decode_region (S (length body)) body program 0 (length body) = Some nodes ->
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program) nodes)
      = Some children ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    Target.score_evidence_for family complete
      (map Projection.project_source_summary children) =
    Projection.project_source_evidence
      (Name.source_name_family_score family complete children).
Proof.
  intros fuel hash_fn family program body nodes children complete
    family_member looked_up decoded selected _.
  assert (aligned :
    Forall2 (aligned_child fuel hash_fn body program) nodes children).
  { exact (ProcessChild.generated_selected_children_are_aligned program
      (Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
        hash_fn body program) nodes children
      (Source.decoded_region_nodes_match_generated_source _ _ _ _ _ _ decoded)
      (RecursiveProjection.selected_source_evaluator_outputs_are_positional
        _ _ _ _ _ selected)). }
  destruct complete as [[|]|].
  - destruct family_member as [same|[same|[same|[same|[]]]]]; subst family.
    + exact (epathmap_family_projects fuel hash_fn body program nodes children
        looked_up aligned).
    + exact (ezipper_family_projects fuel hash_fn body program nodes children
        looked_up aligned).
    + exact (connective_family_projects children
        (connective_children_are_total fuel hash_fn body program nodes children
          looked_up aligned)).
    + exact (unforgeable_family_projects children
        (unforgeable_children_are_total fuel hash_fn body program nodes children
          looked_up aligned)).
  - destruct family_member as [same|[same|[same|[same|[]]]]]; subst family;
      reflexivity.
  - destruct family_member as [same|[same|[same|[same|[]]]]]; subst family;
      reflexivity.
Qed.

Print Assumptions wire_eqb_true_same.
Print Assumptions payload_eqb_true_same.
Print Assumptions existsb_eqb_member.
Print Assumptions forall2_right_forall.
Print Assumptions rev_empty_is_empty.
Print Assumptions source_descriptor_field_member.
Print Assumptions looked_up_program_family.
Print Assumptions evaluated_node_retains_source_raw.
Print Assumptions evaluated_scalar_node_shape.
Print Assumptions scalar_value_decodes_with_contract.
Print Assumptions independent_pbbool_is_bool.
Print Assumptions independent_forbidden_bool_is_true_one_byte.
Print Assumptions independent_forbidden_bytes_is_nonempty_body.
Print Assumptions target_forbidden_bytes_semantic.
Print Assumptions reference_remaining_empty_region.
Print Assumptions decode_source_fold_tracks_length.
Print Assumptions decoded_empty_region_has_no_bytes.
Print Assumptions exact_or_unknown_of_projected.
Print Assumptions scalar_true_or_absent_agrees_of_flag_children.
Print Assumptions epathmap_flag_tag_is_scalar.
Print Assumptions ezipper_flag_tag_is_scalar.
Print Assumptions epathmap_flag_contract.
Print Assumptions ezipper_flag_contract.
Print Assumptions generated_flag_tag_field_is_scalar.
Print Assumptions forbidden_flag_child_is_true.
Print Assumptions flag_tag_children_agree.
Print Assumptions epathmap_family_projects.
Print Assumptions ezipper_family_projects.
Print Assumptions connective_fields_classified_true.
Print Assumptions generated_connective_field_is_classified.
Print Assumptions connective_scalar_constructors_agree.
Print Assumptions connective_child_is_total.
Print Assumptions connective_children_are_total.
Print Assumptions connective_single_child_projects_totally.
Print Assumptions connective_score_projects_totally.
Print Assumptions connective_family_projects.
Print Assumptions unforgeable_fields_are_arms_true.
Print Assumptions generated_unforgeable_field_is_arm.
Print Assumptions generated_unforgeable_arm_is_fixed.
Print Assumptions leaf_program_facts.
Print Assumptions forall2_head_inv.
Print Assumptions forall2_same_length.
Print Assumptions unforgeable_leaf_bytes_agree.
Print Assumptions unforgeable_child_is_total.
Print Assumptions unforgeable_children_are_total.
Print Assumptions unforgeable_single_child_projects_totally.
Print Assumptions unforgeable_score_projects_totally.
Print Assumptions unforgeable_family_projects.
Print Assumptions generated_name_family_score_projects_totally.
