From Stdlib Require Import List Bool Arith Lia.

Import ListNotations.
Set Implicit Arguments.

(**
  Executable, allocation-independent model of the RCI2 channel-image codec.

  The Rust implementation streams the following grammar from the canonical
  RPB3 PathMap endpoints:

    "RCI" 02
    (zero_escape(relative_RPB3_key) 00 00 multiplicity:u64-BE)*
    00 00 distinct_count:u64-BE flags:u8

  [flags] admits exactly 0 and 1; bit zero is root [connective_used].  Root
  [locally_free] is deliberately quotiented out because it is a derived cache
  and is absent from [Par::{Eq,Ord,Hash}].

  This development models every framing and scalar check performed by RCI2.
  The one deliberately explicit bridge boundary is the generated-Rust RPB3
  endpoint validator.  Rather than assuming anything about that validator, all
  results quantify universally over its total Boolean decision procedure
  [canonical_rpb3].  Consequently the capstone theorems have no axioms: they
  hold for every possible implementation of that admission decision.  Rust
  differential tests are responsible only for showing that
  [validate_canonical_key] is the decision procedure instantiated there.
*)

Definition byte := nat.
Definition bytes := list byte.

Definition rci2_domain : bytes := [82; 67; 73; 2].
Definition byte_ok (value : byte) : bool := value <? 256.
Definition bytes_ok (value : bytes) : bool := forallb byte_ok value.

Fixpoint consume_exact (expected input : bytes) : option bytes :=
  match expected, input with
  | [], rest => Some rest
  | expected_byte :: expected_rest, actual_byte :: actual_rest =>
      if expected_byte =? actual_byte
      then consume_exact expected_rest actual_rest
      else None
  | _ :: _, [] => None
  end.

Lemma consume_exact_app :
  forall prefix suffix,
    consume_exact prefix (prefix ++ suffix) = Some suffix.
Proof.
  induction prefix as [|head tail induction]; intros suffix; simpl.
  - reflexivity.
  - rewrite Nat.eqb_refl. exact (induction suffix).
Qed.

(** Fixed-width big-endian words.  This recursive definition emits the low
    [width] base-256 digits, most significant first. *)
Fixpoint encode_fixed_be (width value : nat) : bytes :=
  match width with
  | 0 => []
  | S smaller => encode_fixed_be smaller (value / 256) ++ [value mod 256]
  end.

Fixpoint decode_fixed_be (width : nat) (input : bytes)
    : option (nat * bytes) :=
  match width with
  | 0 => Some (0, input)
  | S smaller =>
      match decode_fixed_be smaller input with
      | Some (prefix, current :: rest) =>
          if current <? 256
          then Some (prefix * 256 + current, rest)
          else None
      | _ => None
      end
  end.

Definition encode_u64_be := encode_fixed_be 8.
Definition decode_u64_be := decode_fixed_be 8.
Definition u64_bound : nat := 256 ^ 8.

(** An executable width check uses eight divisions instead of materializing
    the unary numeral [256^8]. *)
Fixpoint fits_fixed_be (width value : nat) : bool :=
  match width with
  | 0 => value =? 0
  | S smaller => fits_fixed_be smaller (value / 256)
  end.

Lemma encode_fixed_be_length :
  forall width value, length (encode_fixed_be width value) = width.
Proof.
  induction width as [|width induction]; intros value; simpl.
  - reflexivity.
  - rewrite length_app, induction. simpl. lia.
Qed.

Lemma decode_encode_fixed_be :
  forall width value suffix,
    value < 256 ^ width ->
    decode_fixed_be width (encode_fixed_be width value ++ suffix) =
      Some (value, suffix).
Proof.
  induction width as [|width induction]; intros value suffix bounded.
  - simpl in bounded. assert (value = 0) by lia. subst. reflexivity.
  - simpl encode_fixed_be.
    rewrite Nat.pow_succ_r in bounded by lia.
    assert (quotient_bound : value / 256 < 256 ^ width).
    {
      apply Nat.Private_NDivProp.div_lt_upper_bound; [lia|].
      nia.
    }
    simpl decode_fixed_be.
    rewrite <- app_assoc, induction by exact quotient_bound.
    change
      ((if value mod 256 <? 256
        then Some ((value / 256) * 256 + value mod 256, suffix)
        else None) = Some (value, suffix)).
    assert (remainder_bound : value mod 256 < 256).
    { apply Nat.mod_upper_bound. lia. }
    destruct (value mod 256 <? 256) eqn:check.
    + pose proof (Nat.div_mod value 256 ltac:(lia)) as division.
      assert ((value / 256) * 256 + value mod 256 = value) by lia.
      now rewrite H.
    + apply Nat.ltb_ge in check. lia.
Qed.

Theorem u64_big_endian_round_trip :
  forall value suffix,
    value < u64_bound ->
    decode_u64_be (encode_u64_be value ++ suffix) = Some (value, suffix).
Proof. exact (decode_encode_fixed_be 8). Qed.

Theorem u64_big_endian_word_has_exactly_eight_bytes :
  forall value, length (encode_u64_be value) = 8.
Proof. exact (encode_fixed_be_length 8). Qed.

Lemma fits_fixed_be_true_iff :
  forall width value,
    fits_fixed_be width value = true <-> value < 256 ^ width.
Proof.
  induction width as [|width induction]; intro value.
  - cbn [fits_fixed_be]. change ((value =? 0) = true <-> value < 1).
    rewrite Nat.eqb_eq. lia.
  - cbn [fits_fixed_be]. rewrite induction.
    rewrite Nat.pow_succ_r by lia. split.
    + intro quotient_bound.
      pose proof (Nat.div_mod value 256 ltac:(lia)) as division.
      pose proof (Nat.mod_upper_bound value 256 ltac:(lia)) as remainder_bound.
      nia.
    + intro value_bound.
      apply Nat.Private_NDivProp.div_lt_upper_bound; [lia|]. nia.
Qed.

(** RCI2 reserves [00 00] for a frame terminator and encodes a literal zero
    as [00 FF].  Nonzero admitted bytes pass through unchanged. *)
Fixpoint zero_escape (input : bytes) : bytes :=
  match input with
  | [] => []
  | current :: rest =>
      if current =? 0
      then 0 :: 255 :: zero_escape rest
      else current :: zero_escape rest
  end.

Fixpoint take_escaped_frame (input : bytes) : option (bytes * bytes) :=
  match input with
  | [] => None
  | current :: rest =>
      if current =? 0 then
        match rest with
        | [] => None
        | escaped :: suffix =>
            if escaped =? 255 then
              match take_escaped_frame suffix with
              | Some (payload, trailing) => Some (0 :: payload, trailing)
              | None => None
              end
            else if escaped =? 0
                 then Some ([], suffix)
                 else None
        end
      else if current <? 256 then
        match take_escaped_frame rest with
        | Some (payload, trailing) => Some (current :: payload, trailing)
        | None => None
        end
      else None
  end.

Lemma take_zero_escaped_frame_round_trip :
  forall payload suffix,
    bytes_ok payload = true ->
    take_escaped_frame (zero_escape payload ++ ([0; 0] ++ suffix)) =
      Some (payload, suffix).
Proof.
  induction payload as [|current payload induction]; intros suffix valid; simpl.
  - reflexivity.
  - apply andb_true_iff in valid as [current_valid payload_valid].
    unfold byte_ok in current_valid.
    destruct (current =? 0) eqn:is_zero.
    + apply Nat.eqb_eq in is_zero. subst current. simpl.
      change
        (match take_escaped_frame
           (zero_escape payload ++ ([0; 0] ++ suffix)) with
         | Some (decoded, trailing) => Some (0 :: decoded, trailing)
         | None => None
         end = Some (0 :: payload, suffix)).
      rewrite (induction suffix payload_valid). reflexivity.
    + simpl. rewrite is_zero, current_valid.
      change
        (match take_escaped_frame
           (zero_escape payload ++ ([0; 0] ++ suffix)) with
         | Some (decoded, trailing) => Some (current :: decoded, trailing)
         | None => None
         end = Some (current :: payload, suffix)).
      rewrite (induction suffix payload_valid). reflexivity.
Qed.

Lemma zero_escape_nonzero_head :
  forall head tail,
    head <> 0 -> zero_escape (head :: tail) = head :: zero_escape tail.
Proof.
  intros head tail nonzero. simpl.
  destruct (head =? 0) eqn:is_zero.
  - apply Nat.eqb_eq in is_zero. contradiction.
  - reflexivity.
Qed.

Fixpoint bytes_ltb (left right : bytes) : bool :=
  match left, right with
  | [], [] => false
  | [], _ :: _ => true
  | _ :: _, [] => false
  | left_head :: left_tail, right_head :: right_tail =>
      if left_head <? right_head then true
      else if right_head <? left_head then false
      else bytes_ltb left_tail right_tail
  end.

Lemma bytes_ltb_irreflexive :
  forall value, bytes_ltb value value = false.
Proof.
  induction value as [|head tail induction]; simpl.
  - reflexivity.
  - rewrite Nat.ltb_irrefl. exact induction.
Qed.

Record endpoint : Type := Endpoint {
  endpoint_key : bytes;
  endpoint_multiplicity : nat
}.

Definition first_byte_nonzero (key : bytes) : bool :=
  match key with
  | [] => false
  | head :: _ => negb (head =? 0)
  end.

Section ChannelImage.

(** The universally quantified admission decider is precisely the proof
    boundary around Rust's [validate_canonical_key].  No semantic property of
    this function is assumed anywhere below. *)
Variable canonical_rpb3 : bytes -> bool.

Definition endpoint_scalar_valid (entry : endpoint) : bool :=
  bytes_ok (endpoint_key entry) &&
  first_byte_nonzero (endpoint_key entry) &&
  canonical_rpb3 (endpoint_key entry) &&
  (0 <? endpoint_multiplicity entry) &&
  fits_fixed_be 8 (endpoint_multiplicity entry).

Definition endpoint_after (previous : option bytes) (entry : endpoint) : bool :=
  match previous with
  | None => true
  | Some key => bytes_ltb key (endpoint_key entry)
  end.

Fixpoint canonical_endpoints_from
    (previous : option bytes) (entries : list endpoint) : bool :=
  match entries with
  | [] => true
  | entry :: rest =>
      endpoint_scalar_valid entry &&
      endpoint_after previous entry &&
      canonical_endpoints_from (Some (endpoint_key entry)) rest
  end.

Record semantic_par : Type := SemanticPar {
  par_endpoints : list endpoint;
  par_connective_used : bool;
  par_locally_free : list nat
}.

Definition normalize_par (value : semantic_par) : semantic_par :=
  SemanticPar (par_endpoints value) (par_connective_used value) [].

Definition semantic_par_eq (left right : semantic_par) : Prop :=
  par_endpoints left = par_endpoints right /\
  par_connective_used left = par_connective_used right.

Definition canonical_par (value : semantic_par) : bool :=
  canonical_endpoints_from None (par_endpoints value) &&
  (length (par_endpoints value) <? u64_bound).

Definition encode_endpoint_frame (entry : endpoint) : bytes :=
  zero_escape (endpoint_key entry) ++ [0; 0] ++
  encode_u64_be (endpoint_multiplicity entry).

Definition encode_endpoint_stream
    (entries : list endpoint) (trailer : bytes) : bytes :=
  fold_right
    (fun entry suffix => encode_endpoint_frame entry ++ suffix)
    trailer entries.

Definition connective_flag (used : bool) : byte := if used then 1 else 0.

Definition encode_channel_with_suffix
    (value : semantic_par) (suffix : bytes) : bytes :=
  rci2_domain ++
  encode_endpoint_stream
    (par_endpoints value)
    ([0; 0] ++ encode_u64_be (length (par_endpoints value)) ++
     [connective_flag (par_connective_used value)] ++ suffix).

Definition encode_channel (value : semantic_par) : bytes :=
  encode_channel_with_suffix value [].

Lemma encode_endpoint_stream_append :
  forall entries trailer suffix,
    encode_endpoint_stream entries (trailer ++ suffix) =
    encode_endpoint_stream entries trailer ++ suffix.
Proof.
  induction entries as [|entry rest induction]; intros trailer suffix.
  - reflexivity.
  - change
      (encode_endpoint_frame entry ++
         encode_endpoint_stream rest (trailer ++ suffix) =
       (encode_endpoint_frame entry ++ encode_endpoint_stream rest trailer) ++ suffix).
    rewrite induction. apply app_assoc.
Qed.

Lemma encode_channel_with_suffix_is_append :
  forall value suffix,
    encode_channel_with_suffix value suffix = encode_channel value ++ suffix.
Proof.
  intros [entries connective locally_free] suffix.
  set (trailer := [0; 0] ++ encode_u64_be (length entries) ++
                  [connective_flag connective]).
  change
    (rci2_domain ++ encode_endpoint_stream entries (trailer ++ suffix) =
     (rci2_domain ++ encode_endpoint_stream entries trailer) ++ suffix).
  rewrite encode_endpoint_stream_append. apply app_assoc.
Qed.

Fixpoint decode_endpoint_stream
    (fuel : nat) (previous : option bytes) (input : bytes)
    : option (list endpoint * bytes) :=
  match fuel with
  | 0 => None
  | S remaining =>
      match take_escaped_frame input with
      | None => None
      | Some ([], rest) => Some ([], rest)
      | Some (key, rest) =>
          match decode_u64_be rest with
          | None => None
          | Some (multiplicity, suffix) =>
              let entry := Endpoint key multiplicity in
              if endpoint_scalar_valid entry && endpoint_after previous entry
              then
                match decode_endpoint_stream remaining (Some key) suffix with
                | Some (entries, trailing) => Some (entry :: entries, trailing)
                | None => None
                end
              else None
          end
      end
  end.

Definition decode_connective_flag (input : bytes) : option (bool * bytes) :=
  match input with
  | 0 :: rest => Some (false, rest)
  | 1 :: rest => Some (true, rest)
  | _ => None
  end.

Definition decode_channel_prefix (input : bytes)
    : option (semantic_par * bytes) :=
  match consume_exact rci2_domain input with
  | None => None
  | Some body =>
      match decode_endpoint_stream (S (length body)) None body with
      | None => None
      | Some (entries, count_and_flag) =>
          match decode_u64_be count_and_flag with
          | None => None
          | Some (declared_count, flag_and_suffix) =>
              if declared_count =? length entries then
                match decode_connective_flag flag_and_suffix with
                | Some (connective, suffix) =>
                    Some (SemanticPar entries connective [], suffix)
                | None => None
                end
              else None
          end
      end
  end.

Definition decode_channel_exact (input : bytes) : option semantic_par :=
  match decode_channel_prefix input with
  | Some (value, []) => Some value
  | _ => None
  end.

Lemma endpoint_stream_length_lower_bound :
  forall entries trailer,
    length entries <= length (encode_endpoint_stream entries trailer).
Proof.
  induction entries as [|entry rest induction]; intros trailer.
  - change (0 <= length trailer). lia.
  - change
      (S (length rest) <=
       length (encode_endpoint_frame entry ++
               encode_endpoint_stream rest trailer)).
    rewrite length_app. specialize (induction trailer).
    assert (frame_nonempty : 1 <= length (encode_endpoint_frame entry)).
    {
      unfold encode_endpoint_frame.
      repeat rewrite length_app.
      rewrite u64_big_endian_word_has_exactly_eight_bytes. simpl. lia.
    }
    lia.
Qed.

Lemma decode_endpoint_stream_encode :
  forall entries previous trailer fuel,
    canonical_endpoints_from previous entries = true ->
    length entries < fuel ->
    decode_endpoint_stream fuel previous
      (encode_endpoint_stream entries ([0; 0] ++ trailer)) =
      Some (entries, trailer).
Proof.
  induction entries as [|entry rest induction];
    intros previous trailer fuel canonical enough.
  - destruct fuel; [lia|]. simpl. reflexivity.
  - destruct fuel as [|fuel]; [lia|].
    simpl in canonical.
    apply andb_true_iff in canonical as [entry_and_after rest_canonical].
    apply andb_true_iff in entry_and_after as [entry_valid after_valid].
    pose proof entry_valid as scalar_valid.
    unfold endpoint_scalar_valid in entry_valid.
    repeat rewrite andb_true_iff in entry_valid.
    destruct entry_valid as [[[[key_bytes key_nonzero] admitted] multiplicity_positive]
      multiplicity_bound].
    destruct entry as [key multiplicity]. simpl in *.
    destruct key as [|head tail].
    { discriminate. }
    apply Bool.negb_true_iff, Nat.eqb_neq in key_nonzero.
    change
      (decode_endpoint_stream (S fuel) previous
        (encode_endpoint_frame (Endpoint (head :: tail) multiplicity) ++
         encode_endpoint_stream rest ([0; 0] ++ trailer)) =
       Some (Endpoint (head :: tail) multiplicity :: rest, trailer)).
    unfold encode_endpoint_frame at 1. simpl endpoint_key. simpl endpoint_multiplicity.
    repeat rewrite <- app_assoc.
    cbn [decode_endpoint_stream].
    rewrite take_zero_escaped_frame_round_trip.
    2: exact key_bytes.
    rewrite u64_big_endian_round_trip by
      (unfold u64_bound;
       apply (proj1 (fits_fixed_be_true_iff 8 multiplicity));
       exact multiplicity_bound).
    simpl. rewrite scalar_valid, after_valid. simpl.
    rewrite induction with (fuel := fuel); auto. lia.
Qed.

Theorem channel_prefix_parser_is_left_inverse :
  forall value suffix,
    canonical_par value = true ->
    decode_channel_prefix (encode_channel value ++ suffix) =
      Some (normalize_par value, suffix).
Proof.
  intros [entries connective locally_free] suffix canonical.
  unfold canonical_par in canonical. simpl in canonical.
  apply andb_true_iff in canonical as [entries_canonical count_bound].
  rewrite <- encode_channel_with_suffix_is_append.
  unfold encode_channel_with_suffix, decode_channel_prefix.
  rewrite consume_exact_app.
  set (trailer := encode_u64_be (length entries) ++
                  [connective_flag connective] ++ suffix).
  rewrite decode_endpoint_stream_encode with (trailer := trailer).
  2: exact entries_canonical.
  2: {
    pose proof (endpoint_stream_length_lower_bound entries
      ([0; 0] ++ trailer)).
    change
      (length entries <
       S (length (encode_endpoint_stream entries ([0; 0] ++ trailer)))).
    lia.
  }
  unfold trailer.
  rewrite u64_big_endian_round_trip.
  2: { apply Nat.ltb_lt. exact count_bound. }
  rewrite Nat.eqb_refl. destruct connective; reflexivity.
Qed.

Theorem channel_exact_parser_is_left_inverse :
  forall value,
    canonical_par value = true ->
    decode_channel_exact (encode_channel value) = Some (normalize_par value).
Proof.
  intros value canonical. unfold decode_channel_exact.
  pose proof
    (channel_prefix_parser_is_left_inverse value [] canonical) as decoded.
  rewrite app_nil_r in decoded. rewrite decoded. reflexivity.
Qed.

Theorem root_locally_free_is_quotiented_out :
  forall entries connective left_cache right_cache,
    encode_channel (SemanticPar entries connective left_cache) =
    encode_channel (SemanticPar entries connective right_cache).
Proof. reflexivity. Qed.

Theorem canonical_channel_image_is_unique_iff_semantic_par_equality :
  forall left right,
    canonical_par left = true ->
    canonical_par right = true ->
    (encode_channel left = encode_channel right <-> semantic_par_eq left right).
Proof.
  intros left right left_canonical right_canonical. split.
  - intro encoded_equal.
    pose proof (channel_exact_parser_is_left_inverse left left_canonical) as left_decoded.
    pose proof (channel_exact_parser_is_left_inverse right right_canonical) as right_decoded.
    rewrite encoded_equal in left_decoded. rewrite right_decoded in left_decoded.
    injection left_decoded as normalized_equal.
    destruct left as [left_entries left_connective left_cache].
    destruct right as [right_entries right_connective right_cache].
    unfold semantic_par_eq. simpl. split; symmetry; assumption.
  - intros [entries_equal connective_equal].
    destruct left as [left_entries left_connective left_cache].
    destruct right as [right_entries right_connective right_cache].
    simpl in *. subst. apply root_locally_free_is_quotiented_out.
Qed.

Theorem connective_flag_separates_otherwise_equal_channels :
  forall entries left_cache right_cache,
    canonical_par (SemanticPar entries false left_cache) = true ->
    canonical_par (SemanticPar entries true right_cache) = true ->
    encode_channel (SemanticPar entries false left_cache) <>
    encode_channel (SemanticPar entries true right_cache).
Proof.
  intros entries left_cache right_cache left_valid right_valid same.
  pose proof (channel_exact_parser_is_left_inverse
    (SemanticPar entries false left_cache) left_valid) as left_decoded.
  pose proof (channel_exact_parser_is_left_inverse
    (SemanticPar entries true right_cache) right_valid) as right_decoded.
  rewrite same in left_decoded. rewrite right_decoded in left_decoded.
  discriminate.
Qed.

Theorem endpoint_multiplicities_are_preserved_exactly :
  forall value decoded,
    canonical_par value = true ->
    decode_channel_exact (encode_channel value) = Some decoded ->
    map endpoint_multiplicity (par_endpoints decoded) =
    map endpoint_multiplicity (par_endpoints value).
Proof.
  intros value decoded canonical decoded_ok.
  rewrite channel_exact_parser_is_left_inverse in decoded_ok by exact canonical.
  injection decoded_ok as same. subst decoded. reflexivity.
Qed.

(** An explicit one-step equation prevents proof automation from unfolding
    the recursive decoder below the current PDA state. *)
Lemma decode_endpoint_stream_succ_unfold :
  forall fuel previous input,
    decode_endpoint_stream (S fuel) previous input =
      match take_escaped_frame input with
      | None => None
      | Some ([], rest) => Some ([], rest)
      | Some (key, rest) =>
          match decode_u64_be rest with
          | None => None
          | Some (multiplicity, suffix) =>
              let entry := Endpoint key multiplicity in
              if endpoint_scalar_valid entry && endpoint_after previous entry
              then
                match decode_endpoint_stream fuel (Some key) suffix with
                | Some (entries, trailing) => Some (entry :: entries, trailing)
                | None => None
                end
              else None
          end
      end.
Proof. reflexivity. Qed.

Lemma recursive_endpoint_some_inv :
  forall fuel key trailing entry entries suffix,
    (match decode_endpoint_stream fuel (Some key) trailing with
     | Some (tail_entries, tail_suffix) =>
         Some (entry :: tail_entries, tail_suffix)
     | None => None
     end) = Some (entries, suffix) ->
    exists tail_entries tail_suffix,
      decode_endpoint_stream fuel (Some key) trailing =
        Some (tail_entries, tail_suffix) /\
      entries = entry :: tail_entries /\
      suffix = tail_suffix.
Proof.
  intros fuel key trailing entry entries suffix decoded.
  destruct (decode_endpoint_stream fuel (Some key) trailing)
    as [[tail_entries tail_suffix]|] eqn:tail_decoded.
  - exists tail_entries, tail_suffix. split; [reflexivity|].
    injection decoded. auto.
  - discriminate.
Qed.

Lemma decoded_endpoint_stream_is_canonical :
  forall fuel previous input entries suffix,
    decode_endpoint_stream fuel previous input = Some (entries, suffix) ->
    canonical_endpoints_from previous entries = true.
Proof.
  induction fuel as [|fuel induction]; intros previous input entries suffix decoded.
  - discriminate.
  - rewrite decode_endpoint_stream_succ_unfold in decoded.
    destruct (take_escaped_frame input) as [[key rest]|] eqn:frame.
    2: { discriminate. }
    destruct key as [|head tail].
    + injection decoded as entries_equal suffix_equal. subst. reflexivity.
    + destruct (decode_u64_be rest) as [[multiplicity trailing]|] eqn:word.
      2: { discriminate. }
      cbn in decoded.
      set (entry := Endpoint (head :: tail) multiplicity) in *.
      destruct (endpoint_scalar_valid entry && endpoint_after previous entry)
        eqn:valid.
      2: { discriminate. }
      apply recursive_endpoint_some_inv in decoded.
      destruct decoded as
        [decoded_tail [final_suffix [tail_decoded [entries_equal suffix_equal]]]].
      subst entries suffix.
      simpl. change
        ((endpoint_scalar_valid entry && endpoint_after previous entry) &&
         canonical_endpoints_from (Some (head :: tail)) decoded_tail = true).
      rewrite valid. simpl.
      eapply induction. exact tail_decoded.
Qed.

Theorem every_successfully_decoded_channel_has_canonical_endpoints :
  forall input value suffix,
    decode_channel_prefix input = Some (value, suffix) ->
    canonical_endpoints_from None (par_endpoints value) = true.
Proof.
  intros input value suffix decoded.
  unfold decode_channel_prefix in decoded.
  destruct (consume_exact rci2_domain input) as [body|] eqn:domain;
    [|discriminate].
  destruct (decode_endpoint_stream (S (length body)) None body)
    as [[entries count_and_flag]|] eqn:endpoints; [|discriminate].
  destruct (decode_u64_be count_and_flag)
    as [[declared flag_and_suffix]|] eqn:count; [|discriminate].
  destruct (declared =? length entries) eqn:count_ok; [|discriminate].
  destruct (decode_connective_flag flag_and_suffix)
    as [[connective trailing]|] eqn:flag; [|discriminate].
  injection decoded as value_equal suffix_equal. subst value suffix.
  simpl. eapply decoded_endpoint_stream_is_canonical. exact endpoints.
Qed.

(** Duplicate keys are rejected by the strict-order boundary.  This lemma is
    independent of protobuf meaning: even if the external RPB3 decider admits
    the key, a second identical endpoint can never pass. *)
Theorem duplicate_endpoint_fails_strict_order_check :
  forall key multiplicity,
    endpoint_after (Some key) (Endpoint key multiplicity) = false.
Proof. intros. unfold endpoint_after. simpl. apply bytes_ltb_irreflexive. Qed.

(** A reversed adjacent pair fails at the same explicit boundary whenever the
    Boolean lexicographic order says the second does not follow the first. *)
Theorem permuted_endpoint_fails_strict_order_check :
  forall first_key second_key multiplicity,
    bytes_ltb first_key second_key = false ->
    endpoint_after (Some first_key) (Endpoint second_key multiplicity) = false.
Proof. intros. exact H. Qed.

Theorem noncanonical_rpb3_endpoint_fails_admission_check :
  forall key multiplicity,
    canonical_rpb3 key = false ->
    endpoint_scalar_valid (Endpoint key multiplicity) = false.
Proof.
  intros key multiplicity rejected.
  unfold endpoint_scalar_valid. simpl. rewrite rejected.
  repeat rewrite andb_false_r. reflexivity.
Qed.

Theorem zero_multiplicity_fails_admission_check :
  forall key, endpoint_scalar_valid (Endpoint key 0) = false.
Proof.
  intro key. unfold endpoint_scalar_valid. simpl.
  repeat rewrite andb_false_r. reflexivity.
Qed.

End ChannelImage.

(** Concrete end-to-end negative controls exercise the parser, not merely the
    predicates: duplicate, reversed, noncanonical-leading-zero, zero-count,
    and reserved-flag images are all rejected. *)
Definition minimal_rpb3_decider (key : bytes) : bool :=
  bytes_ok key && first_byte_nonzero key.

Example duplicate_endpoint_image_is_rejected :
  @decode_channel_exact minimal_rpb3_decider
    (encode_channel
      (SemanticPar [Endpoint [1] 1; Endpoint [1] 2] false [])) = None.
Proof. vm_compute. reflexivity. Qed.

Example permuted_endpoint_image_is_rejected :
  @decode_channel_exact minimal_rpb3_decider
    (encode_channel
      (SemanticPar [Endpoint [2] 1; Endpoint [1] 1] false [])) = None.
Proof. vm_compute. reflexivity. Qed.

Example leading_zero_endpoint_image_is_rejected :
  @decode_channel_exact minimal_rpb3_decider
    (encode_channel
      (SemanticPar [Endpoint [0; 1] 1] false [])) = None.
Proof. vm_compute. reflexivity. Qed.

Example zero_multiplicity_image_is_rejected :
  @decode_channel_exact minimal_rpb3_decider
    (encode_channel
      (SemanticPar [Endpoint [1] 0] false [])) = None.
Proof. vm_compute. reflexivity. Qed.

Example reserved_connective_flag_is_rejected :
  @decode_channel_exact minimal_rpb3_decider
    (rci2_domain ++ [0; 0] ++ encode_u64_be 0 ++ [2]) = None.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions channel_prefix_parser_is_left_inverse.
Print Assumptions channel_exact_parser_is_left_inverse.
Print Assumptions canonical_channel_image_is_unique_iff_semantic_par_equality.
Print Assumptions root_locally_free_is_quotiented_out.
Print Assumptions connective_flag_separates_otherwise_equal_channels.
Print Assumptions endpoint_multiplicities_are_preserved_exactly.
Print Assumptions every_successfully_decoded_channel_has_canonical_endpoints.
Print Assumptions duplicate_endpoint_fails_strict_order_check.
Print Assumptions permuted_endpoint_fails_strict_order_check.
Print Assumptions noncanonical_rpb3_endpoint_fails_admission_check.
Print Assumptions zero_multiplicity_fails_admission_check.
