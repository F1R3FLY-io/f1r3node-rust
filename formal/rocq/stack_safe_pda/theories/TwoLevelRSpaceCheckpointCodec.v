From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import
  EPM1
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceRepresentation.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceRepresentation.
Set Implicit Arguments.

(** RBA1 is the canonical physical checkpoint envelope for a two-level RSpace
    authority.  PathMap remains responsible for canonical ACT bytes.  This
    envelope stores those bytes directly:

      "RBA" 0x01
      frame(outer ACT)
      canonical-u64-varint(channel count)
      frame(inner Bag ACT 0) ... frame(inner Bag ACT n-1)

    The inner frames follow exact outer endpoint order.  No channel or datum
    is decoded to [Par], projected to a vector, or sorted by this codec. *)
Module TwoLevelRSpaceCheckpointCodec.

Definition rba1_magic_and_version : bytes := [82; 66; 65; 1].
Definition rba1_hash_domain : bytes := [82; 66; 65; 72; 1].

Record rba1_image : Type := Rba1Image {
  rba1_outer_act : bytes;
  rba1_inner_bag_acts : list bytes
}.

Definition rba1_channel_count (image : rba1_image) : nat :=
  length (rba1_inner_bag_acts image).

Definition encode_rba1_body (image : rba1_image) : bytes :=
  encode_frame (rba1_outer_act image) ++
  (encode_varint (rba1_channel_count image) ++
   encode_frames (rba1_inner_bag_acts image)).

Definition encode_rba1 (image : rba1_image) : bytes :=
  rba1_magic_and_version ++ encode_rba1_body image.

Definition decode_rba1_body (rest : bytes) : option (rba1_image * bytes) :=
  match decode_frame rest with
  | Some (outer_act, after_outer) =>
      match decode_varint after_outer with
      | Some (channel_count, after_count) =>
          match decode_frames channel_count after_count with
          | Some (inner_acts, suffix) =>
              Some (Rba1Image outer_act inner_acts, suffix)
          | None => None
          end
      | None => None
      end
  | None => None
  end.

Definition decode_rba1_prefix (encoded : bytes)
    : option (rba1_image * bytes) :=
  match encoded with
  | 82 :: 66 :: 65 :: 1 :: rest =>
      decode_rba1_body rest
  | _ => None
  end.

Definition decode_rba1 (encoded : bytes) : option rba1_image :=
  match decode_rba1_prefix encoded with
  | Some (image, []) => Some image
  | _ => None
  end.

Lemma rba1_body_round_trip : forall image suffix,
  decode_rba1_body (encode_rba1_body image ++ suffix) = Some (image, suffix).
Proof.
  intros [outer_act inner_acts] suffix.
  unfold encode_rba1_body, decode_rba1_body, rba1_channel_count.
  cbn [rba1_outer_act rba1_inner_bag_acts].
  repeat rewrite <- app_assoc.
  fold decode_frame.
  rewrite canonical_frame_round_trip.
  rewrite canonical_varint_round_trip.
  rewrite canonical_frames_round_trip.
  reflexivity.
Qed.

Theorem rba1_prefix_round_trip : forall image suffix,
  decode_rba1_prefix (encode_rba1 image ++ suffix) = Some (image, suffix).
Proof.
  intros image suffix. unfold encode_rba1, rba1_magic_and_version.
  rewrite <- app_assoc. simpl. apply rba1_body_round_trip.
Qed.

Theorem rba1_decode_encode_identity : forall image,
  decode_rba1 (encode_rba1 image) = Some image.
Proof.
  intro image. unfold decode_rba1.
  pose proof (rba1_prefix_round_trip image []) as Hround.
  rewrite app_nil_r in Hround. now rewrite Hround.
Qed.

Theorem rba1_encoding_is_injective : forall left right,
  encode_rba1 left = encode_rba1 right -> left = right.
Proof.
  intros left right Hequal.
  pose proof (rba1_decode_encode_identity left) as Hleft.
  pose proof (rba1_decode_encode_identity right) as Hright.
  rewrite Hequal in Hleft. rewrite Hright in Hleft. now inversion Hleft.
Qed.

Theorem rba1_rejects_trailing_bytes : forall image trailing,
  trailing <> [] -> decode_rba1 (encode_rba1 image ++ trailing) = None.
Proof.
  intros image trailing Hnonempty. unfold decode_rba1.
  rewrite rba1_prefix_round_trip.
  destruct trailing; [contradiction | reflexivity].
Qed.

Theorem rba1_rejects_wrong_magic :
  decode_rba1 [69; 80; 77; 49] = None.
Proof. reflexivity. Qed.

Theorem rba1_rejects_wrong_version :
  decode_rba1 [82; 66; 65; 2] = None.
Proof. reflexivity. Qed.

Definition rba1_direct_act_objects (image : rba1_image) : list bytes :=
  rba1_outer_act image :: rba1_inner_bag_acts image.

Theorem rba1_frames_are_exact_pathmap_act_objects : forall image,
  rba1_direct_act_objects image =
    rba1_outer_act image :: rba1_inner_bag_acts image.
Proof. reflexivity. Qed.

Theorem rba1_direct_replay_uses_no_logical_projection : forall image candidate,
  direct_object_replay (rba1_direct_act_objects image) candidate <->
  concat (rba1_outer_act image :: rba1_inner_bag_acts image) = candidate.
Proof.
  intros image candidate.
  apply direct_object_replay_iff_materialized_writer_image.
Qed.

Definition encode_inner_bags_in_outer_order
    (encode_bag_act : ChannelBag -> bytes)
    (authority : TwoLevelAuthority) : list bytes :=
  map (fun entry => encode_bag_act (snd entry)) authority.

Definition prepare_rba1_checkpoint
    (encode_outer_act : TwoLevelAuthority -> bytes)
    (encode_bag_act : ChannelBag -> bytes)
    (internal : InternalAuthority) : option rba1_image :=
  match project_public_authority internal with
  | Some public =>
      Some (Rba1Image
        (encode_outer_act public)
        (encode_inner_bags_in_outer_order encode_bag_act public))
  | None => None
  end.

Theorem checkpoint_preparation_rejects_every_barrier_bearing_authority : forall
    encode_outer_act encode_bag_act internal,
  0 < barrier_count internal ->
  prepare_rba1_checkpoint encode_outer_act encode_bag_act internal = None.
Proof.
  intros encode_outer_act encode_bag_act internal Hbarrier.
  unfold prepare_rba1_checkpoint.
  pose proof (@a_barrier_bearing_authority_is_not_serializable
    internal Hbarrier)
    as Hreject.
  unfold serializable_public_authority in Hreject. now rewrite Hreject.
Qed.

Theorem prepared_inner_frames_follow_exact_outer_endpoint_order : forall
    encode_outer_act encode_bag_act internal public image,
  project_public_authority internal = Some public ->
  prepare_rba1_checkpoint encode_outer_act encode_bag_act internal = Some image ->
  rba1_inner_bag_acts image =
    map (fun entry => encode_bag_act (snd entry)) public.
Proof.
  intros encode_outer_act encode_bag_act internal public image
    Hproject Hprepare.
  unfold prepare_rba1_checkpoint in Hprepare. rewrite Hproject in Hprepare.
  inversion Hprepare. reflexivity.
Qed.

Theorem prepared_channel_count_is_exact_outer_endpoint_count : forall
    encode_outer_act encode_bag_act internal public image,
  project_public_authority internal = Some public ->
  prepare_rba1_checkpoint encode_outer_act encode_bag_act internal = Some image ->
  rba1_channel_count image = length public.
Proof.
  intros encode_outer_act encode_bag_act internal public image
    Hproject Hprepare.
  unfold prepare_rba1_checkpoint in Hprepare. rewrite Hproject in Hprepare.
  inversion Hprepare; subst. unfold rba1_channel_count,
    encode_inner_bags_in_outer_order. cbn [rba1_inner_bag_acts].
  apply length_map.
Qed.

Definition loaded_empty_image (outer_act empty_bag_act : bytes) : rba1_image :=
  Rba1Image outer_act [empty_bag_act].

Definition unloaded_image (outer_act : bytes) : rba1_image :=
  Rba1Image outer_act [].

Theorem loaded_empty_is_distinct_from_unloaded_for_every_act_image : forall
    outer_act empty_bag_act,
  loaded_empty_image outer_act empty_bag_act <> unloaded_image outer_act.
Proof. intros. discriminate. Qed.

Theorem loaded_empty_and_unloaded_have_distinct_rba1_bytes : forall
    outer_act empty_bag_act,
  encode_rba1 (loaded_empty_image outer_act empty_bag_act) <>
  encode_rba1 (unloaded_image outer_act).
Proof.
  intros outer_act empty_bag_act Hequal.
  apply rba1_encoding_is_injective in Hequal. discriminate.
Qed.

Definition rba1_checkpoint_hash_preimage (image : rba1_image) : bytes :=
  rba1_hash_domain ++ encode_rba1 image.

Theorem rba1_hash_preimage_is_domain_separated_from_epm1 : forall image,
  firstn 4 (rba1_checkpoint_hash_preimage image) <>
  firstn 4 (epm_magic ++ encode_rba1 image).
Proof. intros. vm_compute. discriminate. Qed.

Inductive rba1_codec_invariant_id : Type :=
| RbaInvDecodeEncode
| RbaInvInjective
| RbaInvTrailingRejected
| RbaInvMagicRejected
| RbaInvVersionRejected
| RbaInvDirectObjects
| RbaInvNoLogicalProjection
| RbaInvBarrierRejected
| RbaInvInnerOuterOrder
| RbaInvChannelCount
| RbaInvLoadedEmptyDistinct
| RbaInvLoadedEmptyBytesDistinct
| RbaInvHashDomainSeparated.

Definition rba1_codec_invariant_catalog : list rba1_codec_invariant_id :=
  [ RbaInvDecodeEncode; RbaInvInjective; RbaInvTrailingRejected;
    RbaInvMagicRejected; RbaInvVersionRejected; RbaInvDirectObjects;
    RbaInvNoLogicalProjection; RbaInvBarrierRejected; RbaInvInnerOuterOrder;
    RbaInvChannelCount; RbaInvLoadedEmptyDistinct;
    RbaInvLoadedEmptyBytesDistinct; RbaInvHashDomainSeparated ].

Theorem rba1_codec_invariant_catalog_is_unique_and_exact :
  NoDup rba1_codec_invariant_catalog /\
  length rba1_codec_invariant_catalog = 13.
Proof.
  split; [repeat constructor; simpl; intuition discriminate | reflexivity].
Qed.

Print Assumptions rba1_decode_encode_identity.
Print Assumptions checkpoint_preparation_rejects_every_barrier_bearing_authority.
Print Assumptions loaded_empty_and_unloaded_have_distinct_rba1_bytes.
Print Assumptions rba1_hash_preimage_is_domain_separated_from_epm1.

End TwoLevelRSpaceCheckpointCodec.
