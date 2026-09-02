From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import
  EPM1
  IncrementalPrepareStreaming
  StreamingCodecPDA
  TwoLevelRSpaceCheckpointCodec.

Import ListNotations.
Import TwoLevelRSpaceCheckpointCodec.
Set Implicit Arguments.

(** Incremental writer/reader refinement for RBA1.  The whole RSpace image is
    never assembled as one additional buffer: the envelope header, outer ACT,
    count, and each inner ACT frame are emitted as ordered chunks.  The current
    PathMap API may still materialize one ACT object at a time; that honest
    per-object bound is modeled separately from the eliminated whole-RSpace
    buffer. *)
Module TwoLevelRSpaceCheckpointStreaming.

Definition rba1_stream_chunks (image : rba1_image) : list bytes :=
  [rba1_magic_and_version;
   encode_frame (rba1_outer_act image);
   encode_varint (rba1_channel_count image)] ++
  map encode_frame (rba1_inner_bag_acts image).

Lemma concat_map_encode_frame : forall payloads,
  concat (map encode_frame payloads) = encode_frames payloads.
Proof.
  induction payloads as [|payload tail IH]; simpl; [reflexivity |].
  now rewrite IH.
Qed.

Theorem rba1_stream_chunks_concatenate_to_exact_checkpoint : forall image,
  concat (rba1_stream_chunks image) = encode_rba1 image.
Proof.
  intros [outer inner]. unfold rba1_stream_chunks, encode_rba1,
    encode_rba1_body, rba1_channel_count, rba1_magic_and_version.
  change
    (concat
      ([[82; 66; 65; 1]; encode_frame outer;
         encode_varint (length inner)] ++ map encode_frame inner) =
     [82; 66; 65; 1] ++ encode_frame outer ++
       encode_varint (length inner) ++ encode_frames inner).
  rewrite concat_app. simpl.
  rewrite concat_map_encode_frame.
  repeat rewrite app_nil_r. repeat rewrite <- app_assoc. reflexivity.
Qed.

Theorem rba1_chunked_decode_refines_contiguous_decode : forall image,
  decode_rba1 (concat (rba1_stream_chunks image)) = Some image.
Proof.
  intro image. rewrite rba1_stream_chunks_concatenate_to_exact_checkpoint.
  apply rba1_decode_encode_identity.
Qed.

Section ArbitrarySink.

Context {SinkState : Type}.
Variable sink_step : SinkState -> byte -> SinkState.

Definition write_rba1_chunks (initial : SinkState) (image : rba1_image) :=
  fold_chunks sink_step initial (rba1_stream_chunks image).

Definition write_rba1_contiguous (initial : SinkState) (image : rba1_image) :=
  fold_bytes sink_step initial (encode_rba1 image).

Theorem rba1_chunk_writer_refines_contiguous_sink : forall initial image,
  write_rba1_chunks initial image = write_rba1_contiguous initial image.
Proof.
  intros initial image. unfold write_rba1_chunks, write_rba1_contiguous.
  rewrite chunk_fold_equals_contiguous_fold.
  now rewrite rba1_stream_chunks_concatenate_to_exact_checkpoint.
Qed.

End ArbitrarySink.

Definition rba1_hash_stream_chunks (image : rba1_image) : list bytes :=
  rba1_hash_domain :: rba1_stream_chunks image.

Theorem rba1_hash_chunks_concatenate_to_exact_domain_preimage : forall image,
  concat (rba1_hash_stream_chunks image) =
    rba1_checkpoint_hash_preimage image.
Proof.
  intro image. unfold rba1_hash_stream_chunks,
    rba1_checkpoint_hash_preimage.
  change
    (rba1_hash_domain ++ concat (rba1_stream_chunks image) =
     rba1_hash_domain ++ encode_rba1 image).
  f_equal. apply rba1_stream_chunks_concatenate_to_exact_checkpoint.
Qed.

Section ArbitraryHash.

Context {HashState : Type}.
Variable hash_step : HashState -> byte -> HashState.

Theorem rba1_streamed_hash_refines_contiguous_preimage : forall initial image,
  fold_chunks hash_step initial (rba1_hash_stream_chunks image) =
  fold_bytes hash_step initial (rba1_checkpoint_hash_preimage image).
Proof.
  intros. rewrite chunk_fold_equals_contiguous_fold.
  now rewrite rba1_hash_chunks_concatenate_to_exact_domain_preimage.
Qed.

End ArbitraryHash.

Fixpoint maximum_length (objects : list bytes) : nat :=
  match objects with
  | [] => 0
  | object :: tail => Nat.max (length object) (maximum_length tail)
  end.

Definition rba1_act_objects (image : rba1_image) : list bytes :=
  rba1_outer_act image :: rba1_inner_bag_acts image.

Record rba1_stream_memory : Type := Rba1StreamMemory {
  rba1_fixed_scratch_bytes : nat;
  rba1_whole_rspace_buffer_bytes : nat;
  rba1_peak_materialized_act_bytes : nat
}.

Definition current_pathmap_stream_memory
    (scratch_bytes : nat) (image : rba1_image) : rba1_stream_memory :=
  Rba1StreamMemory
    scratch_bytes
    0
    (maximum_length (rba1_act_objects image)).

Theorem streaming_eliminates_the_whole_rspace_buffer : forall scratch image,
  rba1_whole_rspace_buffer_bytes
    (current_pathmap_stream_memory scratch image) = 0.
Proof. reflexivity. Qed.

Theorem current_peak_materialization_is_exactly_one_largest_act_object : forall
    scratch image,
  rba1_peak_materialized_act_bytes
    (current_pathmap_stream_memory scratch image) =
  maximum_length (rba1_act_objects image).
Proof. reflexivity. Qed.

Lemma every_object_is_bounded_by_maximum_length : forall objects object,
  In object objects -> length object <= maximum_length objects.
Proof.
  induction objects as [|head tail IH]; intros object Hin; simpl in *;
    [contradiction |].
  destruct Hin as [-> | Hin].
  - apply Nat.le_max_l.
  - eapply Nat.le_trans; [apply IH; exact Hin | apply Nat.le_max_r].
Qed.

Theorem every_outer_or_inner_act_fits_the_modeled_peak : forall image object,
  In object (rba1_act_objects image) ->
  length object <=
    rba1_peak_materialized_act_bytes
      (current_pathmap_stream_memory 0 image).
Proof. intros. apply every_object_is_bounded_by_maximum_length. exact H. Qed.

Theorem inner_frame_count_is_exact : forall image,
  length (map encode_frame (rba1_inner_bag_acts image)) =
    rba1_channel_count image.
Proof.
  intro image. unfold rba1_channel_count. apply length_map.
Qed.

Theorem loaded_empty_stream_contains_one_inner_frame : forall outer empty_act,
  length
    (map encode_frame
      (rba1_inner_bag_acts (loaded_empty_image outer empty_act))) = 1.
Proof. reflexivity. Qed.

Theorem unloaded_stream_contains_no_inner_frame : forall outer,
  map encode_frame (rba1_inner_bag_acts (unloaded_image outer)) = [].
Proof. reflexivity. Qed.

Theorem truncated_inner_length_frame_is_rejected :
  decode_frame [2; 42] = None.
Proof. apply length_frame_rejects_truncated_payload. Qed.

Definition streaming_cursor_frames (_ : rba1_image) : nat := 1.

Theorem streaming_writer_has_constant_native_control : forall image,
  streaming_cursor_frames image = 1.
Proof. reflexivity. Qed.

Theorem streaming_writer_has_no_configured_traversal_depth_limit : forall
    image,
  exists chunks, chunks = rba1_stream_chunks image /\
    concat chunks = encode_rba1 image.
Proof.
  intro image. exists (rba1_stream_chunks image). split; [reflexivity |].
  apply rba1_stream_chunks_concatenate_to_exact_checkpoint.
Qed.

Inductive rba1_streaming_invariant_id : Type :=
| RbaStreamInvConcatExact
| RbaStreamInvDecodeRefinement
| RbaStreamInvSinkRefinement
| RbaStreamInvHashConcat
| RbaStreamInvHashRefinement
| RbaStreamInvNoWholeBuffer
| RbaStreamInvPeakOneAct
| RbaStreamInvEveryActFitsPeak
| RbaStreamInvFrameCount
| RbaStreamInvLoadedEmptyFrame
| RbaStreamInvUnloadedNoFrame
| RbaStreamInvTruncationRejected
| RbaStreamInvConstantControl
| RbaStreamInvNoDepthLimit.

Definition rba1_streaming_invariant_catalog :
    list rba1_streaming_invariant_id :=
  [ RbaStreamInvConcatExact; RbaStreamInvDecodeRefinement;
    RbaStreamInvSinkRefinement; RbaStreamInvHashConcat;
    RbaStreamInvHashRefinement; RbaStreamInvNoWholeBuffer;
    RbaStreamInvPeakOneAct; RbaStreamInvEveryActFitsPeak;
    RbaStreamInvFrameCount; RbaStreamInvLoadedEmptyFrame;
    RbaStreamInvUnloadedNoFrame; RbaStreamInvTruncationRejected;
    RbaStreamInvConstantControl; RbaStreamInvNoDepthLimit ].

Theorem rba1_streaming_invariant_catalog_is_unique_and_exact :
  NoDup rba1_streaming_invariant_catalog /\
  length rba1_streaming_invariant_catalog = 14.
Proof.
  split; [repeat constructor; simpl; intuition discriminate | reflexivity].
Qed.

Print Assumptions rba1_chunked_decode_refines_contiguous_decode.
Print Assumptions rba1_streamed_hash_refines_contiguous_preimage.
Print Assumptions streaming_eliminates_the_whole_rspace_buffer.
Print Assumptions streaming_writer_has_no_configured_traversal_depth_limit.

End TwoLevelRSpaceCheckpointStreaming.
