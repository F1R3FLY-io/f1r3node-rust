From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import EPM1 SubstitutionRegionFraming
  RPB3WireCursor RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3SourceScalarReduction
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Scalar := RPB3SourceScalarReduction.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Wire := RPB3WireCursor.
Module Frame := SubstitutionRegionFraming.SubstitutionRegionFraming.

Lemma cursor_skipn_cons :
  forall (bytes : list nat) (offset head : nat) (rest : list nat),
    skipn offset bytes = head :: rest ->
    nth_error bytes offset = Some head /\
    skipn (S offset) bytes = rest.
Proof.
  intros bytes offset head rest source.
  revert bytes head rest source.
  induction offset as [|offset IH]; intros bytes head rest source;
    destruct bytes as [|first bytes]; try discriminate.
  - cbn in source |- *. inversion source; subst. auto.
  - cbn in source |- *. apply IH in source. exact source.
Qed.

Lemma cursor_nth_error_cons :
  forall (bytes : list nat) (offset head : nat),
    nth_error bytes offset = Some head ->
    skipn offset bytes = head :: skipn (S offset) bytes.
Proof.
  intros bytes offset head found.
  revert bytes head found.
  induction offset as [|offset IH]; intros bytes head found;
    destruct bytes as [|first bytes]; try discriminate.
  - cbn in found |- *. now inversion found.
  - cbn in found |- *. now apply IH.
Qed.

Lemma epm_positive_value_has_width :
  forall body value,
    EPM1.decode_varint body = Some (value, []) ->
    0 < value ->
    (128 ^ N.of_nat (length body - 1) <= N.of_nat value)%N.
Proof.
  induction body as [|head tail IH]; intros value parsed positive.
  - rewrite EPM1.decode_varint_nil in parsed. discriminate.
  - rewrite EPM1.decode_varint_cons in parsed.
    destruct (head <? 128) eqn:small.
    + inversion parsed; subst. cbn. lia.
    + destruct (head <? 256) eqn:physical; [|discriminate].
      destruct (EPM1.decode_varint tail) as [[quotient suffix]|]
        eqn:child; [|discriminate].
      destruct (quotient =? 0) eqn:nonzero; [discriminate|].
      assert (reconstruction : value = head - 128 + 128 * quotient)
        by now inversion parsed.
      assert (empty : suffix = []) by now inversion parsed.
      subst suffix.
      apply Nat.eqb_neq in nonzero.
      destruct tail as [|next tail'];
        [rewrite EPM1.decode_varint_nil in child; discriminate|].
      specialize (IH quotient eq_refl ltac:(lia)).
      replace (length (head :: next :: tail') - 1)
        with (S (length tail')) by reflexivity.
      replace (length (next :: tail') - 1)
        with (length tail') in IH
        by (simpl; lia).
      rewrite Nat2N.inj_succ.
      rewrite N.pow_succ_r by lia.
      rewrite reconstruction.
      rewrite Nat2N.inj_add, Nat2N.inj_mul.
      nia.
Qed.

Lemma epm_u64_body_has_at_most_ten_bytes :
  forall body value,
    EPM1.decode_varint body = Some (value, []) ->
    0 < value ->
    (N.of_nat value < 18446744073709551616)%N ->
    length body <= 10.
Proof.
  intros body value parsed positive bounded.
  pose proof (epm_positive_value_has_width body value parsed positive)
    as lower.
  destruct (le_gt_dec (length body) 10) as [short|long]; [exact short|].
  assert (power : (128 ^ 10 <=
      128 ^ N.of_nat (length body - 1))%N).
  { apply N.pow_le_mono_r; lia. }
  assert (overflow : (18446744073709551615 < 128 ^ 10)%N)
    by (vm_compute; reflexivity).
  lia.
Qed.

Lemma epm_full_u64_to_wire_groups :
  forall body decoded bytes region_end offset group accumulator fuel,
    EPM1.decode_varint body = Some (decoded, []) ->
    skipn offset bytes = body ->
    group + length body <= 10 ->
    length body <= fuel ->
    offset + length body <= region_end ->
    (accumulator + N.of_nat decoded * 128 ^ N.of_nat group <
      18446744073709551616)%N ->
    (group > 0 -> decoded > 0) ->
    Wire.varint_groups fuel bytes region_end offset group accumulator =
      Some ((accumulator + N.of_nat decoded *
        128 ^ N.of_nat group)%N, offset + length body).
Proof.
  induction body as [|head tail IH];
    intros decoded bytes region_end offset group accumulator fuel
      parsed source width budget region bounded nonzero.
  - rewrite EPM1.decode_varint_nil in parsed. discriminate.
  - destruct fuel as [|fuel]; [cbn in budget; lia|].
    destruct (cursor_skipn_cons bytes offset head tail source)
      as [at_offset tail_source].
    assert (inside : (offset <? region_end) = true)
      by (apply Nat.ltb_lt; cbn in region; lia).
    rewrite EPM1.decode_varint_cons in parsed.
    cbn [Wire.varint_groups]. rewrite inside, at_offset.
    destruct (head <? 128) eqn:small.
    + inversion parsed; subst decoded tail.
      assert (physical : Wire.byte_ok head = true)
        by (unfold Wire.byte_ok; apply Nat.ltb_lt;
            apply Nat.ltb_lt in small; lia).
      rewrite physical.
      assert (last_guard : (group =? 9) && (1 <? head) = false).
      { destruct (group =? 9) eqn:last; [|reflexivity].
        apply Nat.eqb_eq in last; subst group.
        assert (head <= 1).
        { change (128 ^ N.of_nat 9)%N with 9223372036854775808%N
            in bounded.
          assert ((N.of_nat head <= 1)%N) by nia.
          lia. }
        destruct head as [|[|higher]]; try reflexivity; lia. }
      rewrite last_guard.
      assert (minimal : (0 <? group) && (head =? 0) = false).
      { destruct (0 <? group) eqn:positive_group; [|reflexivity].
        apply Nat.ltb_lt in positive_group.
        apply nonzero in positive_group.
        apply andb_false_iff; right.
        apply Nat.eqb_neq. lia. }
      rewrite minimal.
      rewrite Nat.mod_small by (apply Nat.ltb_lt; exact small).
      replace (offset + length (head :: [])) with (S offset)
        by (cbn; lia).
      reflexivity.
    + destruct (head <? 256) eqn:physical; [|discriminate].
      destruct (EPM1.decode_varint tail) as [[quotient suffix]|]
        eqn:child; [|discriminate].
      destruct (quotient =? 0) eqn:quotient_nonzero;
        [discriminate|].
      assert (tail_nonempty : tail <> []).
      { intro empty; subst tail.
        rewrite empty in child.
        rewrite EPM1.decode_varint_nil in child. discriminate. }
      assert (decoded_formula : decoded = head - 128 + 128 * quotient)
        by now inversion parsed.
      assert (suffix_empty : suffix = []) by now inversion parsed.
      subst suffix.
      assert (tail_positive : quotient > 0)
        by (apply Nat.eqb_neq in quotient_nonzero; lia).
      assert (earlier : group < 9).
      { destruct tail; [contradiction|]. cbn in width. lia. }
      assert (byte_bound : Wire.byte_ok head = true)
        by (unfold Wire.byte_ok; exact physical).
      assert (last_guard : (group =? 9) && (1 <? head) = false).
      { apply andb_false_iff. left. apply Nat.eqb_neq. lia. }
      rewrite byte_bound, last_guard.
      assert (digit : head mod 128 = head - 128).
      { apply Nat.ltb_ge in small. apply Nat.ltb_lt in physical.
        symmetry. apply Nat.mod_unique with (q := 1); lia. }
      set (next := (accumulator +
        N.of_nat (head mod 128) * 128 ^ N.of_nat group)%N).
      assert (next_bound :
        (next + N.of_nat quotient * 128 ^ N.of_nat (S group) <
          18446744073709551616)%N).
      { unfold next. rewrite digit.
        rewrite decoded_formula, Nat2N.inj_add, Nat2N.inj_mul
          in bounded.
        rewrite Nat2N.inj_succ.
        rewrite N.pow_succ_r by lia.
        nia. }
      specialize (IH quotient bytes region_end (S offset) (S group)
        next fuel eq_refl tail_source ltac:(cbn in width |- *; lia)
        ltac:(cbn in budget |- *; lia)
        ltac:(cbn in region |- *; lia) next_bound
        ltac:(intros; exact tail_positive)).
      rewrite IH.
      unfold next. rewrite digit, decoded_formula,
        Nat2N.inj_add, Nat2N.inj_mul, Nat2N.inj_succ.
      rewrite N.pow_succ_r by lia.
      replace (offset + length (head :: tail))
        with (S offset + length tail) by (cbn; lia).
      f_equal.
      f_equal.
      nia.
Qed.

Lemma epm_full_u64_to_wire :
  forall body decoded,
    EPM1.decode_varint body = Some (decoded, []) ->
    (N.of_nat decoded < 18446744073709551616)%N ->
    Wire.canonical_varint body (length body) 0 =
      Some (N.of_nat decoded, length body).
Proof.
  intros body decoded parsed bounded.
  destruct (Nat.eq_dec decoded 0) as [zero|positive].
  - subst decoded.
    apply Frame.successful_varint_reencodes_consumed_prefix in parsed.
    change (EPM1.encode_varint 0) with [0] in parsed.
    cbn [app] in parsed. subst body. reflexivity.
  - assert (positive_value : decoded > 0) by lia.
    pose proof (epm_u64_body_has_at_most_ten_bytes
      body decoded parsed positive_value bounded) as width.
    unfold Wire.canonical_varint.
    replace (N.of_nat decoded) with
      (0 + N.of_nat decoded * 128 ^ N.of_nat 0)%N
      by nia.
    replace (length body) with (0 + length body) at 2 by lia.
    eapply epm_full_u64_to_wire_groups;
      try eassumption; try reflexivity; lia.
Qed.

Lemma wire_groups_to_epm :
  forall fuel bytes region_end offset group accumulator number stop,
    group + fuel = 10 ->
    (accumulator < 128 ^ N.of_nat group)%N ->
    Wire.varint_groups fuel bytes region_end offset group accumulator =
      Some (number, stop) ->
    exists decoded,
      EPM1.decode_varint (skipn offset bytes) =
        Some (decoded, skipn stop bytes) /\
      number = (accumulator + N.of_nat decoded *
        128 ^ N.of_nat group)%N /\
      (number < 18446744073709551616)%N /\
      stop <= region_end /\
      (group > 0 -> decoded > 0).
Proof.
  induction fuel as [|fuel IH];
    intros bytes region_end offset group accumulator number stop
      position accumulator_bound parsed.
  - discriminate.
  - cbn [Wire.varint_groups] in parsed.
    destruct (offset <? region_end) eqn:inside; [|discriminate].
    destruct (nth_error bytes offset) as [byte|] eqn:at_offset;
      [|discriminate].
    destruct (Wire.byte_ok byte) eqn:physical; [|discriminate].
    destruct ((group =? 9) && (1 <? byte)) eqn:last_guard;
      [discriminate|].
    assert (source : skipn offset bytes =
        byte :: skipn (S offset) bytes)
      by (apply cursor_nth_error_cons; exact at_offset).
    assert (digit_bound : (N.of_nat (byte mod 128) < 128)%N).
    { pose proof (Nat.mod_upper_bound byte 128 ltac:(lia)). lia. }
    set (next := (accumulator + N.of_nat (byte mod 128) *
      128 ^ N.of_nat group)%N).
    assert (next_bound : (next < 128 ^ N.of_nat (S group))%N).
    { unfold next.
      rewrite Nat2N.inj_succ.
      rewrite N.pow_succ_r by lia. nia. }
    destruct (byte <? 128) eqn:small.
    + destruct ((0 <? group) && (byte =? 0)) eqn:minimal;
        [discriminate|].
      assert (number_eq : number = next)
        by (inversion parsed; reflexivity).
      assert (stop_eq : stop = S offset)
        by (inversion parsed; reflexivity).
      subst number stop.
      exists byte. rewrite source, EPM1.decode_varint_cons, small.
      unfold next.
      rewrite Nat.mod_small by (apply Nat.ltb_lt; exact small).
      repeat split; try reflexivity.
      * assert (group <= 9) by lia.
        destruct (Nat.eq_dec group 9) as [last|earlier].
        -- subst group.
           assert (byte <= 1).
           { change (9 =? 9) with true in last_guard.
             cbn in last_guard.
             destruct byte as [|[|higher]]; try lia;
               discriminate last_guard. }
           unfold next in next_bound.
           change (128 ^ N.of_nat 9)%N with 9223372036854775808%N
             in accumulator_bound.
           change (128 ^ N.of_nat 9)%N with 9223372036854775808%N.
           nia.
        -- assert (power : (128 ^ N.of_nat (S group) <= 128 ^ 9)%N).
           { apply N.pow_le_mono_r; lia. }
           assert (cap : (128 ^ 9 < 18446744073709551616)%N)
             by (vm_compute; reflexivity).
           unfold next in next_bound.
           rewrite Nat.mod_small in next_bound
             by (apply Nat.ltb_lt; exact small).
           lia.
      * apply Nat.ltb_lt in inside. lia.
      * intro positive_group.
        destruct (0 <? group) eqn:at_high; [|apply Nat.ltb_ge in at_high; lia].
        cbn in minimal.
        apply Nat.eqb_neq in minimal. lia.
    + change (Wire.varint_groups fuel bytes region_end
        (S offset) (S group) next = Some (number, stop)) in parsed.
      destruct (Wire.varint_groups fuel bytes region_end
          (S offset) (S group) next) as [[child_number child_stop]|]
        eqn:child; [|discriminate].
      inversion parsed; subst number stop.
      destruct (IH bytes region_end (S offset) (S group) next
        child_number child_stop ltac:(lia) next_bound child)
        as [quotient [decoded [numeric [bounded [region positive]]]]].
      specialize (positive ltac:(lia)).
      assert (byte_bound : (byte <? 256) = true)
        by (unfold Wire.byte_ok in physical; exact physical).
      assert (digit : byte mod 128 = byte - 128).
      { symmetry. apply Nat.mod_unique with (q := 1);
          apply Nat.ltb_ge in small; apply Nat.ltb_lt in byte_bound; lia. }
      exists (byte - 128 + 128 * quotient).
      rewrite source, EPM1.decode_varint_cons, small, byte_bound,
        decoded.
      assert (nonzero : (quotient =? 0) = false)
        by (apply Nat.eqb_neq; lia).
      rewrite nonzero.
      repeat split; try reflexivity.
      * unfold next in numeric.
        rewrite digit, Nat2N.inj_succ in numeric.
        rewrite N.pow_succ_r in numeric by lia.
        rewrite Nat2N.inj_add, Nat2N.inj_mul.
        nia.
      * exact bounded.
      * exact region.
      * intro. lia.
Qed.

Lemma wire_full_u64_to_epm :
  forall body number,
    Wire.canonical_varint body (length body) 0 =
      Some (number, length body) ->
    EPM1.decode_varint body = Some (N.to_nat number, []) /\
    (number < 18446744073709551616)%N.
Proof.
  intros body number parsed.
  unfold Wire.canonical_varint in parsed.
  destruct (wire_groups_to_epm 10 body (length body) 0 0 0
    number (length body) eq_refl ltac:(reflexivity) parsed)
    as [decoded [epm [numeric [bounded [region positive]]]]].
  cbn [skipn] in epm.
  rewrite skipn_all in epm.
  assert (same : number = N.of_nat decoded).
  { rewrite numeric. nia. }
  rewrite same in bounded |- *.
  rewrite Nat2N.id.
  split; assumption.
Qed.

Lemma full_u64_decoder_agreement :
  forall body,
    Core.independently_decoded_u64 body =
      Scalar.compact_varint_value body.
Proof.
  intro body.
  destruct (Core.independently_decoded_u64 body) as [source|]
    eqn:independent;
    destruct (Scalar.compact_varint_value body) as [target|]
      eqn:compact; try reflexivity.
  - apply Core.independently_decoded_u64_has_canonical_full_body
      in independent as [decoded [parsed [numeric bounded]]].
    subst source.
    assert (wire := epm_full_u64_to_wire body decoded parsed ltac:(lia)).
    unfold Scalar.compact_varint_value in compact.
    change (Scalar.Wire.canonical_varint body (length body) 0 =
      Some (N.of_nat decoded, length body)) in wire.
    rewrite wire, Nat.eqb_refl in compact.
    inversion compact; subst; reflexivity.
  - apply Core.independently_decoded_u64_has_canonical_full_body
      in independent as [decoded [parsed [numeric bounded]]].
    subst source.
    assert (wire := epm_full_u64_to_wire body decoded parsed ltac:(lia)).
    unfold Scalar.compact_varint_value in compact.
    change (Scalar.Wire.canonical_varint body (length body) 0 =
      Some (N.of_nat decoded, length body)) in wire.
    rewrite wire, Nat.eqb_refl in compact. discriminate.
  - unfold Scalar.compact_varint_value in compact.
    destruct (Wire.canonical_varint body (length body) 0)
      as [[number stop]|] eqn:wire; [|discriminate].
    destruct (stop =? length body) eqn:full; [|discriminate].
    apply Nat.eqb_eq in full. subst stop.
    inversion compact; subst target.
    apply wire_full_u64_to_epm in wire as [parsed bounded].
    unfold Core.independently_decoded_u64 in independent.
    rewrite parsed in independent.
    rewrite N2Nat.id in independent.
    replace (N.leb number 18446744073709551615) with true
      in independent by (symmetry; apply N.leb_le; lia).
    discriminate.
Qed.

Theorem pbint32_scalar_decoder_equivalence :
  forall tag body value,
    (Core.independently_decode_scalar Data.PBInt32 true body =
      Some (Core.SourceI32 value) <->
     Scalar.scalar_semantic_from_source
       (Scalar.ScalarRule tag Data.PBInt32 true) body =
       Some (Scalar.ScalarI32 value)).
Proof.
  intros tag body value.
  unfold Core.independently_decode_scalar,
    Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  unfold Scalar.numeric_scalar_valid.
  rewrite <- !full_u64_decoder_agreement.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|cbn; split; discriminate].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault.
  cbn [negb orb].
  destruct (N.eqb number 0),
    (N.leb number 2147483647 ||
      N.leb 18446744071562067968 number);
    cbn; split; intro accepted; try discriminate;
    inversion accepted; subst; reflexivity.
Qed.

Theorem pbint32_scalar_decoder_refusal_equivalence :
  forall tag body,
    Core.independently_decode_scalar Data.PBInt32 true body = None <->
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule tag Data.PBInt32 true) body = None.
Proof.
  intros tag body.
  unfold Core.independently_decode_scalar,
    Scalar.scalar_semantic_from_source, Scalar.scalar_body_valid.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden].
  unfold Scalar.numeric_scalar_valid.
  rewrite <- !full_u64_decoder_agreement.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|cbn; tauto].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault.
  cbn [negb orb].
  destruct (N.eqb number 0),
    (N.leb number 2147483647 ||
      N.leb 18446744071562067968 number);
    cbn; split; intro refused; try reflexivity; discriminate.
Qed.

Definition process_pbint32_slots : list (nat * nat) :=
  [(16, 4); (17, 3); (19, 5); (21, 3)].

Definition negative_one_body : list nat :=
  [255; 255; 255; 255; 255; 255; 255; 255; 255; 1].

Lemma generated_process_pbint32_contract :
  forall family tag,
    In (family, tag) process_pbint32_slots ->
    Admission.source_scalar_contract_for family tag =
      Some (Admission.SourceScalarContract Data.PBInt32 true).
Proof.
  intros family tag member.
  unfold process_pbint32_slots in member.
  destruct member as [member|[member|[member|[member|[]]]]];
    inversion member; subst; vm_compute; reflexivity.
Qed.

Lemma generated_program_has_requested_family :
  forall family program,
    Schema.lookup_projected_program family [] [] = Some program ->
    Schema.program_family program = family.
Proof.
  intros family program found.
  unfold Schema.lookup_projected_program in found.
  destruct (find (fun row => Data.program_type_id row =? family)
      Data.generated_programs) as [row|] eqn:selected;
    [|discriminate].
  inversion found; subst program.
  apply find_some in selected as [_ same].
  cbn [Schema.program_family Schema.project_generated_program].
  now apply Nat.eqb_eq in same.
Qed.

Theorem generated_process_pbint32_admission :
  forall family program raw value,
    In (family, Schema.raw_tag raw) process_pbint32_slots ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Admission.source_typed_scalar program raw =
      Some (Core.TypedSourceScalar raw (Core.SourceI32 value)) ->
    Schema.raw_wire raw = Schema.Varint /\
    Schema.raw_payload raw = Schema.Scalar /\
    Core.independently_decode_scalar Data.PBInt32 true
      (Schema.raw_body raw) = Some (Core.SourceI32 value).
Proof.
  intros family program raw value slot found accepted.
  pose proof (generated_program_has_requested_family
    family program found) as family_exact.
  pose proof (generated_process_pbint32_contract
    family (Schema.raw_tag raw) slot) as contract.
  unfold Admission.source_typed_scalar,
    Admission.source_scalar_value_of_raw in accepted.
  rewrite family_exact, contract in accepted.
  destruct (Schema.raw_payload raw) eqn:payload; try discriminate.
  destruct (Schema.raw_program_type_id raw) as [type_id|]
    eqn:no_type; try discriminate.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Schema.raw_tag raw)) as [descriptor|] eqn:resolved;
    [|discriminate].
  cbn [Admission.source_contract_kind Admission.source_scalar_wire
    Admission.source_contract_default_forbidden] in accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Schema.wire_eqb (Schema.raw_wire raw) Schema.Varint &&
      Schema.wire_eqb Schema.Varint (Schema.descriptor_wire descriptor) &&
      Schema.payload_eqb (Schema.descriptor_payload descriptor)
        Schema.Scalar) eqn:guard; [|discriminate].
  destruct (Core.independently_decode_scalar Data.PBInt32 true
      (Schema.raw_body raw)) as [decoded_value|] eqn:decoder;
    [|discriminate].
  cbn [option_map] in accepted.
  inversion accepted; subst decoded_value.
  assert (wire : Schema.raw_wire raw = Schema.Varint).
  { repeat rewrite andb_true_iff in guard.
    destruct guard as [[[bytes wire_match] descriptor_wire] descriptor_payload].
    destruct (Schema.raw_wire raw); cbn in wire_match;
      try discriminate; reflexivity. }
  repeat split; try reflexivity; assumption.
Qed.

Lemma accepted_pbint32_has_signed_range :
  forall body value,
    Core.independently_decode_scalar Data.PBInt32 true body =
      Some (Core.SourceI32 value) ->
    exists decoded,
      EPM1.decode_varint body = Some (decoded, []) /\
      (N.of_nat decoded <> 0)%N /\
      ((N.of_nat decoded <= 2147483647)%N \/
       (18446744071562067968 <= N.of_nat decoded)%N) /\
      (N.of_nat decoded <= 18446744073709551615)%N /\
      value = Core.source_signed_64 (N.of_nat decoded).
Proof.
  intros body value accepted.
  cbn [Core.independently_decode_scalar] in accepted.
  destruct (Core.independently_decoded_u64 body) as [number|]
    eqn:read; [|discriminate].
  unfold Core.source_numeric_scalar, Core.source_numeric_nondefault
    in accepted.
  simpl in accepted.
  destruct (N.eqb number 0) eqn:nonzero;
    simpl in accepted; [discriminate|].
  destruct (N.leb number 2147483647 ||
      N.leb 18446744071562067968 number) eqn:range;
    simpl in accepted; [|discriminate].
  inversion accepted; subst value.
  apply Core.independently_decoded_u64_has_canonical_full_body
    in read as [decoded [parsed [numeric bounded]]].
  subst number.
  exists decoded.
  repeat split; try assumption; try reflexivity.
  - apply N.eqb_neq in nonzero. exact nonzero.
  - apply orb_true_iff in range.
    destruct range as [positive|negative].
    + left. now apply N.leb_le.
    + right. now apply N.leb_le.
Qed.

Lemma independent_pbint32_signed_value_cases :
  forall number,
    ((number <= 2147483647)%N \/
     ((18446744071562067968 <= number)%N /\
      (number <= 18446744073709551615)%N)) ->
    Core.source_signed_64 number = Scalar.signed_64 number /\
    ((number <= 2147483647)%N ->
      Core.source_signed_64 number = Z.of_N number) /\
    ((18446744071562067968 <= number)%N ->
      Core.source_signed_64 number =
        (Z.of_N number - 18446744073709551616)%Z).
Proof.
  intros number domain.
  split; [reflexivity|].
  split; intro bound.
  - unfold Core.source_signed_64.
    replace (N.ltb number 9223372036854775808) with true
      by (symmetry; apply N.ltb_lt; lia).
    reflexivity.
  - unfold Core.source_signed_64.
    replace (N.ltb number 9223372036854775808) with false
      by (symmetry; apply N.ltb_ge; lia).
    reflexivity.
Qed.

Lemma canonical_positive_signed_representatives :
  Core.independently_decode_scalar Data.PBInt32 true [1] =
    Some (Core.SourceI32 1%Z) /\
  Scalar.scalar_semantic_from_source
    (Scalar.ScalarRule 1 Data.PBInt32 true) [1] =
    Some (Scalar.ScalarI32 1%Z) /\
  Core.independently_decode_scalar Data.PBInt32 true
    [128; 1] = Some (Core.SourceI32 128%Z) /\
  Scalar.scalar_semantic_from_source
    (Scalar.ScalarRule 1 Data.PBInt32 true) [128; 1] =
    Some (Scalar.ScalarI32 128%Z).
Proof. vm_compute. repeat split; reflexivity. Qed.

Lemma target_negative_one_is_canonical :
  Scalar.scalar_semantic_from_source
    (Scalar.ScalarRule 1 Data.PBInt32 true) negative_one_body =
    Some (Scalar.ScalarI32 (-1)%Z).
Proof.
  unfold Scalar.scalar_semantic_from_source,
    Scalar.scalar_body_valid, Scalar.numeric_scalar_valid,
    Scalar.compact_varint_value, negative_one_body.
  cbn [Scalar.scalar_kind Scalar.scalar_default_forbidden length].
  rewrite !Wire.terminal_group_nine_preserves_max_u64.
  reflexivity.
Qed.

Lemma independent_negative_one_is_canonical :
  Core.independently_decode_scalar Data.PBInt32 true negative_one_body =
    Some (Core.SourceI32 (-1)%Z).
Proof.
  apply (proj2 (pbint32_scalar_decoder_equivalence
    1 negative_one_body (-1)%Z)).
  exact target_negative_one_is_canonical.
Qed.

Lemma rejected_signed_representatives :
  Core.independently_decode_scalar Data.PBInt32 true [0] = None /\
  Scalar.scalar_semantic_from_source
    (Scalar.ScalarRule 1 Data.PBInt32 true) [0] = None /\
  Core.independently_decode_scalar Data.PBInt32 true [128; 0] = None /\
  Scalar.scalar_semantic_from_source
    (Scalar.ScalarRule 1 Data.PBInt32 true) [128; 0] = None /\
  Core.source_numeric_scalar Data.PBInt32 true 2147483648%N = None /\
  Scalar.scalar_semantic_from_source
    (Scalar.ScalarRule 1 Data.PBInt32 true)
    [128; 128; 128; 128; 8] = None.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem generated_process_pbint32_decoder_bridge :
  forall family program raw value,
    In (family, Schema.raw_tag raw) process_pbint32_slots ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Admission.source_typed_scalar program raw =
      Some (Core.TypedSourceScalar raw (Core.SourceI32 value)) ->
    Schema.raw_wire raw = Schema.Varint /\
    Schema.raw_payload raw = Schema.Scalar /\
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule (Schema.raw_tag raw) Data.PBInt32 true)
      (Schema.raw_body raw) = Some (Scalar.ScalarI32 value).
Proof.
  intros family program raw value slot found accepted.
  pose proof (generated_process_pbint32_admission
    family program raw value slot found accepted)
    as [wire [payload decoded]].
  repeat split; try assumption.
  apply (proj1 (pbint32_scalar_decoder_equivalence
    (Schema.raw_tag raw) (Schema.raw_body raw) value)).
  exact decoded.
Qed.

Theorem generated_process_signed_representatives_agree :
  forall family program raw value,
    In (family, Schema.raw_tag raw) process_pbint32_slots ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Admission.source_typed_scalar program raw =
      Some (Core.TypedSourceScalar raw (Core.SourceI32 value)) ->
    Schema.raw_body raw = [1] \/
      Schema.raw_body raw = [128; 1] ->
    Schema.raw_wire raw = Schema.Varint /\
    Schema.raw_payload raw = Schema.Scalar /\
    Scalar.scalar_semantic_from_source
      (Scalar.ScalarRule (Schema.raw_tag raw) Data.PBInt32 true)
      (Schema.raw_body raw) = Some (Scalar.ScalarI32 value).
Proof.
  intros family program raw value slot found accepted representative.
  exact (generated_process_pbint32_decoder_bridge
    family program raw value slot found accepted).
Qed.

Print Assumptions epm_full_u64_to_wire.
Print Assumptions wire_full_u64_to_epm.
Print Assumptions full_u64_decoder_agreement.
Print Assumptions pbint32_scalar_decoder_equivalence.
Print Assumptions pbint32_scalar_decoder_refusal_equivalence.
Print Assumptions generated_process_pbint32_contract.
Print Assumptions generated_program_has_requested_family.
Print Assumptions generated_process_pbint32_admission.
Print Assumptions accepted_pbint32_has_signed_range.
Print Assumptions independent_pbint32_signed_value_cases.
Print Assumptions canonical_positive_signed_representatives.
Print Assumptions target_negative_one_is_canonical.
Print Assumptions independent_negative_one_is_canonical.
Print Assumptions rejected_signed_representatives.
Print Assumptions generated_process_pbint32_decoder_bridge.
Print Assumptions generated_process_signed_representatives_agree.
