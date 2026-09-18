From Stdlib Require Import List Arith Bool Lia.
From StackSafePDA Require Import EPM1 EPathMap SubstitutionRegionFraming
  SubstitutionRegionOrdinals SubstitutionValueRegions.

Import ListNotations.
Set Implicit Arguments.
Module SubstitutionSnapshotPrefix.
Module F := SubstitutionRegionFraming.SubstitutionRegionFraming.
Module O := SubstitutionRegionOrdinals.SubstitutionRegionOrdinals.
Module V := SubstitutionValueRegions.SubstitutionValueRegions.

Definition fixed_header storage_mode : bytes :=
  epm_magic ++ [epm_version; mode_code storage_mode].

Definition read_mode (input : bytes) : option mode :=
  if list_eq_dec Nat.eq_dec (firstn 6 input) (fixed_header Neutral) then Some Neutral
  else if list_eq_dec Nat.eq_dec (firstn 6 input) (fixed_header SetMode) then Some SetMode
  else if list_eq_dec Nat.eq_dec (firstn 6 input) (fixed_header MapMode) then Some MapMode
  else None.

Lemma fixed_header_has_six_bytes : forall storage_mode,
  length (fixed_header storage_mode) = 6.
Proof. destruct storage_mode; reflexivity. Qed.

Theorem successful_mode_selects_exact_fixed_header : forall input storage_mode,
  read_mode input = Some storage_mode ->
  firstn 6 input = fixed_header storage_mode /\ 6 <= length input.
Proof.
  intros input storage_mode Selected.
  assert (Header : firstn 6 input = fixed_header storage_mode).
  { unfold read_mode in Selected.
    destruct (list_eq_dec _ _ _) as [Same|Different].
    - inversion Selected; subst; exact Same.
    - destruct (list_eq_dec _ _ _) as [Same|DifferentSet].
      + inversion Selected; subst; exact Same.
      + destruct (list_eq_dec _ _ _) as [Same|DifferentMap]; try discriminate.
        inversion Selected; subst; exact Same. }
  split; [exact Header|].
  pose proof (f_equal (@length nat) Header) as Length.
  rewrite length_firstn, fixed_header_has_six_bytes in Length; lia.
Qed.

Definition read_count (input : bytes) (position : nat) : option (nat * nat) :=
  match F.scan_groups 10 true (skipn position input) with
  | Some (count, suffix) =>
      Some (count, position + (length (skipn position input) - length suffix))
  | None => None
  end.

Theorem successful_count_selects_exact_boundary : forall input position count ending,
  position <= length input -> read_count input position = Some (count, ending) ->
  position < ending /\ ending <= length input /\
  ending = position + length (encode_varint count) /\
  decode_varint (skipn position input) = Some (count, skipn ending input).
Proof.
  intros input position count ending Bound Selected. unfold read_count in Selected.
  destruct (F.scan_groups 10 true (skipn position input)) as [[value suffix]|]
    eqn:Scan; try discriminate.
  inversion Selected; subst count ending; clear Selected.
  pose proof (@F.successful_bounded_varint_reencodes_consumed_prefix
    (skipn position input) value suffix Scan) as Split.
  destruct (@V.suffix_split_gives_exact_offset input position
    (encode_varint value) suffix Bound Split) as [EndBound [AtEnd Exact]].
  rewrite Exact.
  pose proof (O.encoded_varint_is_nonempty value) as Progress.
  repeat split; try lia.
  rewrite AtEnd. exact (proj1 (@F.successful_groups_refine_decoder
    10 true (skipn position input) value suffix Scan)).
Qed.

Record prefix := Prefix {
  storage_mode : mode;
  arena : V.span;
  value_count : nat;
  values_start : nat
}.

Definition parse_prefix (input : bytes) : option prefix :=
  match read_mode input with
  | Some storage_mode =>
      match V.scan_frame input 6 with
      | Some arena =>
          match read_count input (V.finish arena) with
          | Some (count, ending) =>
              if count <=? length input - ending then
                Some (Prefix storage_mode arena count ending)
              else None
          | None => None
          end
      | None => None
      end
  | None => None
  end.

Record prefix_certificate (input : bytes) (p : prefix) : Prop := PrefixCertificate {
  mode_bytes : firstn 6 input = fixed_header (storage_mode p);
  arena_bounds : 6 < V.start (arena p) /\
    V.start (arena p) <= V.finish (arena p) /\
    V.finish (arena p) < values_start p /\ values_start p <= length input;
  arena_header_width : V.start (arena p) =
    6 + length (encode_varint (length (V.body input (arena p))));
  arena_decode : decode_frame (skipn 6 input) =
    Some (V.body input (arena p), skipn (V.finish (arena p)) input);
  count_header_width : values_start p =
    V.finish (arena p) + length (encode_varint (value_count p));
  count_decode : decode_varint (skipn (V.finish (arena p)) input) =
    Some (value_count p, skipn (values_start p) input);
  count_lower_bound : value_count p <= length input - values_start p
}.

Theorem successful_prefix_has_exact_source_certificate : forall input p,
  parse_prefix input = Some p -> prefix_certificate input p.
Proof.
  intros input p Parsed. unfold parse_prefix in Parsed.
  destruct (read_mode input) as [m|] eqn:Mode; try discriminate.
  destruct (V.scan_frame input 6) as [r|] eqn:Arena; try discriminate.
  destruct (read_count input (V.finish r)) as [[count ending]|] eqn:Count;
    try discriminate.
  destruct (count <=? length input - ending) eqn:Enough; try discriminate.
  inversion Parsed; subst p; clear Parsed.
  apply Nat.leb_le in Enough.
  destruct (@successful_mode_selects_exact_fixed_header input m Mode) as [Header Bound].
  destruct (@V.single_frame_selects_exact_physical_range input 6 r Bound Arena)
    as [Start [Order [End [Width Decode]]]].
  destruct (@successful_count_selects_exact_boundary input (V.finish r) count ending End Count)
    as [Progress [Limit [CountWidth CountDecode]]].
  constructor; cbn; auto; intuition lia.
Qed.

Theorem prefix_reconstructs_exact_envelope_without_value_decode : forall input p,
  parse_prefix input = Some p ->
  input = fixed_header (storage_mode p) ++
    encode_frame (V.body input (arena p)) ++
    encode_varint (value_count p) ++ skipn (values_start p) input.
Proof.
  intros input p Parsed.
  destruct (@successful_prefix_has_exact_source_certificate input p Parsed)
    as [Header Bounds Width Arena CountWidth Count Enough].
  pose proof (@F.successful_frame_reencodes_consumed_prefix
    (skipn 6 input) (V.body input (arena p)) (skipn (V.finish (arena p)) input) Arena) as A.
  pose proof (@F.successful_varint_reencodes_consumed_prefix
    (skipn (V.finish (arena p)) input) (value_count p)
    (skipn (values_start p) input) Count) as C.
  rewrite <- (firstn_skipn 6 input) at 1.
  rewrite Header, A, C. reflexivity.
Qed.

Theorem prefix_offsets_and_count_fit_resident_input : forall input p limit,
  length input <= limit -> parse_prefix input = Some p ->
  V.start (arena p) <= limit /\ V.finish (arena p) <= limit /\
  values_start p <= limit /\ value_count p <= limit /\
  length (V.body input (arena p)) <= limit.
Proof.
  intros input p limit Limit Parsed.
  destruct (@successful_prefix_has_exact_source_certificate input p Parsed)
    as [Header [Start [Order [End Finish]]] Width Arena CountWidth Count Enough].
  rewrite V.span_body_length by lia. intuition lia.
Qed.

Definition inspect_framing input : option (prefix * list V.span) :=
  match parse_prefix input with
  | Some p =>
      match V.inspect_values (value_count p) input (values_start p) with
      | Some ranges => Some (p, ranges)
      | None => None
      end
  | None => None
  end.

Theorem successful_framing_retains_prefix_and_value_selections : forall input p ranges,
  inspect_framing input = Some (p, ranges) ->
  parse_prefix input = Some p /\
  V.inspect_values (value_count p) input (values_start p) = Some ranges.
Proof.
  intros input p ranges Inspected. unfold inspect_framing in Inspected.
  destruct (parse_prefix input) as [selected|] eqn:Prefix; try discriminate.
  destruct (V.inspect_values (value_count selected) input (values_start selected))
    as [selected_ranges|] eqn:Values; try discriminate.
  inversion Inspected; subst; auto.
Qed.

Theorem complete_framing_reconstructs_exact_epm1_bytes : forall input p ranges,
  inspect_framing input = Some (p, ranges) ->
  input = encode_epm1
    (Epm1Snapshot (storage_mode p) (V.body input (arena p)) (map (V.body input) ranges)).
Proof.
  intros input p ranges Inspected.
  destruct (@successful_framing_retains_prefix_and_value_selections input p ranges Inspected)
    as [Prefix Values].
  destruct (@V.successful_inspection_decodes_complete_suffix
    (value_count p) input (values_start p) ranges Values) as [Part [Count Decode]].
  pose proof (@F.successful_frames_reencode_consumed_prefix
    (value_count p) (skipn (values_start p) input) (map (V.body input) ranges) [] Decode)
    as Suffix.
  rewrite app_nil_r in Suffix.
  rewrite (@prefix_reconstructs_exact_envelope_without_value_decode input p Prefix) at 1.
  rewrite Suffix.
  unfold encode_epm1, fixed_header; cbn [snapshot_mode topology_arena value_bodies].
  rewrite length_map, Count. now repeat rewrite app_assoc.
Qed.

Theorem malformed_prefix_never_publishes_framing : forall input,
  parse_prefix input = None -> inspect_framing input = None.
Proof. intros; unfold inspect_framing; now rewrite H. Qed.

Theorem malformed_value_suffix_never_publishes_framing : forall input p,
  parse_prefix input = Some p ->
  V.inspect_values (value_count p) input (values_start p) = None ->
  inspect_framing input = None.
Proof. intros; unfold inspect_framing; now rewrite H, H0. Qed.

Example route_prefix_does_not_validate_topology_or_value_suffix :
  parse_prefix [69;80;77;49;1;2;0;1;128] =
    Some (Prefix MapMode (V.Span 7 7) 1 8) /\
  inspect_framing [69;80;77;49;1;2;0;1;128] = None.
Proof. split; reflexivity. Qed.

Example impossible_count_is_rejected_before_framing :
  parse_prefix [69;80;77;49;1;2;0;2;0] = None.
Proof. reflexivity. Qed.

Print Assumptions fixed_header_has_six_bytes.
Print Assumptions successful_mode_selects_exact_fixed_header.
Print Assumptions successful_count_selects_exact_boundary.
Print Assumptions successful_prefix_has_exact_source_certificate.
Print Assumptions prefix_reconstructs_exact_envelope_without_value_decode.
Print Assumptions prefix_offsets_and_count_fit_resident_input.
Print Assumptions successful_framing_retains_prefix_and_value_selections.
Print Assumptions complete_framing_reconstructs_exact_epm1_bytes.
Print Assumptions malformed_prefix_never_publishes_framing.
Print Assumptions malformed_value_suffix_never_publishes_framing.
Print Assumptions route_prefix_does_not_validate_topology_or_value_suffix.
Print Assumptions impossible_count_is_rejected_before_framing.
End SubstitutionSnapshotPrefix.
