From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import StateImportCodec.
Import ListNotations.

Definition phlo_word width value := rev (import_nat_to_key width value).
Definition phlo_word_value bytes := import_key_to_nat (rev bytes).

Theorem phlo_word_exact_width : forall width value,
  length (phlo_word width value) = width.
Proof.
  intros. unfold phlo_word. rewrite length_rev. apply import_key_conversion_preserves_exact_width.
Qed.

Lemma phlo_little_word_value_bound : forall bytes,
  Forall import_wire_byte bytes -> import_key_to_nat bytes < 256 ^ length bytes.
Proof.
  intros bytes valid. induction valid as [|byte rest valid_byte valid_rest IH].
  - cbn. lia.
  - change (byte + 256 * import_key_to_nat rest < 256 * 256 ^ length rest).
    unfold import_wire_byte in valid_byte. nia.
Qed.

Lemma phlo_little_word_roundtrip : forall width value,
  value < 256 ^ width -> import_key_to_nat (import_nat_to_key width value) = value.
Proof.
  induction width as [|width IH]; intros value bounded.
  - cbn in bounded. assert (value = 0) by lia. subst. reflexivity.
  - change (value < 256 * 256 ^ width) in bounded.
    change (value mod 256 + 256 * import_key_to_nat (import_nat_to_key width (value / 256)) = value).
    rewrite IH.
    + pose proof (Nat.div_mod value 256 ltac:(lia)). nia.
    + apply Nat.Div0.div_lt_upper_bound; lia.
Qed.

Theorem phlo_word_roundtrip : forall width value,
  value < 256 ^ width -> phlo_word_value (phlo_word width value) = value.
Proof.
  intros. unfold phlo_word_value, phlo_word. rewrite rev_involutive.
  now apply phlo_little_word_roundtrip.
Qed.

Theorem phlo_word_reencode_exact : forall bytes,
  Forall import_wire_byte bytes -> phlo_word (length bytes) (phlo_word_value bytes) = bytes.
Proof.
  intros bytes valid. unfold phlo_word, phlo_word_value.
  rewrite <- (length_rev bytes).
  rewrite import_key_conversion_is_reversible.
  - apply rev_involutive.
  - now apply Forall_rev.
Qed.

Definition phlo_parse_word width input :=
  match import_take_exact_bytes width input with
  | Some (bytes, suffix) =>
      if forallb (fun byte => byte <? 256) bytes then Some (phlo_word_value bytes, suffix) else None
  | None => None
  end.

Lemma phlo_take_prefix : forall prefix suffix,
  import_take_exact_bytes (length prefix) (prefix ++ suffix) = Some (prefix, suffix).
Proof.
  intros. unfold import_take_exact_bytes. rewrite length_app.
  assert ((length prefix <=? length prefix + length suffix) = true) by (apply Nat.leb_le; lia).
  rewrite H, firstn_app, firstn_all, Nat.sub_diag, skipn_app, skipn_all.
  rewrite Nat.sub_diag. cbn. now rewrite app_nil_r.
Qed.

Theorem phlo_parse_word_roundtrip : forall width value suffix,
  value < 256 ^ width ->
  phlo_parse_word width (phlo_word width value ++ suffix) = Some (value, suffix).
Proof.
  intros width value suffix bounded. unfold phlo_parse_word.
  rewrite <- (phlo_word_exact_width width value) at 1. rewrite phlo_take_prefix.
  assert (valid : forallb (fun byte => byte <? 256) (phlo_word width value) = true).
  { apply forallb_forall. intros byte included. apply Nat.ltb_lt.
    pose proof (import_decoded_numeric_key_has_valid_bytes width value) as bytes.
    apply Forall_rev in bytes. unfold phlo_word in included.
    rewrite Forall_forall in bytes. exact (bytes byte included). }
  rewrite valid, phlo_word_roundtrip; auto.
Qed.

Theorem phlo_parse_word_preserves_exact_prefix : forall width input value suffix,
  phlo_parse_word width input = Some (value, suffix) ->
  value < 256 ^ width /\ input = phlo_word width value ++ suffix.
Proof.
  intros width input value suffix parsed. unfold phlo_parse_word in parsed.
  destruct (import_take_exact_bytes width input) as [[bytes tail]|] eqn:taken; [|discriminate].
  destruct (forallb (fun byte => byte <? 256) bytes) eqn:checked; [|discriminate].
  inversion parsed; subst value suffix. apply import_exact_bytes_partition in taken.
  destruct taken as [size partition].
  assert (valid : Forall import_wire_byte bytes).
  { rewrite Forall_forall, forallb_forall in *. intros byte included.
    apply Nat.ltb_lt. apply checked. exact included. }
  split.
  - unfold phlo_word_value. rewrite <- size, <- (length_rev bytes).
    apply phlo_little_word_value_bound. now apply Forall_rev.
  - rewrite <- size, phlo_word_reencode_exact; assumption.
Qed.

Definition phlo_field width payload := phlo_word width (length payload) ++ payload.

Definition phlo_parse_field width maximum input :=
  match phlo_parse_word width input with
  | Some (count, rest) =>
      if count <=? maximum then import_take_exact_bytes count rest else None
  | None => None
  end.

Theorem phlo_field_roundtrip : forall width maximum payload suffix,
  length payload < 256 ^ width -> length payload <= maximum ->
  phlo_parse_field width maximum (phlo_field width payload ++ suffix) = Some (payload, suffix).
Proof.
  intros width maximum payload suffix representable bounded.
  unfold phlo_parse_field, phlo_field. rewrite <- app_assoc, phlo_parse_word_roundtrip; auto.
  assert ((length payload <=? maximum) = true) by now apply Nat.leb_le.
  rewrite H. apply phlo_take_prefix.
Qed.

Theorem phlo_parsed_field_reencodes_exact_prefix : forall width maximum input payload suffix,
  phlo_parse_field width maximum input = Some (payload, suffix) ->
  length payload < 256 ^ width /\ length payload <= maximum /\
  input = phlo_field width payload ++ suffix.
Proof.
  intros width maximum input payload suffix parsed. unfold phlo_parse_field in parsed.
  destruct (phlo_parse_word width input) as [[count rest]|] eqn:word; [|discriminate].
  destruct (count <=? maximum) eqn:bounded; [|discriminate].
  apply Nat.leb_le in bounded. apply import_exact_bytes_partition in parsed.
  destruct parsed as [size partition]. apply phlo_parse_word_preserves_exact_prefix in word.
  destruct word as [fits prefix]. repeat split; try lia.
  unfold phlo_field. rewrite size, <- app_assoc, <- partition. exact prefix.
Qed.

Theorem phlo_field_consumes_exact_size : forall width maximum input payload suffix,
  phlo_parse_field width maximum input = Some (payload, suffix) ->
  length input = width + length payload + length suffix.
Proof.
  intros. apply phlo_parsed_field_reencodes_exact_prefix in H. destruct H as [_ [_ same]].
  rewrite same, length_app. unfold phlo_field. rewrite length_app, phlo_word_exact_width. lia.
Qed.

Theorem phlo_field_encoding_is_injective : forall width maximum first second,
  length first < 256 ^ width -> length first <= maximum ->
  length second < 256 ^ width -> length second <= maximum ->
  phlo_field width first = phlo_field width second -> first = second.
Proof.
  intros width maximum first second first_fits first_bound second_fits second_bound same.
  pose proof (phlo_field_roundtrip width maximum first [] first_fits first_bound) as parsed_first.
  pose proof (phlo_field_roundtrip width maximum second [] second_fits second_bound) as parsed_second.
  rewrite same in parsed_first. congruence.
Qed.

Definition phlo_append_field width maximum existing payload :=
  if (length payload <? 256 ^ width) &&
    (length existing + width + length payload <=? maximum)
  then Some (existing ++ phlo_field width payload)
  else None.

Theorem phlo_append_field_acceptance_exact : forall width maximum existing payload,
  (exists output, phlo_append_field width maximum existing payload = Some output) <->
  length payload < 256 ^ width /\ length existing + width + length payload <= maximum.
Proof.
  intros. unfold phlo_append_field.
  destruct ((length payload <? 256 ^ width) &&
    (length existing + width + length payload <=? maximum)) eqn:checked.
  - rewrite andb_true_iff, Nat.ltb_lt, Nat.leb_le in checked.
    split; [auto|intros; eexists; reflexivity].
  - split; [intros [? impossible]; discriminate|].
    intros [fits bounded]. assert ((length payload <? 256 ^ width) &&
      (length existing + width + length payload <=? maximum) = true)
      by (rewrite andb_true_iff, Nat.ltb_lt, Nat.leb_le; auto).
    congruence.
Qed.

Theorem phlo_append_field_preserves_prefix_and_limit : forall width maximum existing payload output,
  phlo_append_field width maximum existing payload = Some output ->
  output = existing ++ phlo_field width payload /\ length output <= maximum.
Proof.
  intros width maximum existing payload output appended.
  pose proof (proj1 (phlo_append_field_acceptance_exact width maximum existing payload)
    (ex_intro _ output appended)) as [_ bounded].
  unfold phlo_append_field in appended.
  destruct ((length payload <? 256 ^ width) &&
    (length existing + width + length payload <=? maximum)); [|discriminate].
  inversion appended; subst. split; [reflexivity|].
  rewrite length_app. unfold phlo_field. rewrite length_app, phlo_word_exact_width. lia.
Qed.

Definition phlo_fields width payloads := concat (map (phlo_field width) payloads).

Fixpoint phlo_parse_fields count width maximum input :=
  match count with
  | 0 => Some ([], input)
  | S remaining =>
      match phlo_parse_field width maximum input with
      | Some (payload, rest) =>
          match phlo_parse_fields remaining width maximum rest with
          | Some (payloads, suffix) => Some (payload :: payloads, suffix)
          | None => None
          end
      | None => None
      end
  end.

Theorem phlo_field_sequence_roundtrip : forall width maximum payloads suffix,
  Forall (fun payload => length payload < 256 ^ width /\ length payload <= maximum) payloads ->
  phlo_parse_fields (length payloads) width maximum (phlo_fields width payloads ++ suffix) =
    Some (payloads, suffix).
Proof.
  intros width maximum payloads suffix valid.
  induction valid as [|payload payloads [fits bounded] valid IH].
  - reflexivity.
  - change (match phlo_parse_field width maximum
      ((phlo_field width payload ++ phlo_fields width payloads) ++ suffix) with
      | Some (head, rest) => match phlo_parse_fields (length payloads) width maximum rest with
        | Some (tail, remaining) => Some (head :: tail, remaining) | None => None end
      | None => None end = Some (payload :: payloads, suffix)).
    rewrite <- app_assoc, phlo_field_roundtrip; auto. now rewrite IH.
Qed.

Theorem phlo_field_sequence_preserves_exact_prefix : forall count width maximum input payloads suffix,
  phlo_parse_fields count width maximum input = Some (payloads, suffix) ->
  length payloads = count /\
  Forall (fun payload => length payload < 256 ^ width /\ length payload <= maximum) payloads /\
  input = phlo_fields width payloads ++ suffix.
Proof.
  induction count as [|count IH]; intros width maximum input payloads suffix parsed.
  - cbn in parsed. inversion parsed; subst. repeat split; constructor.
  - cbn [phlo_parse_fields] in parsed.
    destruct (phlo_parse_field width maximum input) as [[payload rest]|] eqn:head; [|discriminate].
    destruct (phlo_parse_fields count width maximum rest) as [[tail remaining]|] eqn:next; [|discriminate].
    inversion parsed; subst payloads suffix.
    apply phlo_parsed_field_reencodes_exact_prefix in head.
    destruct head as [fits [bounded prefix]].
    destruct (IH width maximum rest tail remaining next) as [size [valid suffix]].
    split; [cbn; lia|]. split; [constructor; auto|].
    unfold phlo_fields. cbn [map concat]. rewrite prefix, suffix. now rewrite app_assoc.
Qed.

Definition phlo_decode_fields count width maximum input :=
  match phlo_parse_fields count width maximum input with
  | Some (payloads, []) => Some payloads
  | _ => None
  end.

Theorem phlo_complete_fields_reencode_exactly : forall count width maximum input payloads,
  phlo_decode_fields count width maximum input = Some payloads ->
  length payloads = count /\
  Forall (fun payload => length payload < 256 ^ width /\ length payload <= maximum) payloads /\
  input = phlo_fields width payloads.
Proof.
  intros count width maximum input payloads decoded. unfold phlo_decode_fields in decoded.
  destruct (phlo_parse_fields count width maximum input) as [[fields [|byte rest]]|] eqn:parsed;
    try discriminate.
  inversion decoded; subst. apply phlo_field_sequence_preserves_exact_prefix in parsed.
  now rewrite app_nil_r in parsed.
Qed.

Theorem phlo_field_sequences_cannot_alias : forall width maximum first second,
  0 < width ->
  Forall (fun payload => length payload < 256 ^ width /\ length payload <= maximum) first ->
  Forall (fun payload => length payload < 256 ^ width /\ length payload <= maximum) second ->
  phlo_fields width first = phlo_fields width second -> first = second.
Proof.
  intros width maximum first. induction first as [|head tail IH];
    intros [|other rest] positive valid_first valid_second same.
  - reflexivity.
  - exfalso. apply (f_equal (@length nat)) in same.
    change (0 = length (phlo_field width other ++ phlo_fields width rest)) in same.
    rewrite length_app in same. unfold phlo_field in same.
    rewrite length_app, phlo_word_exact_width in same. lia.
  - exfalso. apply (f_equal (@length nat)) in same.
    change (length (phlo_field width head ++ phlo_fields width tail) = 0) in same.
    rewrite length_app in same. unfold phlo_field in same.
    rewrite length_app, phlo_word_exact_width in same. lia.
  - apply Forall_cons_iff in valid_first. destruct valid_first as [[head_fits head_bound] tail_valid].
    apply Forall_cons_iff in valid_second. destruct valid_second as [[other_fits other_bound] rest_valid].
    change (phlo_field width head ++ phlo_fields width tail =
      phlo_field width other ++ phlo_fields width rest) in same.
    pose proof (phlo_field_roundtrip width maximum head (phlo_fields width tail) head_fits head_bound) as first_parsed.
    pose proof (phlo_field_roundtrip width maximum other (phlo_fields width rest) other_fits other_bound) as second_parsed.
    rewrite same in first_parsed. rewrite second_parsed in first_parsed.
    injection first_parsed as heads suffixes. subst other. f_equal.
    apply IH; try assumption. symmetry. exact suffixes.
Qed.

Theorem phlo_word_encoding_is_injective : forall width first second,
  first < 256 ^ width -> second < 256 ^ width ->
  phlo_word width first = phlo_word width second -> first = second.
Proof.
  intros width first second first_fits second_fits same.
  apply (f_equal phlo_word_value) in same.
  rewrite !phlo_word_roundtrip in same; assumption.
Qed.

Print Assumptions phlo_field_sequences_cannot_alias.
Print Assumptions phlo_word_encoding_is_injective.
Print Assumptions phlo_field_sequence_roundtrip.
Print Assumptions phlo_field_sequence_preserves_exact_prefix.
Print Assumptions phlo_complete_fields_reencode_exactly.
Print Assumptions phlo_word_exact_width.
Print Assumptions phlo_word_roundtrip.
Print Assumptions phlo_word_reencode_exact.
Print Assumptions phlo_parse_word_roundtrip.
Print Assumptions phlo_parse_word_preserves_exact_prefix.
Print Assumptions phlo_field_roundtrip.
Print Assumptions phlo_parsed_field_reencodes_exact_prefix.
Print Assumptions phlo_field_consumes_exact_size.
Print Assumptions phlo_field_encoding_is_injective.
Print Assumptions phlo_append_field_acceptance_exact.
Print Assumptions phlo_append_field_preserves_prefix_and_limit.
