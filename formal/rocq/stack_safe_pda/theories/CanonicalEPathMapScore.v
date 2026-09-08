From Stdlib Require Import List Bool Arith Lia NArith.
From StackSafePDA Require Import ChannelImage RhoPathV1Datum CanonicalEPathMapImport.

Import ListNotations.
Set Implicit Arguments.

Module CanonicalEPathMapScore.

Module Frame := RhoPathV1Datum.RhoPathV1Datum.

Definition bytes := list nat.

Inductive integer_atom := Constructor13 | ConnectiveFlag (value : bool).

Definition integer_value (value : integer_atom) : nat :=
  match value with Constructor13 => 13 | ConnectiveFlag false => 0 | ConnectiveFlag true => 1 end.

Definition semantic_integer_bytes (value : integer_atom) : bytes :=
  [1; 1; 128] ++ ChannelImage.encode_fixed_be 7 (integer_value value).

Definition direct_integer_bytes (value : integer_atom) : bytes :=
  [1; 1; 128; 0; 0; 0; 0; 0; 0; integer_value value].

Theorem specialized_integer_atoms_have_exact_bytes :
  forall value, semantic_integer_bytes value = direct_integer_bytes value.
Proof. intros [|[]]; reflexivity. Qed.

Definition direct_terminated_bytes (input : bytes) : bytes :=
  if existsb (Nat.eqb 0) input
  then Frame.escape_frame_payload input ++ [0; 0]
  else input ++ [0; 0].

Lemma no_zero_escape_is_identity : forall input,
  existsb (Nat.eqb 0) input = false -> Frame.escape_frame_payload input = input.
Proof.
  induction input as [|head tail induction]; intro absent; [reflexivity|].
  cbn [existsb] in absent. apply orb_false_iff in absent as [head_nonzero tail_absent].
  cbn [Frame.escape_frame_payload]. unfold Frame.frame_escape.
  rewrite Nat.eqb_sym, head_nonzero, induction by exact tail_absent. reflexivity.
Qed.

Theorem direct_zero_escaping_matches_exact_frame : forall input,
  direct_terminated_bytes input = Frame.encode_frame input.
Proof.
  intro input. unfold direct_terminated_bytes, Frame.encode_frame.
  destruct (existsb (Nat.eqb 0) input) eqn:has_zero; [reflexivity|].
  now rewrite no_zero_escape_is_identity by exact has_zero.
Qed.

Inductive score_atom := IntegerAtom (value : integer_atom) | SnapshotAtom (snapshot : bytes).

Definition serialize_atom (atom : score_atom) : bytes :=
  match atom with
  | IntegerAtom value => semantic_integer_bytes value
  | SnapshotAtom snapshot => [1; 3] ++ Frame.encode_frame snapshot
  end.

Definition serialize_node (children : list score_atom) : bytes :=
  [2] ++ concat (map serialize_atom children) ++ [0].

Definition rpb3_pathmap_semantic_score (snapshot : bytes) (connective : bool) : list score_atom :=
  [IntegerAtom Constructor13; SnapshotAtom snapshot; IntegerAtom (ConnectiveFlag connective)].

Definition direct_score (snapshot : bytes) (connective : bool) : bytes :=
  [2] ++ direct_integer_bytes Constructor13 ++ [1; 3] ++
  direct_terminated_bytes snapshot ++ direct_integer_bytes (ConnectiveFlag connective) ++ [0].

Theorem direct_score_refines_fixed_shape_semantic_serialization :
  forall snapshot connective,
    direct_score snapshot connective = serialize_node (rpb3_pathmap_semantic_score snapshot connective).
Proof.
  intros snapshot connective. unfold direct_score, serialize_node, rpb3_pathmap_semantic_score.
  cbn [map concat serialize_atom]. rewrite !specialized_integer_atoms_have_exact_bytes.
  rewrite direct_zero_escaping_matches_exact_frame, app_nil_r.
  repeat rewrite app_assoc. reflexivity.
Qed.

Fixpoint zero_count (input : bytes) : nat :=
  match input with [] => 0 | head :: tail => (if head =? 0 then 1 else 0) + zero_count tail end.

Lemma zero_count_bounded : forall input, zero_count input <= length input.
Proof.
  induction input as [|head tail induction]; [reflexivity|].
  cbn [zero_count length]. destruct (head =? 0); lia.
Qed.

Lemma escaped_payload_exact_length : forall input,
  length (Frame.escape_frame_payload input) = length input + zero_count input.
Proof.
  induction input as [|head tail induction]; [reflexivity|].
  cbn [Frame.escape_frame_payload zero_count length]. unfold Frame.frame_escape.
  destruct (head =? 0); cbn [length]; rewrite induction; lia.
Qed.

Theorem direct_score_has_exact_length : forall snapshot connective,
  length (direct_score snapshot connective) = 26 + length snapshot + zero_count snapshot.
Proof.
  intros snapshot connective. unfold direct_score.
  rewrite direct_zero_escaping_matches_exact_frame. unfold Frame.encode_frame.
  repeat rewrite length_app. rewrite escaped_payload_exact_length.
  destruct connective; cbn [direct_integer_bytes integer_value length]; unfold Frame.byte; lia.
Qed.

Definition endpoint_domain : bytes := [82; 80; 66; 3; 5].

Definition endpoint_key (snapshot : bytes) (connective : bool) (protobuf_expr : bytes) : bytes :=
  let score := direct_score snapshot connective in
  endpoint_domain ++ score ++ protobuf_expr ++ ChannelImage.encode_u64_be (length score).

Theorem endpoint_has_exact_bytes_and_length : forall snapshot connective protobuf_expr,
  endpoint_key snapshot connective protobuf_expr =
    endpoint_domain ++ serialize_node (rpb3_pathmap_semantic_score snapshot connective) ++
      protobuf_expr ++ ChannelImage.encode_u64_be (26 + length snapshot + zero_count snapshot) /\
  length (endpoint_key snapshot connective protobuf_expr) =
    39 + length snapshot + zero_count snapshot + length protobuf_expr.
Proof.
  intros snapshot connective protobuf_expr. split.
  - unfold endpoint_key. now rewrite direct_score_has_exact_length,
      direct_score_refines_fixed_shape_semantic_serialization.
  - unfold endpoint_key. repeat rewrite length_app.
    rewrite ChannelImage.u64_big_endian_word_has_exactly_eight_bytes, direct_score_has_exact_length.
    cbn [endpoint_domain length]. lia.
Qed.

Definition split_endpoint_suffix (suffix : bytes) : option (bytes * bytes) :=
  if length suffix <? 8 then None else
  let body_length := length suffix - 8 in
  match ChannelImage.decode_u64_be (skipn body_length suffix) with
  | Some (score_length, []) =>
      if score_length <=? body_length then
        let body := firstn body_length suffix in
        Some (firstn score_length body, skipn score_length body)
      else None
  | _ => None
  end.

Definition split_endpoint (input : bytes) : option (bytes * bytes) :=
  match ChannelImage.consume_exact endpoint_domain input with
  | Some suffix => split_endpoint_suffix suffix
  | None => None
  end.

Lemma take_exact_prefix : forall (prefix suffix : bytes),
  firstn (length prefix) (prefix ++ suffix) = prefix.
Proof. intros. rewrite firstn_app, firstn_all, Nat.sub_diag. now rewrite app_nil_r. Qed.

Lemma drop_exact_prefix : forall (prefix suffix : bytes),
  skipn (length prefix) (prefix ++ suffix) = suffix.
Proof. intros. rewrite skipn_app, skipn_all, Nat.sub_diag. reflexivity. Qed.

Lemma split_endpoint_suffix_round_trip : forall score protobuf_expr,
  length score < ChannelImage.u64_bound ->
  split_endpoint_suffix (score ++ protobuf_expr ++ ChannelImage.encode_u64_be (length score)) =
    Some (score, protobuf_expr).
Proof.
  intros score protobuf_expr bounded. unfold split_endpoint_suffix.
  repeat rewrite length_app. rewrite ChannelImage.u64_big_endian_word_has_exactly_eight_bytes.
  assert (enough : (length score + (length protobuf_expr + 8) <? 8) = false)
    by (apply Nat.ltb_ge; lia).
  rewrite enough.
  replace (length score + (length protobuf_expr + 8) - 8)
    with (length (score ++ protobuf_expr)) by (rewrite length_app; lia).
  rewrite app_assoc, drop_exact_prefix.
  pose proof (@ChannelImage.u64_big_endian_round_trip (length score) [] bounded) as trailer.
  rewrite app_nil_r in trailer. rewrite trailer.
  assert (within : (length score <=? length (score ++ protobuf_expr)) = true)
    by (apply Nat.leb_le; rewrite length_app; lia).
  rewrite within, take_exact_prefix, take_exact_prefix, drop_exact_prefix. reflexivity.
Qed.

Theorem endpoint_trailer_recovers_exact_score_and_protobuf :
  forall snapshot connective protobuf_expr,
    26 + length snapshot + zero_count snapshot < ChannelImage.u64_bound ->
    split_endpoint (endpoint_key snapshot connective protobuf_expr) =
      Some (direct_score snapshot connective, protobuf_expr).
Proof.
  intros snapshot connective protobuf_expr bounded. unfold split_endpoint, endpoint_key.
  rewrite ChannelImage.consume_exact_app. apply split_endpoint_suffix_round_trip.
  now rewrite direct_score_has_exact_length.
Qed.

Definition checked_add (limit left right : nat) : option nat :=
  if left + right <=? limit then Some (left + right) else None.

Definition checked_reserve (limit snapshot_length protobuf_length : nat) : option nat :=
  match checked_add limit snapshot_length snapshot_length with
  | None => None
  | Some doubled =>
      match checked_add limit 5 doubled with
      | None => None
      | Some header =>
          match checked_add limit header protobuf_length with
          | None => None
          | Some payload => checked_add limit payload 64
          end
      end
  end.

Theorem checked_reserve_exact_acceptance : forall limit snapshot_length protobuf_length capacity,
  checked_reserve limit snapshot_length protobuf_length = Some capacity <->
    capacity = 69 + 2 * snapshot_length + protobuf_length /\ capacity <= limit.
Proof.
  intros limit snapshot_length protobuf_length capacity.
  unfold checked_reserve, checked_add.
  repeat match goal with
  | |- context [if ?condition then _ else _] => destruct condition eqn:?
  end; split; intro accepted; try discriminate;
    repeat match goal with
    | hypothesis : (_ <=? _) = true |- _ => apply Nat.leb_le in hypothesis
    | hypothesis : (_ <=? _) = false |- _ => apply Nat.leb_gt in hypothesis
    end; try (inversion accepted; subst; lia); destruct accepted; subst; try lia;
    f_equal; lia.
Qed.

Theorem emitted_endpoint_fits_successful_reserve :
  forall limit snapshot connective protobuf_expr capacity,
    checked_reserve limit (length snapshot) (length protobuf_expr) = Some capacity ->
    length (endpoint_key snapshot connective protobuf_expr) <= capacity /\ capacity <= limit.
Proof.
  intros limit snapshot connective protobuf_expr capacity accepted.
  apply checked_reserve_exact_acceptance in accepted.
  pose proof (endpoint_has_exact_bytes_and_length snapshot connective protobuf_expr) as [_ size].
  pose proof (zero_count_bounded snapshot). lia.
Qed.

Definition score_header : bytes := [2; 1; 1; 128; 0; 0; 0; 0; 0; 0; 13; 1; 3].

Definition score_tail (connective : bool) : bytes :=
  direct_integer_bytes (ConnectiveFlag connective) ++ [0].

Lemma direct_score_layout : forall snapshot connective,
  direct_score snapshot connective = score_header ++ Frame.encode_frame snapshot ++ score_tail connective.
Proof.
  intros. unfold direct_score, score_header, score_tail.
  rewrite direct_zero_escaping_matches_exact_frame.
  unfold direct_integer_bytes, integer_value. repeat rewrite app_assoc. reflexivity.
Qed.

Definition parse_score_tail (input : bytes) : option (bool * bytes) :=
  match ChannelImage.consume_exact (score_tail false) input with
  | Some suffix => Some (false, suffix)
  | None => option_map (fun suffix => (true, suffix))
      (ChannelImage.consume_exact (score_tail true) input)
  end.

Definition parse_score (input : bytes) : option (bytes * bool * bytes) :=
  match ChannelImage.consume_exact score_header input with
  | None => None
  | Some body =>
      match Frame.parse_frame body with
      | None => None
      | Some (snapshot, remaining) =>
          option_map (fun parsed => (snapshot, fst parsed, snd parsed)) (parse_score_tail remaining)
      end
  end.

Lemma parse_score_tail_round_trip : forall connective suffix,
  parse_score_tail (score_tail connective ++ suffix) = Some (connective, suffix).
Proof.
  intros [] suffix; reflexivity.
Qed.

Theorem concrete_score_parser_round_trip : forall snapshot connective suffix,
  Frame.bytes_valid snapshot = true ->
  parse_score (direct_score snapshot connective ++ suffix) = Some (snapshot, connective, suffix).
Proof.
  intros snapshot connective suffix valid. rewrite direct_score_layout.
  unfold parse_score. rewrite <- !app_assoc, ChannelImage.consume_exact_app.
  rewrite Frame.frame_encode_parse_round_trip by exact valid.
  rewrite parse_score_tail_round_trip. reflexivity.
Qed.

Theorem fixed_shape_score_is_prefix_free : forall left left_flag right right_flag suffix,
  Frame.bytes_valid left = true -> Frame.bytes_valid right = true ->
  direct_score left left_flag ++ suffix = direct_score right right_flag ->
  left = right /\ left_flag = right_flag /\ suffix = [].
Proof.
  intros left left_flag right right_flag suffix valid_left valid_right same.
  pose proof (@concrete_score_parser_round_trip left left_flag suffix valid_left) as parsed_left.
  pose proof (@concrete_score_parser_round_trip right right_flag [] valid_right) as parsed_right.
  rewrite same in parsed_left. rewrite app_nil_r in parsed_right.
  rewrite parsed_right in parsed_left. inversion parsed_left. auto.
Qed.

Definition prefix (left right : bytes) : Prop := exists suffix, right = left ++ suffix.

Lemma bytes_ltb_common_prefix : forall common left right,
  ChannelImage.bytes_ltb (common ++ left) (common ++ right) = ChannelImage.bytes_ltb left right.
Proof.
  induction common as [|head tail induction]; intros; [reflexivity|].
  cbn [app ChannelImage.bytes_ltb]. rewrite Nat.ltb_irrefl. apply induction.
Qed.

Lemma bytes_ltb_suffixes_at_distinct_nonprefixes : forall left right left_suffix right_suffix,
  ~ prefix left right -> ~ prefix right left ->
  ChannelImage.bytes_ltb (left ++ left_suffix) (right ++ right_suffix) =
    ChannelImage.bytes_ltb left right.
Proof.
  induction left as [|head tail induction]; intros right left_suffix right_suffix no_left no_right.
  - exfalso. apply no_left. exists right. reflexivity.
  - destruct right as [|other rest].
    + exfalso. apply no_right. exists (head :: tail). reflexivity.
    + cbn [app ChannelImage.bytes_ltb].
      destruct (head <? other) eqn:first; [reflexivity|].
      destruct (other <? head) eqn:second; [reflexivity|].
      apply Nat.ltb_ge in first. apply Nat.ltb_ge in second.
      assert (head = other) by lia. subst other. apply induction.
      * intros [suffix equal]. apply no_left. exists suffix. now rewrite equal.
      * intros [suffix equal]. apply no_right. exists suffix. now rewrite equal.
Qed.

Theorem equal_score_endpoint_order_preserves_nonprefix_protobuf_order :
  forall snapshot connective left_protobuf right_protobuf,
    ~ prefix left_protobuf right_protobuf -> ~ prefix right_protobuf left_protobuf ->
    ChannelImage.bytes_ltb (endpoint_key snapshot connective left_protobuf)
      (endpoint_key snapshot connective right_protobuf) =
    ChannelImage.bytes_ltb left_protobuf right_protobuf.
Proof.
  intros. unfold endpoint_key. rewrite !bytes_ltb_common_prefix.
  now apply bytes_ltb_suffixes_at_distinct_nonprefixes.
Qed.

Theorem unequal_score_endpoint_order_is_score_order :
  forall left left_flag right right_flag left_protobuf right_protobuf,
    Frame.bytes_valid left = true -> Frame.bytes_valid right = true ->
    direct_score left left_flag <> direct_score right right_flag ->
    ChannelImage.bytes_ltb (endpoint_key left left_flag left_protobuf)
      (endpoint_key right right_flag right_protobuf) =
    ChannelImage.bytes_ltb (direct_score left left_flag) (direct_score right right_flag).
Proof.
  intros left left_flag right right_flag left_protobuf right_protobuf
    valid_left valid_right distinct.
  unfold endpoint_key. rewrite bytes_ltb_common_prefix.
  apply bytes_ltb_suffixes_at_distinct_nonprefixes.
  - intros [suffix equal]. symmetry in equal.
    pose proof (@fixed_shape_score_is_prefix_free left left_flag right right_flag suffix
      valid_left valid_right equal) as [same_snapshot [same_flag _]].
    subst. contradiction.
  - intros [suffix equal]. symmetry in equal.
    pose proof (@fixed_shape_score_is_prefix_free right right_flag left left_flag suffix
      valid_right valid_left equal) as [same_snapshot [same_flag _]].
    subst. contradiction.
Qed.

Theorem false_connective_score_precedes_true_independently_of_suffix :
  forall snapshot left_suffix right_suffix,
    ChannelImage.bytes_ltb (direct_score snapshot false ++ left_suffix)
      (direct_score snapshot true ++ right_suffix) = true.
Proof.
  intros. rewrite !direct_score_layout, <- !app_assoc.
  rewrite !bytes_ltb_common_prefix. reflexivity.
Qed.

Theorem concrete_byte_and_refusal_controls :
  direct_integer_bytes Constructor13 = [1; 1; 128; 0; 0; 0; 0; 0; 0; 13] /\
  direct_terminated_bytes [0; 255; 0] = [0; 255; 255; 0; 255; 0; 0] /\
  length (direct_score [0; 255; 0] true) = 31 /\
  checked_reserve 73 1 2 = Some 73 /\ checked_reserve 72 1 2 = None /\
  split_endpoint_suffix [1; 2; 3] = None /\
  split_endpoint_suffix [0; 0; 0; 0; 0; 0; 0; 1] = None /\
  parse_score (score_header ++ [0; 1]) = None.
Proof. vm_compute. repeat split; reflexivity. Qed.

Definition concrete_score_length (_ : Carrier.carrier) (parts : Envelope.envelope) : nat :=
  length (direct_score (Envelope.snapshot parts) (Envelope.connective_used parts)).

Lemma import_u64_bound_is_fixed_be_bound :
  N.of_nat ChannelImage.u64_bound = Carrier.u64_limit.
Proof.
  unfold ChannelImage.u64_bound. rewrite Nat2N.inj_pow. reflexivity.
Qed.

Lemma import_score_bound_exact : forall size,
  (N.of_nat size < Carrier.u64_limit)%N <-> size < ChannelImage.u64_bound.
Proof.
  intro size. rewrite <- import_u64_bound_is_fixed_be_bound.
  rewrite <- N.compare_lt_iff, <- Nat2N.inj_compare. apply Nat.compare_lt_iff.
Qed.

Theorem concrete_checked_import_sizes_exact : forall limit root parts,
  Chain.checked_import_sizes 5 (N.of_nat limit) concrete_score_length root parts <->
  exists capacity,
    checked_reserve limit (length (Envelope.snapshot parts)) (length (Carrier.carrier_expr root)) =
      Some capacity /\
    length (direct_score (Envelope.snapshot parts) (Envelope.connective_used parts)) <
      ChannelImage.u64_bound.
Proof.
  intros limit root parts. unfold Chain.checked_import_sizes, concrete_score_length.
  rewrite <- N.compare_le_iff, <- Nat2N.inj_compare, Nat.compare_le_iff,
    import_score_bound_exact. split.
  - intros [reserve_bound score_bound].
    exists (69 + 2 * length (Envelope.snapshot parts) + length (Carrier.carrier_expr root)).
    split; [apply checked_reserve_exact_acceptance; split; lia|exact score_bound].
  - intros [capacity [reserve score_bound]]. apply checked_reserve_exact_acceptance in reserve.
    split; [lia|exact score_bound].
Qed.

Definition concrete_full_import (inspect : bytes -> option SingletonPrefilter.inspection)
    (admit_key : bytes -> option bool) (depth_limit limit : nat)
    (input : bytes) (observed : Chain.import_observation) : Prop :=
  Chain.full_import_admitted inspect admit_key depth_limit 5 (N.of_nat limit)
    concrete_score_length input observed.

Definition observation_score (observed : Chain.import_observation) : bytes :=
  direct_score (Chain.imported_snapshot observed) (Chain.imported_score_connective observed).

Definition observation_endpoint (observed : Chain.import_observation) : bytes :=
  endpoint_key (Chain.imported_snapshot observed) (Chain.imported_score_connective observed)
    (Chain.imported_expr observed).

Theorem full_import_has_concrete_rpb3_endpoint :
  forall inspect admit_key depth_limit limit input observed,
    concrete_full_import inspect admit_key depth_limit limit input observed ->
    exists root parts layers word capacity,
      input = Carrier.carrier_bytes root /\
      Carrier.nested_parts_cursor input = Some (root, parts) /\
      Chain.recursive_reference inspect admit_key (Envelope.snapshot parts) layers (Carrier.gint_bytes word) /\
      S (length layers) <= depth_limit /\ (word < Carrier.u64_limit)%N /\
      Chain.canonical_remainder parts /\
      Forall (fun entry => Chain.canonical_remainder (Chain.layer_map entry)) layers /\
      observed = Chain.observe_import root parts (Chain.metadata_fold layers) /\
      checked_reserve limit (length (Envelope.snapshot parts)) (length (Carrier.carrier_expr root)) =
        Some capacity /\
      length (observation_endpoint observed) <= capacity /\ capacity <= limit /\
      split_endpoint (observation_endpoint observed) =
        Some (observation_score observed, Chain.imported_expr observed) /\
      observation_score observed = serialize_node
        (rpb3_pathmap_semantic_score (Chain.imported_snapshot observed)
          (Chain.imported_score_connective observed)) /\
      Chain.imported_multiplicity observed = 1.
Proof.
  intros inspect admit_key depth_limit limit input observed accepted.
  apply Chain.full_import_exact_input_and_observations in accepted.
  destruct accepted as [root [parts [layers [word
    [input_same [parsed [reference [depth_bound [word_bound
      [root_remainder [nested_remainders [sizes observed_same]]]]]]]]]]]].
  apply concrete_checked_import_sizes_exact in sizes.
  destruct sizes as [capacity [reserve score_bound]].
  pose proof (@emitted_endpoint_fits_successful_reserve limit (Envelope.snapshot parts)
    (Envelope.connective_used parts) (Carrier.carrier_expr root) capacity reserve) as [fits bounded].
  exists root, parts, layers, word, capacity.
  subst observed. cbn [observation_endpoint observation_score Chain.observe_import
    Chain.imported_snapshot Chain.imported_score_connective Chain.imported_expr Chain.imported_multiplicity].
  repeat split; try assumption; try reflexivity.
  - apply endpoint_trailer_recovers_exact_score_and_protobuf.
    now rewrite <- direct_score_has_exact_length with (connective := Envelope.connective_used parts).
  - apply direct_score_refines_fixed_shape_semantic_serialization.
Qed.

Definition bridge_control_root : Carrier.carrier :=
  Carrier.Carrier
    (Carrier.delimited_bytes 258
      (Envelope.protobuf_bytes (Envelope.ProtobufEnvelope None false None Chain.synthetic_leaf)))
    [7] true.

Definition bridge_control_parts : Envelope.envelope :=
  Envelope.Envelope Chain.synthetic_leaf false true None.

Theorem concrete_full_import_positive_control :
  concrete_full_import SingletonPrefilter.synthetic_inspector (fun _ => Some true) 10 4096
    (Carrier.carrier_bytes bridge_control_root)
    (Chain.observe_import bridge_control_root bridge_control_parts true).
Proof.
  unfold concrete_full_import, Chain.full_import_admitted.
  exists bridge_control_root, bridge_control_parts,
    (Chain.ChainResult 1 (Carrier.gint_bytes 0) true), 0%N.
  split; [vm_compute; reflexivity|].
  split; [apply Chain.ExecuteStop; vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [unfold Chain.checked_import_sizes; vm_compute; split; congruence|reflexivity].
Qed.

Print Assumptions specialized_integer_atoms_have_exact_bytes.
Print Assumptions direct_zero_escaping_matches_exact_frame.
Print Assumptions direct_score_refines_fixed_shape_semantic_serialization.
Print Assumptions direct_score_has_exact_length.
Print Assumptions endpoint_has_exact_bytes_and_length.
Print Assumptions endpoint_trailer_recovers_exact_score_and_protobuf.
Print Assumptions checked_reserve_exact_acceptance.
Print Assumptions emitted_endpoint_fits_successful_reserve.
Print Assumptions concrete_score_parser_round_trip.
Print Assumptions fixed_shape_score_is_prefix_free.
Print Assumptions equal_score_endpoint_order_preserves_nonprefix_protobuf_order.
Print Assumptions unequal_score_endpoint_order_is_score_order.
Print Assumptions false_connective_score_precedes_true_independently_of_suffix.
Print Assumptions concrete_byte_and_refusal_controls.
Print Assumptions concrete_checked_import_sizes_exact.
Print Assumptions full_import_has_concrete_rpb3_endpoint.
Print Assumptions concrete_full_import_positive_control.

End CanonicalEPathMapScore.
