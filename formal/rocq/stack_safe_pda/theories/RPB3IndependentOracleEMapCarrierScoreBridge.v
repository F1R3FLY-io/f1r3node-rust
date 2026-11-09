(** * Bridge for the EMap payload score, type 30, and its KeyValuePair carrier,
    type 34.

    On the children that the oracle actually produces for a decoded node of one
    of these two families, the target helper agrees with the projection of the
    oracle helper, score and completeness:

    - EMap is constructor 9 over the remainder (default -1), the key scores of
      the pairs and the connective flag.  Values never enter the score.
    - The pair carrier's evidence is [score_constructor 0 [key_score;
      ScoreBytes (par_identity_bytes key_body)]].

    The first half of this file repeats the reader agreements of the sibling
    bridges, because each bridge is a self-contained theory.  The second half is
    the part specific to the pair carrier: the two sides compute the identity of
    the key Par in unrelated ways.  The target removes one byte span from the
    Par body; the oracle re-serializes the Par's direct children other than
    tag 9.  They agree on an accepted key because the decoder accepts only
    canonical framing, so that each field's re-encoding is exactly its original
    bytes ([checked_field_reencodes]), the decoded spans tile the body
    ([reference_remaining_tiles]), and the key's own scan is valid so tag 9, a
    singular field, occurs at most once ([scan_tag_appears_at_most_once]).

    The red control at the end shows that a carrier that includes the value
    score breaks the bridge. *)

From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleProjection RPB3IndependentOracleRecursive
  RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleNameFamilyBridge
  RPB3IndependentOracleProcessTypedChildren
  RPB3SemanticReducer RPB3IndependentOracleExprPayload.

Import ListNotations.

(** ** Readers on one actual child *)

Lemma payload_typed_message_score_is_process :
  forall type_id child,
    source_payload_typed_message_score type_id child =
    Process.source_typed_message_score type_id child.
Proof. reflexivity. Qed.

Lemma aligned_child_typed_message_score_projects :
  forall fuel hash_fn family program body node child type_id,
    Schema.lookup_projected_program family [] [] = Some program ->
    aligned_child fuel hash_fn body program node child ->
    Target.exact_typed_message_score type_id
      (Projection.project_source_summary child) =
    option_map Core.project_score
      (source_payload_typed_message_score type_id child).
Proof.
  intros fuel hash_fn family program body node child type_id looked_up
    [matched evaluated].
  rewrite payload_typed_message_score_is_process.
  destruct node as [span field|span field nested|span field].
  - destruct (accepted_source_scalar_node_has_typed_value
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program
      span field child evaluated) as [raw [scalar [_ [typed payload]]]].
    destruct (typed_scalar_child_is_not_message child raw scalar typed payload)
      as [not_message not_value].
    exact (non_message_child_typed_score_projects child type_id not_message
      not_value).
  - unfold Source.source_node_matches_generated_descriptor in matched.
    destruct matched as [resolved _].
    cbn [Source.source_span Source.source_field] in resolved.
    exact (accepted_generated_message_node_typed_score_projects
      fuel hash_fn body family program span field nested child type_id
      looked_up resolved evaluated).
  - destruct (evaluated_auxiliary_node_is_not_message
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program
      span field child evaluated) as [not_message not_value].
    exact (non_message_child_typed_score_projects child type_id not_message
      not_value).
Qed.

Lemma aligned_child_of_member :
  forall fuel hash_fn body program nodes children child,
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    In child children ->
    exists node, aligned_child fuel hash_fn body program node child.
Proof.
  intros fuel hash_fn body program nodes children child aligned.
  induction aligned as [|node head nodes' children' pair rest IH].
  - intros [].
  - intros [same|later].
    + subst head. exists node. exact pair.
    + exact (IH later).
Qed.

(** ** Readers on the list of actual children *)

Lemma payload_typed_optional_score_or_projects :
  forall fuel hash_fn family program body nodes children tag type_id fallback,
    Schema.lookup_projected_program family [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    exact_typed_optional_score_or tag type_id (Core.project_score fallback)
      (map Projection.project_source_summary children) =
    option_map Core.project_score
      (source_payload_typed_optional_score_or tag type_id fallback children).
Proof.
  intros fuel hash_fn family program body nodes children tag type_id fallback
    looked_up aligned.
  unfold exact_typed_optional_score_or, exact_typed_optional_child_score,
    source_payload_typed_optional_score_or,
    source_payload_typed_optional_score, source_payload_child_at.
  rewrite <- Projection.projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? tag) children)
    as [child|] eqn:found; cbn [option_map].
  - apply find_some in found as [member _].
    destruct (aligned_child_of_member fuel hash_fn body program nodes children
      child aligned member) as [node pair].
    rewrite (aligned_child_typed_message_score_projects fuel hash_fn family
      program body node child type_id looked_up pair).
    destruct (source_payload_typed_message_score type_id child) as [score|];
      reflexivity.
  - reflexivity.
Qed.

Lemma payload_bool_or_absent_projects :
  forall tag children,
    Forall (connective_flag_child_true tag) children ->
    Forall (fun child => Core.source_summary_refused child = false) children ->
    scalar_true_or_absent tag
      (map Projection.project_source_summary children) =
    source_payload_bool_or_absent tag children.
Proof.
  intros tag children flags unrefused.
  rewrite Forall_forall in flags, unrefused.
  unfold scalar_true_or_absent, source_payload_bool_or_absent,
    source_payload_child_at.
  rewrite <- Projection.projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? tag) children)
    as [child|] eqn:found; [|reflexivity].
  apply find_some in found as [member same_tag].
  apply Nat.eqb_eq in same_tag.
  destruct (flags child member same_tag) as [value [payload body]].
  cbn [option_map].
  change (sem_payload (Projection.project_source_summary child))
    with (Schema.raw_payload (Core.source_summary_raw child)).
  change (sem_body (Projection.project_source_summary child))
    with (Schema.raw_body (Core.source_summary_raw child)).
  rewrite payload, body, value, (unrefused child member).
  reflexivity.
Qed.

Lemma projected_seen_slot :
  forall program children slot,
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    Target.seen_slot
      (fold_left (Target.scan_child program)
        (map Projection.project_source_summary children) Schema.empty_scan)
      slot =
    Admission.source_seen_slot
      (Admission.source_scan_children program children) slot.
Proof.
  intros program children slot valid.
  destruct (Projection.source_scan_children_project_valid program children
    valid) as [_ [_ seen]].
  unfold Target.seen_slot, Admission.source_seen_slot.
  rewrite seen.
  reflexivity.
Qed.

Lemma absent_remainder_score_is_projected :
  exact_absent_remainder_score = Core.project_score source_minus_one.
Proof. vm_compute. reflexivity. Qed.

(** ** The EMap score *)

Lemma kvp_carrier_parts_project :
  forall fuel hash_fn family program body node child,
    Schema.lookup_projected_program family [] [] = Some program ->
    aligned_child fuel hash_fn body program node child ->
    kvp_carrier_parts (Projection.project_source_summary child) =
    option_map (fun parts => (Core.project_score (fst parts), snd parts))
      (source_kvp_carrier_parts child).
Proof.
  intros fuel hash_fn family program body node child looked_up pair.
  unfold kvp_carrier_parts, source_kvp_carrier_parts.
  rewrite (aligned_child_typed_message_score_projects fuel hash_fn family
    program body node child 34 looked_up pair).
  destruct (source_payload_typed_message_score 34 child) as [score|];
    cbn [option_map]; [|reflexivity].
  destruct score as [value|text|blob|forest]; [reflexivity|reflexivity|reflexivity|].
  destruct forest as [|first rest]; [reflexivity|].
  destruct first as [constructor|t|b|f]; [|reflexivity|reflexivity|reflexivity].
  destruct rest as [|key_score rest2]; [reflexivity|].
  destruct rest2 as [|identity rest3]; [reflexivity|].
  destruct identity as [n|t|bytes|f]; [reflexivity|reflexivity| |reflexivity].
  destruct rest3 as [|extra more]; [|reflexivity].
  cbn [Core.project_score Core.project_score_forest].
  destruct (N.eqb constructor 0); reflexivity.
Qed.

Lemma emap_key_scores_project :
  forall fuel hash_fn program body nodes children,
    Schema.lookup_projected_program 30 [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    exact_emap_key_scores (map Projection.project_source_summary children) =
    option_map (map Core.project_score) (source_emap_key_scores children).
Proof.
  intros fuel hash_fn program body nodes children looked_up aligned.
  induction aligned as [|node child nodes' children' pair rest IH];
    [reflexivity|].
  cbn [map exact_emap_key_scores source_emap_key_scores].
  rewrite IH.
  destruct (source_emap_key_scores children') as [keys|]; cbn [option_map];
    [|reflexivity].
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  destruct (Core.source_summary_tag child =? 1); [|reflexivity].
  rewrite (kvp_carrier_parts_project fuel hash_fn 30 program body node child
    looked_up pair).
  destruct (source_kvp_carrier_parts child) as [[key identity]|]; reflexivity.
Qed.

Lemma emap_score_projects :
  forall fuel hash_fn program body nodes children,
    flag_tag_scalar_in_family 30 4 = true ->
    Admission.source_scalar_contract_for 30 4 =
      Some (Admission.SourceScalarContract Data.PBBool true) ->
    Schema.lookup_projected_program 30 [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Forall (fun child => Core.source_summary_refused child = false) children ->
    exact_emap_score (map Projection.project_source_summary children) =
    option_map Core.project_score (source_emap_score children).
Proof.
  intros fuel hash_fn program body nodes children certificate contract
    looked_up aligned unrefused.
  assert (flags : Forall (connective_flag_child_true 4) children).
  { eapply forall2_right_forall; [|exact aligned].
    intros node child pair.
    exact (forbidden_flag_child_is_true fuel hash_fn body program 30 4
      node child looked_up certificate contract pair). }
  pose proof (payload_bool_or_absent_projects 4 children flags unrefused)
    as flag.
  pose proof (emap_key_scores_project fuel hash_fn program body nodes children
    looked_up aligned) as keys.
  pose proof (payload_typed_optional_score_or_projects fuel hash_fn 30 program
    body nodes children 5 6 source_minus_one looked_up aligned) as remainder.
  rewrite <- absent_remainder_score_is_projected in remainder.
  unfold exact_emap_score, source_emap_score.
  rewrite remainder, keys, flag.
  destruct (source_payload_typed_optional_score_or 5 6 source_minus_one
    children) as [remainder_score|]; cbn [option_map]; [|reflexivity].
  destruct (source_emap_key_scores children) as [key_scores|];
    cbn [option_map]; [|reflexivity].
  destruct (source_payload_bool_or_absent 4 children) as [connective|];
    cbn [option_map]; [|reflexivity].
  rewrite Core.project_source_score_constructor.
  cbn [map].
  rewrite map_app.
  cbn [map].
  unfold source_payload_bool_word.
  destruct connective; reflexivity.
Qed.

(** ** Identity of an accepted Par body

    [par_identity_bytes] removes the byte span of the tag-9 field; the oracle
    re-serializes the direct children other than tag 9.  The remainder of this
    section shows that these are the same bytes whenever the Par body decoded
    and its children scan validly. *)

(** *** Byte slices *)

(** Unlike [injection], this leaves the two sides exactly as they were, which
    keeps the field numbers and offsets of a framed span syntactically intact
    for the rewrites below. *)
Lemma some_injective :
  forall (A : Type) (first_value second_value : A),
    Some first_value = Some second_value -> first_value = second_value.
Proof.
  intros A first_value second_value same.
  injection same as result.
  exact result.
Qed.

Definition byte_slice (bytes : list nat) (first limit : nat) : list nat :=
  firstn (limit - first) (skipn first bytes).

Lemma firstn_concat_skipn :
  forall (A : Type) (values : list A) width extra,
    firstn width values ++ firstn extra (skipn width values) =
    firstn (width + extra) values.
Proof.
  intros A values width. revert values.
  induction width as [|width IH]; intros values extra; simpl.
  - reflexivity.
  - destruct values as [|head tail]; simpl.
    + rewrite firstn_nil. reflexivity.
    + rewrite IH. reflexivity.
Qed.

Lemma byte_slice_concat :
  forall (bytes : list nat) first middle limit,
    first <= middle -> middle <= limit ->
    byte_slice bytes first middle ++ byte_slice bytes middle limit =
    byte_slice bytes first limit.
Proof.
  intros bytes first middle limit first_middle middle_limit.
  unfold byte_slice.
  assert (later : skipn middle bytes = skipn (middle - first) (skipn first bytes)).
  { rewrite skipn_skipn. f_equal. lia. }
  rewrite later.
  replace (limit - first) with ((middle - first) + (limit - middle)) by lia.
  apply firstn_concat_skipn.
Qed.

Lemma byte_slice_empty :
  forall bytes offset, byte_slice bytes offset offset = [].
Proof.
  intros bytes offset.
  unfold byte_slice.
  rewrite Nat.sub_diag.
  reflexivity.
Qed.

Lemma skipn_nth_cons :
  forall (A : Type) offset (values : list A) value,
    nth_error values offset = Some value ->
    skipn offset values = value :: skipn (S offset) values.
Proof.
  intros A offset.
  induction offset as [|offset IH]; intros values value selected;
    destruct values as [|head tail]; cbn in selected;
    try discriminate selected.
  - injection selected as same. subst head. reflexivity.
  - cbn [skipn]. apply IH. exact selected.
Qed.

(** *** Canonical varints re-encode to their own bytes *)

Lemma varint_groups_reencode :
  forall fuel bytes region_end offset group value result next,
    Source.Wire.varint_groups fuel bytes region_end offset group value =
      Some (result, next) ->
    exists rest,
      result = (value + rest * 128 ^ N.of_nat group)%N /\
      (0 < group -> (0 < rest)%N) /\
      source_varint_groups fuel rest = firstn (next - offset) (skipn offset bytes) /\
      offset < next.
Proof.
  induction fuel as [|fuel IH];
    intros bytes region_end offset group value result next read;
    [discriminate read|].
  cbn [Source.Wire.varint_groups] in read.
  destruct (offset <? region_end) eqn:in_region; [|discriminate read].
  destruct (nth_error bytes offset) as [byte|] eqn:selected;
    [|discriminate read].
  destruct (Source.Wire.byte_ok byte) eqn:byte_valid; [|discriminate read].
  unfold Source.Wire.byte_ok in byte_valid.
  apply Nat.ltb_lt in byte_valid.
  destruct ((group =? 9) && (1 <? byte)) eqn:top_group; [discriminate read|].
  cbv zeta in read.
  destruct (byte <? 128) eqn:small.
  - apply Nat.ltb_lt in small.
    destruct ((0 <? group) && (byte =? 0)) eqn:non_minimal; [discriminate read|].
    injection read as same_result same_next.
    subst next.
    exists (N.of_nat byte).
    assert (digit_is_byte : byte mod 128 = byte)
      by (apply Nat.mod_small; exact small).
    (* [injection] applies [simpl] to the sides of the equation, which expands
       the modulus; [change] restores the folded form by conversion. *)
    change ((value + N.of_nat (byte mod 128) * 128 ^ N.of_nat group)%N = result)
      in same_result.
    rewrite digit_is_byte in same_result.
    split; [exact (eq_sym same_result)|].
    split.
    + intro positive_group.
      apply Nat.ltb_lt in positive_group.
      rewrite positive_group in non_minimal.
      cbn [andb] in non_minimal.
      apply Nat.eqb_neq in non_minimal.
      lia.
    + split.
      * assert (width_one : S offset - offset = 1) by lia.
        rewrite width_one, (skipn_nth_cons nat offset bytes byte selected).
        cbn [source_varint_groups firstn].
        assert (ltb_small : (N.of_nat byte <? 128)%N = true)
          by (apply N.ltb_lt; lia).
        rewrite ltb_small, Nat2N.id.
        reflexivity.
      * lia.
  - apply Nat.ltb_ge in small.
    destruct (IH bytes region_end (S offset) (S group)
      (value + N.of_nat (byte mod 128) * 128 ^ N.of_nat group)%N result next
      read) as [rest [same_result [positive [encoded advanced]]]].
    pose proof (positive (Nat.lt_0_succ group)) as rest_positive.
    assert (digit_value : byte mod 128 + 128 = byte).
    { pose proof (Nat.mod_unique byte 128 1 (byte - 128) ltac:(lia) ltac:(lia))
        as digit.
      rewrite <- digit. lia. }
    assert (digit_bound : (N.of_nat (byte mod 128) < 128)%N).
    { pose proof (Nat.mod_upper_bound byte 128 ltac:(lia)) as bound. lia. }
    exists (N.of_nat (byte mod 128) + 128 * rest)%N.
    split.
    + rewrite same_result, Nat2N.inj_succ, N.pow_succ_r'.
      ring.
    + split; [intros _; lia|].
      split.
      * assert (width : next - offset = S (next - S offset)) by lia.
        rewrite width, (skipn_nth_cons nat offset bytes byte selected).
        cbn [source_varint_groups firstn].
        assert (ltb_large :
          ((N.of_nat (byte mod 128) + 128 * rest) <? 128)%N = false)
          by (apply N.ltb_ge; lia).
        rewrite ltb_large.
        assert (mod_digit :
          ((N.of_nat (byte mod 128) + 128 * rest) mod 128)%N =
            N.of_nat (byte mod 128)).
        { symmetry.
          apply (N.mod_unique _ 128 rest _ digit_bound).
          lia. }
        assert (div_digit :
          ((N.of_nat (byte mod 128) + 128 * rest) / 128)%N = rest).
        { symmetry.
          apply (N.div_unique _ 128 rest _ digit_bound).
          lia. }
        rewrite mod_digit, div_digit, Nat2N.id, encoded, digit_value.
        reflexivity.
      * lia.
Qed.

Lemma canonical_varint_reencodes :
  forall bytes region_end offset value next,
    Source.Wire.canonical_varint bytes region_end offset = Some (value, next) ->
    source_canonical_varint value = byte_slice bytes offset next /\
    offset < next.
Proof.
  intros bytes region_end offset value next read.
  unfold Source.Wire.canonical_varint in read.
  destruct (varint_groups_reencode 10 bytes region_end offset 0 0%N value next
    read) as [rest [same [_ [encoded advanced]]]].
  assert (value_is_rest : value = rest).
  { rewrite same. change (N.of_nat 0) with 0%N. rewrite N.pow_0_r. ring. }
  subst rest.
  split; [exact encoded|exact advanced].
Qed.

(** *** Every field framed by the decoder re-encodes to its own bytes *)

Lemma key_number_wire_compose :
  forall key : N,
    N.of_nat (N.to_nat (key / 8)%N * 8 + N.to_nat (key mod 8)%N) = key.
Proof.
  intro key.
  destruct (Source.Wire.binary_key_number_and_wire_exact key) as [number wire].
  rewrite number, wire.
  pose proof (Nat.div_mod_eq (N.to_nat key) 8) as decomposition.
  replace (N.to_nat key / 8 * 8 + N.to_nat key mod 8) with (N.to_nat key)
    by lia.
  apply N2Nat.id.
Qed.

Definition span_body (bytes : list nat) (span : Source.Wire.field_span)
    : list nat :=
  byte_slice bytes (Source.Wire.span_payload_start span)
    (Source.Wire.span_end span).

Definition span_reencodes (bytes : list nat) (span : Source.Wire.field_span)
    : Prop :=
  Source.Wire.span_payload_start span <= Source.Wire.span_end span /\
  source_canonical_varint
    (N.of_nat (Source.Wire.span_number span * 8 +
      source_wire_code (Source.wire_kind_of_span span))) ++
  match Source.wire_kind_of_span span with
  | Schema.LengthDelimited =>
      source_canonical_varint
        (N.of_nat (Source.Wire.span_end span -
          Source.Wire.span_payload_start span)) ++ span_body bytes span
  | _ => span_body bytes span
  end =
  byte_slice bytes (Source.Wire.span_start span) (Source.Wire.span_end span).

Lemma checked_field_reencodes :
  forall bytes region_end offset last ordered span,
    Source.Wire.checked_field bytes region_end offset last ordered =
      Some span ->
    span_reencodes bytes span.
Proof.
  intros bytes region_end offset last ordered span checked.
  unfold Source.Wire.checked_field in checked.
  destruct (Source.Wire.framing_field bytes region_end offset last ordered)
    as [field|] eqn:framed; [|discriminate checked].
  destruct ((Source.Wire.span_start field =? offset) &&
    (offset <? Source.Wire.span_end field) &&
    (Source.Wire.span_end field <=? region_end)) eqn:guards;
    [|discriminate checked].
  pose proof (some_injective _ _ _ checked) as same_field.
  subst field.
  clear guards checked.
  unfold Source.Wire.framing_field in framed.
  destruct (Source.Wire.canonical_varint bytes region_end offset)
    as [[key key_end]|] eqn:key_read; [|discriminate framed].
  destruct (canonical_varint_reencodes bytes region_end offset key key_end
    key_read) as [key_bytes key_advanced].
  destruct ((0 <? (key / 8)%N)%N &&
    Source.Wire.field_number_within_limit (key / 8)%N); [|discriminate framed].
  destruct (negb ordered || (last <=? N.to_nat (key / 8)%N));
    [|discriminate framed].
  pose proof (key_number_wire_compose key) as key_is.
  destruct (N.to_nat (key mod 8)%N) as [|[|[|[|[|[|code]]]]]].
  - (* wire type 0: varint *)
    destruct (Source.Wire.canonical_varint bytes region_end key_end)
      as [[value value_end]|] eqn:value_read; [|discriminate framed].
    pose proof (some_injective _ _ _ framed) as same_span.
    subst span.
    destruct (canonical_varint_reencodes bytes region_end key_end value
      value_end value_read) as [_ value_advanced].
    unfold span_reencodes, span_body.
    cbn [Source.Wire.span_number Source.Wire.span_wire Source.Wire.span_start
      Source.Wire.span_payload_start Source.Wire.span_end
      Source.wire_kind_of_span source_wire_code].
    rewrite key_is, key_bytes.
    split; [lia|].
    apply byte_slice_concat; lia.
  - (* wire type 1: fixed 64 *)
    destruct (key_end + 8 <=? region_end) eqn:within; [|discriminate framed].
    pose proof (some_injective _ _ _ framed) as same_span.
    subst span.
    unfold span_reencodes, span_body.
    cbn [Source.Wire.span_number Source.Wire.span_wire Source.Wire.span_start
      Source.Wire.span_payload_start Source.Wire.span_end
      Source.wire_kind_of_span source_wire_code].
    rewrite key_is, key_bytes.
    split; [lia|].
    apply byte_slice_concat; lia.
  - (* wire type 2: length delimited *)
    destruct (Source.Wire.canonical_varint bytes region_end key_end)
      as [[payload_length body_start]|] eqn:length_read; [|discriminate framed].
    destruct (N.leb payload_length (N.of_nat (region_end - body_start)))
      eqn:payload_fits; [|discriminate framed].
    pose proof (some_injective _ _ _ framed) as same_span.
    subst span.
    destruct (canonical_varint_reencodes bytes region_end key_end
      payload_length body_start length_read)
      as [length_bytes length_advanced].
    unfold span_reencodes, span_body.
    cbn [Source.Wire.span_number Source.Wire.span_wire Source.Wire.span_start
      Source.Wire.span_payload_start Source.Wire.span_end
      Source.wire_kind_of_span source_wire_code].
    replace (body_start + N.to_nat payload_length - body_start)
      with (N.to_nat payload_length) by lia.
    rewrite N2Nat.id, key_is, key_bytes, length_bytes.
    split; [lia|].
    rewrite byte_slice_concat by lia.
    apply byte_slice_concat; lia.
  - (* wire type 3: refused *)
    discriminate framed.
  - (* wire type 4: refused *)
    discriminate framed.
  - (* wire type 5: fixed 32 *)
    destruct (key_end + 4 <=? region_end) eqn:within; [|discriminate framed].
    pose proof (some_injective _ _ _ framed) as same_span.
    subst span.
    unfold span_reencodes, span_body.
    cbn [Source.Wire.span_number Source.Wire.span_wire Source.Wire.span_start
      Source.Wire.span_payload_start Source.Wire.span_end
      Source.wire_kind_of_span source_wire_code].
    rewrite key_is, key_bytes.
    split; [lia|].
    apply byte_slice_concat; lia.
  - (* wire types 6 and above: refused *)
    discriminate framed.
Qed.

(** *** The decoded spans tile the body *)

Fixpoint spans_tile (bytes : list nat) (spans : list Source.Wire.field_span)
    (offset region_end : nat) : Prop :=
  match spans with
  | [] => offset = region_end
  | span :: rest =>
      Source.Wire.span_start span = offset /\
      offset < Source.Wire.span_end span /\
      Source.Wire.span_end span <= region_end /\
      span_reencodes bytes span /\
      spans_tile bytes rest (Source.Wire.span_end span) region_end
  end.

Lemma spans_tile_member_bounds :
  forall bytes spans offset region_end span,
    spans_tile bytes spans offset region_end ->
    In span spans ->
    offset <= Source.Wire.span_start span /\
    Source.Wire.span_start span < Source.Wire.span_end span /\
    Source.Wire.span_end span <= region_end.
Proof.
  intros bytes spans.
  induction spans as [|first rest IH];
    intros offset region_end span tile member.
  - destruct member.
  - cbn [spans_tile] in tile.
    destruct tile as [start [progress [bound [_ rest_tile]]]].
    destruct member as [same|later].
    + subst first. repeat split; lia.
    + destruct (IH _ _ span rest_tile later) as [low [mid high]].
      repeat split; lia.
Qed.

Lemma reference_remaining_tiles :
  forall fuel bytes region_end offset last ordered spans,
    Source.Wire.reference_remaining fuel bytes region_end offset last ordered =
      Source.Wire.Parsed spans ->
    spans_tile bytes spans offset region_end.
Proof.
  induction fuel as [|fuel IH];
    intros bytes region_end offset last ordered spans parsed;
    cbn [Source.Wire.reference_remaining] in parsed;
    destruct (offset =? region_end) eqn:finished.
  - injection parsed as same_spans.
    subst spans.
    exact (proj1 (Nat.eqb_eq _ _) finished).
  - destruct (Source.Wire.checked_field bytes region_end offset last ordered);
      discriminate parsed.
  - injection parsed as same_spans.
    subst spans.
    exact (proj1 (Nat.eqb_eq _ _) finished).
  - destruct (Source.Wire.checked_field bytes region_end offset last ordered)
      as [span|] eqn:checked; [|discriminate parsed].
    destruct (Source.Wire.reference_remaining fuel bytes region_end
      (Source.Wire.span_end span) (Source.Wire.span_number span) ordered)
      as [suffix| |] eqn:remaining; try discriminate parsed.
    injection parsed as same_spans.
    subst spans.
    destruct (Source.Wire.checked_field_has_strict_bounded_progress _ _ _ _ _ _
      checked) as [start [progress bound]].
    cbn [spans_tile].
    exact (conj start (conj progress (conj bound (conj
      (checked_field_reencodes _ _ _ _ _ _ checked)
      (IH _ _ _ _ _ _ remaining))))).
Qed.

Lemma decode_fold_tracks_spans :
  forall program decode_child spans initial final,
    fold_left (Source.decode_source_step program decode_child) spans
      (Some initial) = Some final ->
    map Source.source_span (rev (Source.decode_reversed final)) =
    map Source.source_span (rev (Source.decode_reversed initial)) ++ spans.
Proof.
  intros program decode_child spans.
  induction spans as [|span rest IH]; intros initial final folded.
  - cbn [fold_left] in folded.
    injection folded as same.
    subst final.
    rewrite app_nil_r.
    reflexivity.
  - cbn [fold_left] in folded.
    destruct (Source.decode_source_step program decode_child (Some initial)
      span) as [next|] eqn:stepped.
    + rewrite (IH next final folded).
      unfold Source.decode_source_step in stepped.
      destruct (Source.source_descriptor program span) as [field|]
        eqn:resolved; [|discriminate stepped].
      destruct (Source.slot_admissible program initial field);
        [|discriminate stepped].
      destruct (Source.generated_source_node decode_child span field)
        as [node|] eqn:generated; [|discriminate stepped].
      inversion stepped; subst next.
      cbn [Source.decode_reversed rev].
      rewrite map_app.
      cbn [map].
      destruct (Source.generated_source_node_has_selected_constructor
        decode_child span field node generated) as [same_span _].
      rewrite same_span.
      rewrite <- app_assoc.
      reflexivity.
    + rewrite (Source.fold_left_none_stays_none _ _
        (Source.decode_source_step program decode_child) rest
        (fun member => eq_refl)) in folded.
      discriminate folded.
Qed.

Lemma decoded_region_spans :
  forall fuel bytes program start finish nodes,
    Source.decode_region fuel bytes program start finish = Some nodes ->
    spans_tile bytes (map Source.source_span nodes) start finish.
Proof.
  intros [|smaller] bytes program start finish nodes decoded;
    simpl in decoded; try discriminate decoded.
  destruct (Source.Wire.worklist_region bytes start finish false)
    as [spans| |] eqn:framed; try discriminate decoded.
  set (decode_child := fun (child_program : Schema.generated_program)
    (first last : nat) =>
      Source.decode_region smaller bytes child_program first last).
  change (option_map (fun state => rev (Source.decode_reversed state))
    (fold_left (Source.decode_source_step program decode_child) spans
      (Some (Source.DecodeAccumulator None [] []))) = Some nodes) in decoded.
  destruct (fold_left (Source.decode_source_step program decode_child) spans
    (Some (Source.DecodeAccumulator None [] []))) as [final|] eqn:folded;
    [|discriminate decoded].
  cbn [option_map] in decoded.
  injection decoded as same_nodes.
  subst nodes.
  pose proof (decode_fold_tracks_spans program decode_child spans
    (Source.DecodeAccumulator None [] []) final folded) as tracked.
  cbn [Source.decode_reversed rev map app] in tracked.
  rewrite tracked.
  assert (reference :
    Source.Wire.reference_region bytes start finish false =
      Source.Wire.Parsed spans).
  { rewrite <- Source.Wire.worklist_and_reference_agree_on_every_finite_region.
    exact framed. }
  unfold Source.Wire.reference_region in reference.
  destruct (Source.Wire.region_ok bytes start finish); [|discriminate reference].
  exact (reference_remaining_tiles _ _ _ _ _ _ _ reference).
Qed.

(** *** Tag 9 occurs at most once on a valid scan *)

Lemma source_scan_step_valid_facts :
  forall program state child,
    Admission.source_scan_valid
      (Admission.source_scan_step program state child) = true ->
    Admission.source_scan_valid state = true /\
    exists descriptor,
      Schema.resolve_tag (Schema.program_fields program)
        (Core.source_summary_tag child) = Some descriptor /\
      (Schema.descriptor_repeated descriptor ||
        negb (existsb (Nat.eqb (Schema.descriptor_slot descriptor))
          (Admission.source_scan_seen_slots state))) = true /\
      Admission.source_scan_seen_slots
        (Admission.source_scan_step program state child) =
      Schema.descriptor_slot descriptor ::
        Admission.source_scan_seen_slots state.
Proof.
  intros program state child valid.
  assert (state_valid : Admission.source_scan_valid state = true).
  { destruct (Admission.source_scan_valid state) eqn:observed; [reflexivity|].
    rewrite (Admission.source_scan_step_invalid_sticky program state child
      observed) in valid.
    rewrite observed in valid.
    discriminate valid. }
  split; [exact state_valid|].
  unfold Admission.source_scan_step in valid |- *.
  rewrite state_valid in valid |- *.
  destruct (Schema.resolve_tag (Schema.program_fields program)
    (Core.source_summary_tag child)) as [descriptor|] eqn:resolved;
    [|discriminate valid].
  exists descriptor.
  set (slot := Schema.descriptor_slot descriptor) in *.
  destruct (Admission.source_child_matches_descriptor descriptor child &&
    (slot <? Schema.program_slot_count program) &&
    Admission.source_slot_follows (Admission.source_scan_last_slot state) slot
      (Schema.descriptor_repeated descriptor) &&
    (Schema.descriptor_repeated descriptor ||
      negb (existsb (Nat.eqb slot) (Admission.source_scan_seen_slots state))))
    eqn:guard; [|discriminate valid].
  repeat rewrite andb_true_iff in guard.
  destruct guard as [_ unique].
  exact (conj eq_refl (conj unique eq_refl)).
Qed.

Lemma scan_tag_appears_at_most_once :
  forall program tag descriptor,
    Schema.resolve_tag (Schema.program_fields program) tag = Some descriptor ->
    Schema.descriptor_repeated descriptor = false ->
    forall children state,
      Admission.source_scan_valid
        (fold_left (Admission.source_scan_step program) children state) =
        true ->
      (existsb (Nat.eqb (Schema.descriptor_slot descriptor))
        (Admission.source_scan_seen_slots state) = true ->
       filter (fun child => Core.source_summary_tag child =? tag) children =
         []) /\
      length (filter (fun child => Core.source_summary_tag child =? tag)
        children) <= 1.
Proof.
  intros program tag descriptor resolved singular children.
  induction children as [|child rest IH]; intros state valid.
  - cbn [filter length].
    split; [intro seen; reflexivity|lia].
  - cbn [fold_left] in valid.
    assert (step_valid :
      Admission.source_scan_valid
        (Admission.source_scan_step program state child) = true).
    { destruct (Admission.source_scan_valid
        (Admission.source_scan_step program state child)) eqn:observed;
        [reflexivity|].
      pose proof (invalid_source_scan_stays_invalid program rest
        (Admission.source_scan_step program state child) observed)
        as impossible.
      rewrite valid in impossible.
      discriminate impossible. }
    destruct (source_scan_step_valid_facts program state child step_valid)
      as [state_valid [step_descriptor [step_resolved [unique next_seen]]]].
    destruct (IH (Admission.source_scan_step program state child) valid)
      as [rest_no_repeat rest_length].
    rewrite next_seen in rest_no_repeat.
    cbn [filter].
    destruct (Core.source_summary_tag child =? tag) eqn:tag_matches; cbv iota.
    + apply Nat.eqb_eq in tag_matches.
      rewrite tag_matches in step_resolved.
      rewrite resolved in step_resolved.
      injection step_resolved as same_descriptor.
      subst step_descriptor.
      rewrite singular in unique.
      cbn [orb] in unique.
      apply negb_true_iff in unique.
      split.
      * intro seen. rewrite unique in seen. discriminate seen.
      * assert (seen_now :
          existsb (Nat.eqb (Schema.descriptor_slot descriptor))
            (Schema.descriptor_slot descriptor ::
              Admission.source_scan_seen_slots state) = true).
        { cbn [existsb]. rewrite Nat.eqb_refl. reflexivity. }
        rewrite (rest_no_repeat seen_now).
        cbn [length].
        lia.
    + split.
      * intro seen.
        apply rest_no_repeat.
        cbn [existsb].
        rewrite seen.
        apply orb_true_r.
      * exact rest_length.
Qed.

Definition par_locally_free_is_singular : bool :=
  match Schema.lookup_projected_program 1 [] [] with
  | Some program =>
      match Schema.resolve_tag (Schema.program_fields program) 9 with
      | Some descriptor => negb (Schema.descriptor_repeated descriptor)
      | None => false
      end
  | None => false
  end.

Lemma par_locally_free_is_singular_true : par_locally_free_is_singular = true.
Proof. vm_compute. reflexivity. Qed.

Lemma par_locally_free_field_singular :
  forall program,
    Schema.lookup_projected_program 1 [] [] = Some program ->
    exists descriptor,
      Schema.resolve_tag (Schema.program_fields program) 9 = Some descriptor /\
      Schema.descriptor_repeated descriptor = false.
Proof.
  intros program looked_up.
  pose proof par_locally_free_is_singular_true as certificate.
  unfold par_locally_free_is_singular in certificate.
  rewrite looked_up in certificate.
  cbv iota in certificate.
  destruct (Schema.resolve_tag (Schema.program_fields program) 9)
    as [descriptor|] eqn:resolved; [|discriminate certificate].
  exists descriptor.
  split; [reflexivity|].
  destruct (Schema.descriptor_repeated descriptor);
    [discriminate certificate|reflexivity].
Qed.

(** *** Identity of the Par over its aligned children *)

Local Notation is_tag9_node :=
  (fun node : Source.source_tree =>
    Schema.descriptor_tag (Source.source_field node) =? 9).

Definition identity_child_bytes (value : Core.typed_source_value) : list nat :=
  if Schema.raw_tag (Core.source_value_raw value) =? 9 then []
  else source_encoded_field (Core.source_value_raw value).

Lemma identity_child_bytes_of_summary :
  forall child,
    identity_child_bytes (Core.source_summary_value child) =
    if Schema.raw_tag (Core.source_summary_raw child) =? 9 then []
    else source_encoded_field (Core.source_summary_raw child).
Proof. reflexivity. Qed.

Definition identity_tail (bytes : list nat) (nodes : list Source.source_tree)
    (offset region_end : nat) : list nat :=
  match find is_tag9_node nodes with
  | None => byte_slice bytes offset region_end
  | Some node =>
      byte_slice bytes offset (Source.Wire.span_start (Source.source_span node))
      ++ byte_slice bytes (Source.Wire.span_end (Source.source_span node))
        region_end
  end.

Lemma identity_tail_cons_tag9 :
  forall bytes node nodes offset region_end,
    (Schema.descriptor_tag (Source.source_field node) =? 9) = true ->
    identity_tail bytes (node :: nodes) offset region_end =
    byte_slice bytes offset (Source.Wire.span_start (Source.source_span node))
    ++ byte_slice bytes (Source.Wire.span_end (Source.source_span node))
      region_end.
Proof.
  intros bytes node nodes offset region_end is_nine.
  unfold identity_tail.
  cbn [find].
  rewrite is_nine.
  reflexivity.
Qed.

Lemma identity_tail_cons_other :
  forall bytes node nodes offset region_end,
    (Schema.descriptor_tag (Source.source_field node) =? 9) = false ->
    identity_tail bytes (node :: nodes) offset region_end =
    identity_tail bytes nodes offset region_end.
Proof.
  intros bytes node nodes offset region_end not_nine.
  unfold identity_tail.
  cbn [find].
  rewrite not_nine.
  reflexivity.
Qed.

Lemma identity_tail_no_tag9 :
  forall bytes nodes offset region_end,
    find is_tag9_node nodes = None ->
    identity_tail bytes nodes offset region_end =
    byte_slice bytes offset region_end.
Proof.
  intros bytes nodes offset region_end none.
  unfold identity_tail.
  rewrite none.
  reflexivity.
Qed.

Lemma find_none_of_empty_filter :
  forall (A : Type) (predicate : A -> bool) values,
    length (filter predicate values) = 0 -> find predicate values = None.
Proof.
  intros A predicate values.
  induction values as [|value rest IH]; intro empty; [reflexivity|].
  cbn [filter] in empty.
  cbn [find].
  destruct (predicate value).
  - cbn [length] in empty. discriminate empty.
  - exact (IH empty).
Qed.

Lemma aligned_filter_lengths_agree :
  forall fuel hash_fn bytes program nodes children tag,
    Forall2 (aligned_child fuel hash_fn bytes program) nodes children ->
    length (filter (fun node : Source.source_tree =>
      Schema.descriptor_tag (Source.source_field node) =? tag) nodes) =
    length (filter (fun child => Core.source_summary_tag child =? tag)
      children).
Proof.
  intros fuel hash_fn bytes program nodes children tag aligned.
  induction aligned as [|node child nodes' children' pair rest IH];
    [reflexivity|].
  destruct pair as [[selected _] evaluated].
  cbn [filter].
  pose proof (evaluated_node_retains_source_raw fuel hash_fn bytes program node
    child evaluated) as converted.
  destruct (Source.raw_field_of_source_preserves_wire_and_body bytes node
    (Core.source_summary_raw child) converted) as [raw_tag_is _].
  destruct (Source.resolved_source_descriptor_has_actual_tag_and_wire program
    (Source.source_span node) (Source.source_field node) selected)
    as [field_tag _].
  assert (same : Schema.descriptor_tag (Source.source_field node) =
      Core.source_summary_tag child).
  { change (Core.source_summary_tag child)
      with (Schema.raw_tag (Core.source_summary_raw child)).
    rewrite field_tag, raw_tag_is.
    reflexivity. }
  rewrite same.
  destruct (Core.source_summary_tag child =? tag); cbn [length];
    rewrite IH; reflexivity.
Qed.

Lemma source_body_is_span_body :
  forall bytes node,
    Source.source_body bytes node = span_body bytes (Source.source_span node).
Proof. reflexivity. Qed.

Lemma source_encoded_field_of_raw :
  forall bytes node raw,
    Source.raw_field_of_source bytes node = Some raw ->
    span_reencodes bytes (Source.source_span node) ->
    Source.Wire.span_end (Source.source_span node) <= length bytes ->
    source_encoded_field raw =
    byte_slice bytes (Source.Wire.span_start (Source.source_span node))
      (Source.Wire.span_end (Source.source_span node)).
Proof.
  intros bytes node raw converted [payload_le reencodes] end_within.
  destruct (Source.raw_field_of_source_preserves_wire_and_body bytes node raw
    converted) as [tag_eq [wire_eq [_ [body_eq _]]]].
  assert (body_length :
    length (span_body bytes (Source.source_span node)) =
      Source.Wire.span_end (Source.source_span node) -
      Source.Wire.span_payload_start (Source.source_span node)).
  { rewrite <- source_body_is_span_body.
    exact (Source.bounded_source_body_has_exact_length bytes node payload_le
      end_within). }
  unfold source_encoded_field.
  rewrite tag_eq, wire_eq, body_eq, source_body_is_span_body, body_length.
  exact reencodes.
Qed.

Lemma identity_bytes_over_aligned_children :
  forall fuel hash_fn bytes program nodes children region_end,
    Forall2 (aligned_child fuel hash_fn bytes program) nodes children ->
    region_end <= length bytes ->
    forall offset,
      spans_tile bytes (map Source.source_span nodes) offset region_end ->
      length (filter is_tag9_node nodes) <= 1 ->
      flat_map identity_child_bytes (map Core.source_summary_value children) =
      identity_tail bytes nodes offset region_end.
Proof.
  intros fuel hash_fn bytes program nodes children region_end aligned end_within.
  induction aligned as [|node child nodes' children' pair rest IH];
    intros offset tile unique.
  - cbn [map spans_tile] in tile.
    subst offset.
    unfold identity_tail.
    cbn [find flat_map map].
    unfold byte_slice.
    rewrite Nat.sub_diag.
    reflexivity.
  - destruct pair as [[selected _] evaluated].
    cbn [map spans_tile] in tile.
    destruct tile as [span_start_is [span_progress [span_end_bound
      [reencodes rest_tile]]]].
    pose proof (evaluated_node_retains_source_raw fuel hash_fn bytes program
      node child evaluated) as converted.
    destruct (Source.raw_field_of_source_preserves_wire_and_body bytes node
      (Core.source_summary_raw child) converted) as [raw_tag_is _].
    destruct (Source.resolved_source_descriptor_has_actual_tag_and_wire program
      (Source.source_span node) (Source.source_field node) selected)
      as [field_tag _].
    assert (same_tag : Schema.descriptor_tag (Source.source_field node) =
        Schema.raw_tag (Core.source_summary_raw child)).
    { rewrite field_tag, raw_tag_is. reflexivity. }
    assert (end_fits :
      Source.Wire.span_end (Source.source_span node) <= length bytes) by lia.
    cbn [map flat_map].
    rewrite identity_child_bytes_of_summary.
    rewrite <- same_tag.
    cbn [filter] in unique.
    destruct (Schema.descriptor_tag (Source.source_field node) =? 9)
      eqn:is_nine.
    + (* the locally_free field: both sides omit it *)
      cbn [length] in unique.
      assert (no_later : find is_tag9_node nodes' = None).
      { apply find_none_of_empty_filter. lia. }
      rewrite (identity_tail_cons_tag9 bytes node nodes' offset region_end
        is_nine).
      cbn [app].
      rewrite (IH (Source.Wire.span_end (Source.source_span node)) rest_tile
        ltac:(lia)).
      rewrite (identity_tail_no_tag9 bytes nodes'
        (Source.Wire.span_end (Source.source_span node)) region_end no_later).
      rewrite span_start_is, byte_slice_empty.
      reflexivity.
    + (* any other field: its re-encoding is its own bytes *)
      cbn [length] in unique.
      rewrite (identity_tail_cons_other bytes node nodes' offset region_end
        is_nine).
      cbn [app].
      rewrite (source_encoded_field_of_raw bytes node
        (Core.source_summary_raw child) converted reencodes end_fits).
      rewrite span_start_is.
      rewrite (IH (Source.Wire.span_end (Source.source_span node)) rest_tile
        ltac:(lia)).
      unfold identity_tail.
      destruct (find is_tag9_node nodes') as [later|] eqn:later_found;
        cbv iota.
      * apply find_some in later_found as [later_member _].
        destruct (spans_tile_member_bounds bytes (map Source.source_span nodes')
          (Source.Wire.span_end (Source.source_span node)) region_end
          (Source.source_span later) rest_tile
          (in_map Source.source_span nodes' later later_member))
          as [end_le_start _].
        rewrite app_assoc.
        rewrite byte_slice_concat by lia.
        reflexivity.
      * apply byte_slice_concat; lia.
Qed.

Lemma par_identity_bytes_unfold :
  forall body program nodes,
    Schema.lookup_projected_program 1 [] [] = Some program ->
    Source.decode_region (S (length body)) body program 0 (length body) =
      Some nodes ->
    par_identity_bytes body =
    match find is_tag9_node nodes with
    | None => body
    | Some node =>
        firstn (Source.Wire.span_start (Source.source_span node)) body ++
        skipn (Source.Wire.span_end (Source.source_span node)) body
    end.
Proof.
  intros body program nodes looked_up decoded.
  unfold par_identity_bytes.
  rewrite looked_up.
  cbv iota.
  rewrite decoded.
  cbv iota.
  destruct (find is_tag9_node nodes) as [node|]; reflexivity.
Qed.

Lemma par_identity_of_aligned_children :
  forall fuel hash_fn body program nodes children,
    Schema.lookup_projected_program 1 [] [] = Some program ->
    Source.decode_region (S (length body)) body program 0 (length body) =
      Some nodes ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true ->
    par_identity_bytes body =
    flat_map identity_child_bytes (map Core.source_summary_value children).
Proof.
  intros fuel hash_fn body program nodes children looked_up decoded aligned
    valid.
  destruct (par_locally_free_field_singular program looked_up)
    as [descriptor [resolved singular]].
  destruct (scan_tag_appears_at_most_once program 9 descriptor resolved
    singular children Admission.source_empty_scan valid) as [_ at_most_one].
  assert (nodes_at_most_one : length (filter is_tag9_node nodes) <= 1).
  { rewrite (aligned_filter_lengths_agree fuel hash_fn body program nodes
      children 9 aligned).
    exact at_most_one. }
  pose proof (decoded_region_spans _ _ _ _ _ _ decoded) as tile.
  pose proof (identity_bytes_over_aligned_children fuel hash_fn body program
    nodes children (length body) aligned (Nat.le_refl _) 0 tile
    nodes_at_most_one) as identity.
  rewrite (par_identity_bytes_unfold body program nodes looked_up decoded).
  rewrite identity.
  unfold identity_tail.
  destruct (find is_tag9_node nodes) as [node|] eqn:found; cbv iota.
  - apply find_some in found as [member _].
    destruct (spans_tile_member_bounds body (map Source.source_span nodes) 0
      (length body) (Source.source_span node) tile
      (in_map Source.source_span nodes node member)) as [_ [_ end_bound]].
    unfold byte_slice.
    rewrite Nat.sub_0_r.
    cbn [skipn].
    rewrite (firstn_all2 (skipn (Source.Wire.span_end (Source.source_span node))
      body)) by (rewrite length_skipn; lia).
    reflexivity.
  - unfold byte_slice.
    rewrite Nat.sub_0_r.
    cbn [skipn].
    rewrite firstn_all.
    reflexivity.
Qed.

(** ** The KeyValuePair carrier *)

(** A present typed score of a child records that the child is an unrefused
    message of that type.  The carrier's key identity needs all three facts. *)
Lemma typed_message_score_shape :
  forall type_id child score,
    source_payload_typed_message_score type_id child = Some score ->
    Core.source_summary_refused child = false /\
    Core.source_summary_type_id child = Some type_id /\
    value_is_message (Core.source_summary_value child) = true.
Proof.
  intros type_id child score typed.
  unfold source_payload_typed_message_score in typed.
  destruct (Core.source_summary_value child); try discriminate typed.
  destruct (Core.source_summary_type_id child) as [observed|];
    [|discriminate typed].
  destruct ((observed =? type_id) && negb (Core.source_summary_refused child))
    eqn:accepted; [|discriminate typed].
  apply andb_true_iff in accepted as [same_type unrefused].
  apply Nat.eqb_eq in same_type.
  apply negb_true_iff in unrefused.
  subst observed.
  repeat split; [exact unrefused].
Qed.

(** An unrefused message summary came from a valid scan of its children: the
    scan validity is one of the disjuncts of the refusal. *)
Lemma unrefused_message_summary_has_valid_scan :
  forall raw program children,
    Core.source_summary_refused
      (Oracle.source_message_summary raw program children) = false ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true.
Proof.
  intros raw program children accepted.
  destruct (Admission.source_scan_valid
    (Admission.source_scan_children program children)) eqn:scan;
    [reflexivity|].
  unfold Core.source_summary_refused, Oracle.source_message_summary in accepted.
  cbn [Core.source_summary_analysis Core.source_family_refused] in accepted.
  rewrite scan in accepted.
  cbn [negb] in accepted.
  repeat rewrite orb_true_r in accepted.
  discriminate accepted.
Qed.

Lemma key_identity_agrees :
  forall fuel hash_fn body program node key score,
    aligned_child fuel hash_fn body program node key ->
    source_payload_typed_message_score 1 key = Some score ->
    par_identity_bytes (Schema.raw_body (Core.source_summary_raw key)) =
    source_par_identity_bytes (Core.source_summary_value key).
Proof.
  intros fuel hash_fn body program node key score [matched evaluated] typed.
  destruct (typed_message_score_shape 1 key score typed)
    as [unrefused [key_type is_message]].
  destruct node as [span field|span field nested|span field].
  - (* a scalar node has no score of type 1 *)
    exfalso.
    destruct (accepted_source_scalar_node_has_typed_value
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program span
      field key evaluated) as [raw [scalar [_ [value_shape payload]]]].
    destruct (typed_scalar_child_is_not_message key raw scalar value_shape
      payload) as [_ not_value].
    rewrite is_message in not_value.
    discriminate not_value.
  - (* the message node: the child's own evaluation *)
    unfold Oracle.source_evaluate_node in evaluated.
    destruct (Source.raw_field_of_source body
      (Source.SourceMessage span field nested)) as [raw|] eqn:converted;
      [|discriminate evaluated].
    change (Oracle.source_evaluate_message fuel hash_fn raw = Some key)
      in evaluated.
    destruct fuel as [|smaller]; [discriminate evaluated|].
    destruct (accepted_independent_message_has_generated_children smaller
      hash_fn raw key evaluated)
      as [type_id [par_program [nodes [children
        [_ [raw_type [looked_up [decoded [shape aligned]]]]]]]]].
    assert (key_value : Core.source_summary_value key =
        Core.TypedSourceMessage raw (map Core.source_summary_value children)).
    { rewrite shape. reflexivity. }
    assert (key_raw : Core.source_summary_raw key = raw).
    { unfold Core.source_summary_raw. rewrite key_value. reflexivity. }
    assert (is_par : type_id = 1).
    { unfold Core.source_summary_type_id in key_type.
      rewrite key_raw, raw_type in key_type.
      injection key_type as same_type.
      exact same_type. }
    subst type_id.
    assert (scan_valid :
      Admission.source_scan_valid
        (Admission.source_scan_children par_program children) = true).
    { apply (unrefused_message_summary_has_valid_scan raw par_program children).
      rewrite <- shape.
      exact unrefused. }
    rewrite key_value, key_raw.
    exact (par_identity_of_aligned_children smaller hash_fn (Schema.raw_body raw)
      par_program nodes children looked_up decoded aligned scan_valid).
  - (* an auxiliary node is no message *)
    exfalso.
    destruct (evaluated_auxiliary_node_is_not_message
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program span
      field key evaluated) as [_ not_value].
    rewrite is_message in not_value.
    discriminate not_value.
Qed.

Lemma kvp_carrier_projects :
  forall fuel hash_fn program body nodes children,
    Schema.lookup_projected_program 34 [] [] = Some program ->
    Forall2 (aligned_child fuel hash_fn body program) nodes children ->
    exact_kvp_carrier (map Projection.project_source_summary children) =
    option_map Core.project_score (source_kvp_carrier children).
Proof.
  intros fuel hash_fn program body nodes children looked_up aligned.
  unfold exact_kvp_carrier, source_kvp_carrier, source_payload_child_at.
  rewrite <- Projection.projected_find_tag.
  destruct (find (fun child => Core.source_summary_tag child =? 1) children)
    as [key|] eqn:found; cbn [option_map]; [|reflexivity].
  apply find_some in found as [member _].
  destruct (aligned_child_of_member fuel hash_fn body program nodes children
    key aligned member) as [node pair].
  rewrite (aligned_child_typed_message_score_projects fuel hash_fn 34 program
    body node key 1 looked_up pair).
  destruct (source_payload_typed_message_score 1 key) as [score|] eqn:typed;
    cbn [option_map]; [|reflexivity].
  change (sem_body (Projection.project_source_summary key))
    with (Schema.raw_body (Core.source_summary_raw key)).
  rewrite (key_identity_agrees fuel hash_fn body program node key score pair
    typed).
  rewrite Core.project_source_score_constructor.
  reflexivity.
Qed.

(** ** The bridge *)

Theorem emap_payload_projects :
  forall fuel hash_fn program body nodes children,
    (Schema.lookup_projected_program 30 [] [] = Some program ->
     Source.decode_region (S (length body)) body program 0 (length body) =
       Some nodes ->
     Source.sequence_source_results
       (map (Oracle.source_evaluate_node
              (Oracle.source_evaluate_message fuel hash_fn) hash_fn body
              program) nodes) = Some children ->
     Admission.source_scan_valid
       (Admission.source_scan_children program children) = true ->
     exact_emap_score (map Projection.project_source_summary children) =
     option_map Core.project_score (source_emap_score children) /\
     expr_payload_completeness 30
       (fold_left (Target.scan_child program)
         (map Projection.project_source_summary children) Schema.empty_scan)
       (map Projection.project_source_summary children) =
     source_expr_payload_completeness 30
       (Admission.source_scan_children program children) children) /\
    (Schema.lookup_projected_program 34 [] [] = Some program ->
     Source.decode_region (S (length body)) body program 0 (length body) =
       Some nodes ->
     Source.sequence_source_results
       (map (Oracle.source_evaluate_node
              (Oracle.source_evaluate_message fuel hash_fn) hash_fn body
              program) nodes) = Some children ->
     Admission.source_scan_valid
       (Admission.source_scan_children program children) = true ->
     exact_kvp_carrier (map Projection.project_source_summary children) =
     option_map Core.project_score (source_kvp_carrier children) /\
     expr_payload_completeness 34
       (fold_left (Target.scan_child program)
         (map Projection.project_source_summary children) Schema.empty_scan)
       (map Projection.project_source_summary children) =
     source_expr_payload_completeness 34
       (Admission.source_scan_children program children) children).
Proof.
  intros fuel hash_fn program body nodes children.
  split; intros looked_up decoded selected valid.
  all: assert (aligned :
    Forall2 (aligned_child fuel hash_fn body program) nodes children) by
    exact (generated_selected_children_are_aligned program
      (Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
        hash_fn body program) nodes children
      (Source.decoded_region_nodes_match_generated_source _ _ _ _ _ _ decoded)
      (RecursiveProjection.selected_source_evaluator_outputs_are_positional
        _ _ _ _ _ selected)).
  - assert (unrefused :
      Forall (fun child => Core.source_summary_refused child = false) children).
    { unfold Admission.source_scan_children in valid.
      exact (valid_source_scan_children_are_unrefused program children
        Admission.source_empty_scan valid). }
    assert (certificate : flag_tag_scalar_in_family 30 4 = true)
      by (vm_compute; reflexivity).
    assert (contract : Admission.source_scalar_contract_for 30 4 =
        Some (Admission.SourceScalarContract Data.PBBool true))
      by (vm_compute; reflexivity).
    split.
    + exact (emap_score_projects fuel hash_fn program body nodes children
        certificate contract looked_up aligned unrefused).
    + cbn [expr_payload_completeness source_expr_payload_completeness].
      f_equal.
      clear - children.
      induction children as [|child rest IH]; [reflexivity|].
      cbn [map forallb].
      rewrite IH.
      reflexivity.
  - split.
    + exact (kvp_carrier_projects fuel hash_fn program body nodes children
        looked_up aligned).
    + cbn [expr_payload_completeness source_expr_payload_completeness].
      rewrite (projected_seen_slot program children 0 valid).
      rewrite (projected_seen_slot program children 1 valid).
      reflexivity.
Qed.

(** ** Red control: a carrier that includes the value score

    The mutant reads the score of tag 2 and adds it to the carrier evidence.
    The witness is a pair whose key and value are both the empty Par; the key
    and value scores exist, the identity bytes are empty, and the two carriers
    have different numbers of components. *)
Definition mutant_kvp_carrier_with_value (children : list semantic_summary)
    : option canonical_score :=
  match find (fun child => sem_tag child =? 1) children,
      find (fun child => sem_tag child =? 2) children with
  | Some key, Some value =>
      match exact_typed_message_score 1 key, exact_typed_message_score 1 value with
      | Some key_score, Some value_score =>
          Some (score_constructor 0
            [key_score; value_score;
             ScoreBytes (par_identity_bytes (sem_body key))])
      | _, _ => None
      end
  | _, _ => None
  end.

Definition kvp_witness_body : list nat := [10; 0; 18; 0].

Definition kvp_witness_children : list Core.source_exact_summary :=
  Correspondence.witness_children 34 kvp_witness_body.

Lemma kvp_witness_satisfies_the_hypotheses :
  Schema.lookup_projected_program 34 [] [] =
    Some (Correspondence.witness_program 34) /\
  Source.decode_region (S (length kvp_witness_body)) kvp_witness_body
    (Correspondence.witness_program 34) 0 (length kvp_witness_body) =
    Some (Correspondence.witness_nodes 34 kvp_witness_body) /\
  Source.sequence_source_results
    (map (Oracle.source_evaluate_node
           (Oracle.source_evaluate_message 1 Correspondence.witness_hash_fn)
           Correspondence.witness_hash_fn kvp_witness_body
           (Correspondence.witness_program 34))
      (Correspondence.witness_nodes 34 kvp_witness_body)) =
    Some kvp_witness_children /\
  Admission.source_scan_valid
    (Admission.source_scan_children (Correspondence.witness_program 34)
      kvp_witness_children) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example value_including_carrier_disagrees_with_the_oracle :
  mutant_kvp_carrier_with_value
    (map Projection.project_source_summary kvp_witness_children) <>
  option_map Core.project_score (source_kvp_carrier kvp_witness_children).
Proof. vm_compute. discriminate. Qed.

Example real_carrier_agrees_on_the_red_control_witness :
  exists score,
    exact_kvp_carrier
      (map Projection.project_source_summary kvp_witness_children) = Some score /\
    option_map Core.project_score (source_kvp_carrier kvp_witness_children) =
      Some score.
Proof. eexists. split; vm_compute; reflexivity. Qed.

Theorem value_including_carrier_fails_the_bridge :
  ~ (forall fuel hash_fn program body nodes children,
      Schema.lookup_projected_program 34 [] [] = Some program ->
      Source.decode_region (S (length body)) body program 0 (length body) =
        Some nodes ->
      Source.sequence_source_results
        (map (Oracle.source_evaluate_node
               (Oracle.source_evaluate_message fuel hash_fn) hash_fn body
               program) nodes) = Some children ->
      Admission.source_scan_valid
        (Admission.source_scan_children program children) = true ->
      mutant_kvp_carrier_with_value
        (map Projection.project_source_summary children) =
      option_map Core.project_score (source_kvp_carrier children)).
Proof.
  intro bridge.
  destruct kvp_witness_satisfies_the_hypotheses
    as [looked_up [decoded [selected scan_valid]]].
  exact (value_including_carrier_disagrees_with_the_oracle
    (bridge 1 Correspondence.witness_hash_fn
      (Correspondence.witness_program 34) kvp_witness_body
      (Correspondence.witness_nodes 34 kvp_witness_body) kvp_witness_children
      looked_up decoded selected scan_valid)).
Qed.

(** ** Witnesses for the EMap arm

    The empty EMap satisfies every hypothesis and has a score on both sides, so
    the EMap arm of the bridge is not vacuous.  An EMap with one pair child
    satisfies the hypotheses at fuel 2, and neither side has a score: the
    compiled reducers do not yet dispatch the KeyValuePair carrier, so the
    child carries no evidence.  This is why the value-score red control above
    sits at the carrier, type 34, the first place a value score is observable. *)

Definition empty_emap_body : list nat := [].

Definition empty_emap_children : list Core.source_exact_summary :=
  Correspondence.witness_children 30 empty_emap_body.

Lemma empty_emap_witness_satisfies_the_hypotheses :
  Schema.lookup_projected_program 30 [] [] =
    Some (Correspondence.witness_program 30) /\
  Source.decode_region (S (length empty_emap_body)) empty_emap_body
    (Correspondence.witness_program 30) 0 (length empty_emap_body) =
    Some (Correspondence.witness_nodes 30 empty_emap_body) /\
  Source.sequence_source_results
    (map (Oracle.source_evaluate_node
           (Oracle.source_evaluate_message 1 Correspondence.witness_hash_fn)
           Correspondence.witness_hash_fn empty_emap_body
           (Correspondence.witness_program 30))
      (Correspondence.witness_nodes 30 empty_emap_body)) =
    Some empty_emap_children /\
  Admission.source_scan_valid
    (Admission.source_scan_children (Correspondence.witness_program 30)
      empty_emap_children) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example empty_emap_scores_agree :
  exists score,
    exact_emap_score
      (map Projection.project_source_summary empty_emap_children) = Some score /\
    option_map Core.project_score (source_emap_score empty_emap_children) =
      Some score.
Proof. eexists. split; vm_compute; reflexivity. Qed.

(** Field 1, length 4, holding a pair whose key and value are empty Pars. *)
Definition one_pair_emap_body : list nat := [10; 4; 10; 0; 18; 0].

Definition one_pair_emap_nodes : list Source.source_tree :=
  match Source.decode_region (S (length one_pair_emap_body)) one_pair_emap_body
      (Correspondence.witness_program 30) 0 (length one_pair_emap_body) with
  | Some nodes => nodes
  | None => []
  end.

Definition one_pair_emap_children : list Core.source_exact_summary :=
  match Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message 2 Correspondence.witness_hash_fn)
             Correspondence.witness_hash_fn one_pair_emap_body
             (Correspondence.witness_program 30))
        one_pair_emap_nodes) with
  | Some children => children
  | None => []
  end.

Lemma one_pair_emap_witness_satisfies_the_hypotheses :
  Schema.lookup_projected_program 30 [] [] =
    Some (Correspondence.witness_program 30) /\
  Source.decode_region (S (length one_pair_emap_body)) one_pair_emap_body
    (Correspondence.witness_program 30) 0 (length one_pair_emap_body) =
    Some one_pair_emap_nodes /\
  Source.sequence_source_results
    (map (Oracle.source_evaluate_node
           (Oracle.source_evaluate_message 2 Correspondence.witness_hash_fn)
           Correspondence.witness_hash_fn one_pair_emap_body
           (Correspondence.witness_program 30))
      one_pair_emap_nodes) =
    Some one_pair_emap_children /\
  length one_pair_emap_children = 1 /\
  Admission.source_scan_valid
    (Admission.source_scan_children (Correspondence.witness_program 30)
      one_pair_emap_children) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** The pair's carrier is scored once the dispatch reaches it, so the one-pair
    EMap is scored on both sides, with the target equal to the projected
    oracle. *)
Example one_pair_emap_is_scored_on_both_sides :
  exists score,
    exact_emap_score
      (map Projection.project_source_summary one_pair_emap_children) =
      Some score /\
    option_map Core.project_score (source_emap_score one_pair_emap_children) =
      Some score.
Proof. eexists. split; vm_compute; reflexivity. Qed.

Print Assumptions payload_typed_message_score_is_process.
Print Assumptions aligned_child_typed_message_score_projects.
Print Assumptions aligned_child_of_member.
Print Assumptions payload_typed_optional_score_or_projects.
Print Assumptions payload_bool_or_absent_projects.
Print Assumptions projected_seen_slot.
Print Assumptions absent_remainder_score_is_projected.
Print Assumptions kvp_carrier_parts_project.
Print Assumptions emap_key_scores_project.
Print Assumptions emap_score_projects.
Print Assumptions some_injective.
Print Assumptions firstn_concat_skipn.
Print Assumptions byte_slice_concat.
Print Assumptions byte_slice_empty.
Print Assumptions skipn_nth_cons.
Print Assumptions varint_groups_reencode.
Print Assumptions canonical_varint_reencodes.
Print Assumptions key_number_wire_compose.
Print Assumptions checked_field_reencodes.
Print Assumptions spans_tile_member_bounds.
Print Assumptions reference_remaining_tiles.
Print Assumptions decode_fold_tracks_spans.
Print Assumptions decoded_region_spans.
Print Assumptions source_scan_step_valid_facts.
Print Assumptions scan_tag_appears_at_most_once.
Print Assumptions par_locally_free_is_singular_true.
Print Assumptions par_locally_free_field_singular.
Print Assumptions identity_child_bytes_of_summary.
Print Assumptions identity_tail_cons_tag9.
Print Assumptions identity_tail_cons_other.
Print Assumptions identity_tail_no_tag9.
Print Assumptions find_none_of_empty_filter.
Print Assumptions aligned_filter_lengths_agree.
Print Assumptions source_body_is_span_body.
Print Assumptions source_encoded_field_of_raw.
Print Assumptions identity_bytes_over_aligned_children.
Print Assumptions par_identity_bytes_unfold.
Print Assumptions par_identity_of_aligned_children.
Print Assumptions typed_message_score_shape.
Print Assumptions unrefused_message_summary_has_valid_scan.
Print Assumptions key_identity_agrees.
Print Assumptions kvp_carrier_projects.
Print Assumptions emap_payload_projects.
Print Assumptions kvp_witness_satisfies_the_hypotheses.
Print Assumptions value_including_carrier_disagrees_with_the_oracle.
Print Assumptions real_carrier_agrees_on_the_red_control_witness.
Print Assumptions value_including_carrier_fails_the_bridge.
Print Assumptions empty_emap_witness_satisfies_the_hypotheses.
Print Assumptions empty_emap_scores_agree.
Print Assumptions one_pair_emap_witness_satisfies_the_hypotheses.
Print Assumptions one_pair_emap_is_scored_on_both_sides.
