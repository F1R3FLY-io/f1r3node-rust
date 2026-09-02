From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import
  EPM1
  TwoLevelRSpaceCheckpointCodec
  TwoLevelRSpaceCheckpointStreaming
  TwoLevelRSpaceCheckpointCompatibility.

Import ListNotations.
Import TwoLevelRSpaceCheckpointCodec.
Import TwoLevelRSpaceCheckpointStreaming.
Import TwoLevelRSpaceCheckpointCompatibility.

(** One executable causal negative control for each of the forty RBA1 codec,
    streaming, and compatibility obligations. *)
Module TwoLevelRSpaceCheckpointMutants.

Definition checkpoint_fixture : rba1_image :=
  Rba1Image [1; 2; 3] [[4; 5]; []].

Inductive rba1_mutant : Type :=
| MutDecodeDropsInnerFrame
| MutEncodeCollision
| MutTrailingBytesAccepted
| MutWrongMagicAccepted
| MutWrongVersionAccepted
| MutActObjectsFlattened
| MutLogicalParProjectionInserted
| MutBarrierAccepted
| MutInnerOrderReversed
| MutChannelCountOffByOne
| MutLoadedEmptyConflated
| MutLoadedEmptyBytesConflated
| MutHashDomainReusesEpm1
| MutChunkConcatenationReordered
| MutChunkDecodeDiffers
| MutChunkSinkDiffers
| MutHashChunkConcatenationReordered
| MutStreamedHashDiffers
| MutWholeRspaceBuffered
| MutAllActsBufferedAtOnce
| MutPeakUnderstatesAct
| MutInnerFrameCountWrong
| MutLoadedEmptyFrameMissing
| MutUnloadedFrameInvented
| MutTruncatedFrameAccepted
| MutWriterControlGrows
| MutWriterDepthCapIntroduced
| MutRci2MadeAuthoritative
| MutRci2LeavesShadowMode
| MutCheckpointHashBecomesChannelIdentity
| MutLegacyHashChanged
| MutProtobufEgressChanged
| MutBincodeEgressChanged
| MutInvalidOuterPublished
| MutInvalidInnerPublished
| MutCountMismatchPublished
| MutMigrationFailureOverwritesLegacy
| MutMigrationPublishesTwice
| MutMigrationPublishesUndecodableBytes
| MutLegacyFallbackPreemptsRba1.

Definition rba1_mutant_catalog : list rba1_mutant :=
  [ MutDecodeDropsInnerFrame; MutEncodeCollision; MutTrailingBytesAccepted;
    MutWrongMagicAccepted; MutWrongVersionAccepted; MutActObjectsFlattened;
    MutLogicalParProjectionInserted; MutBarrierAccepted; MutInnerOrderReversed;
    MutChannelCountOffByOne; MutLoadedEmptyConflated;
    MutLoadedEmptyBytesConflated; MutHashDomainReusesEpm1;
    MutChunkConcatenationReordered; MutChunkDecodeDiffers;
    MutChunkSinkDiffers; MutHashChunkConcatenationReordered;
    MutStreamedHashDiffers; MutWholeRspaceBuffered; MutAllActsBufferedAtOnce;
    MutPeakUnderstatesAct; MutInnerFrameCountWrong;
    MutLoadedEmptyFrameMissing; MutUnloadedFrameInvented;
    MutTruncatedFrameAccepted; MutWriterControlGrows;
    MutWriterDepthCapIntroduced; MutRci2MadeAuthoritative;
    MutRci2LeavesShadowMode; MutCheckpointHashBecomesChannelIdentity;
    MutLegacyHashChanged; MutProtobufEgressChanged; MutBincodeEgressChanged;
    MutInvalidOuterPublished; MutInvalidInnerPublished;
    MutCountMismatchPublished; MutMigrationFailureOverwritesLegacy;
    MutMigrationPublishesTwice; MutMigrationPublishesUndecodableBytes;
    MutLegacyFallbackPreemptsRba1 ].

Inductive checkpoint_observation : Type :=
| ObserveBytes : bytes -> checkpoint_observation
| ObserveByteObjects : list bytes -> checkpoint_observation
| ObserveCount : nat -> checkpoint_observation
| ObserveCountPair : nat -> nat -> checkpoint_observation
| ObserveFlag : bool -> checkpoint_observation
| ObserveProfile : path_key_profile -> checkpoint_observation
| ObserveStatus : migration_status -> checkpoint_observation.

Definition reference_checkpoint_observation
    (mutant : rba1_mutant) : checkpoint_observation :=
  match mutant with
  | MutDecodeDropsInnerFrame =>
      ObserveByteObjects (rba1_inner_bag_acts checkpoint_fixture)
  | MutEncodeCollision => ObserveFlag false
  | MutTrailingBytesAccepted => ObserveFlag false
  | MutWrongMagicAccepted => ObserveFlag false
  | MutWrongVersionAccepted => ObserveFlag false
  | MutActObjectsFlattened =>
      ObserveByteObjects (rba1_direct_act_objects checkpoint_fixture)
  | MutLogicalParProjectionInserted => ObserveCount 0
  | MutBarrierAccepted => ObserveFlag false
  | MutInnerOrderReversed =>
      ObserveByteObjects (rba1_inner_bag_acts checkpoint_fixture)
  | MutChannelCountOffByOne => ObserveCount (rba1_channel_count checkpoint_fixture)
  | MutLoadedEmptyConflated => ObserveCountPair 1 0
  | MutLoadedEmptyBytesConflated => ObserveFlag false
  | MutHashDomainReusesEpm1 => ObserveBytes rba1_hash_domain
  | MutChunkConcatenationReordered => ObserveBytes (encode_rba1 checkpoint_fixture)
  | MutChunkDecodeDiffers => ObserveFlag true
  | MutChunkSinkDiffers => ObserveFlag true
  | MutHashChunkConcatenationReordered =>
      ObserveBytes (rba1_checkpoint_hash_preimage checkpoint_fixture)
  | MutStreamedHashDiffers => ObserveFlag true
  | MutWholeRspaceBuffered =>
      ObserveCount
        (rba1_whole_rspace_buffer_bytes
          (current_pathmap_stream_memory 64 checkpoint_fixture))
  | MutAllActsBufferedAtOnce =>
      ObserveCount
        (rba1_peak_materialized_act_bytes
          (current_pathmap_stream_memory 64 checkpoint_fixture))
  | MutPeakUnderstatesAct =>
      ObserveCount
        (maximum_length (rba1_act_objects checkpoint_fixture))
  | MutInnerFrameCountWrong => ObserveCount (rba1_channel_count checkpoint_fixture)
  | MutLoadedEmptyFrameMissing => ObserveCount 1
  | MutUnloadedFrameInvented => ObserveCount 0
  | MutTruncatedFrameAccepted => ObserveFlag false
  | MutWriterControlGrows => ObserveCount (streaming_cursor_frames checkpoint_fixture)
  | MutWriterDepthCapIntroduced => ObserveFlag false
  | MutRci2MadeAuthoritative => ObserveProfile authoritative_path_key_profile
  | MutRci2LeavesShadowMode => ObserveProfile RhoPathV1
  | MutCheckpointHashBecomesChannelIdentity => ObserveFlag false
  | MutLegacyHashChanged => ObserveBytes [7; 8]
  | MutProtobufEgressChanged => ObserveBytes [9; 10]
  | MutBincodeEgressChanged => ObserveBytes [11; 12]
  | MutInvalidOuterPublished => ObserveFlag false
  | MutInvalidInnerPublished => ObserveFlag false
  | MutCountMismatchPublished => ObserveFlag false
  | MutMigrationFailureOverwritesLegacy => ObserveBytes [13; 14]
  | MutMigrationPublishesTwice => ObserveCount 1
  | MutMigrationPublishesUndecodableBytes => ObserveFlag true
  | MutLegacyFallbackPreemptsRba1 => ObserveFlag false
  end.

Definition mutated_checkpoint_observation
    (mutant : rba1_mutant) : checkpoint_observation :=
  match mutant with
  | MutDecodeDropsInnerFrame => ObserveByteObjects [[4; 5]]
  | MutEncodeCollision => ObserveFlag true
  | MutTrailingBytesAccepted => ObserveFlag true
  | MutWrongMagicAccepted => ObserveFlag true
  | MutWrongVersionAccepted => ObserveFlag true
  | MutActObjectsFlattened => ObserveByteObjects [[1; 2; 3; 4; 5]]
  | MutLogicalParProjectionInserted => ObserveCount 2
  | MutBarrierAccepted => ObserveFlag true
  | MutInnerOrderReversed => ObserveByteObjects [[]; [4; 5]]
  | MutChannelCountOffByOne => ObserveCount 3
  | MutLoadedEmptyConflated => ObserveCountPair 0 0
  | MutLoadedEmptyBytesConflated => ObserveFlag true
  | MutHashDomainReusesEpm1 => ObserveBytes epm_magic
  | MutChunkConcatenationReordered => ObserveBytes [1; 82; 66; 65]
  | MutChunkDecodeDiffers => ObserveFlag false
  | MutChunkSinkDiffers => ObserveFlag false
  | MutHashChunkConcatenationReordered => ObserveBytes [72; 65; 66; 82]
  | MutStreamedHashDiffers => ObserveFlag false
  | MutWholeRspaceBuffered => ObserveCount 4096
  | MutAllActsBufferedAtOnce => ObserveCount 5
  | MutPeakUnderstatesAct => ObserveCount 2
  | MutInnerFrameCountWrong => ObserveCount 3
  | MutLoadedEmptyFrameMissing => ObserveCount 0
  | MutUnloadedFrameInvented => ObserveCount 1
  | MutTruncatedFrameAccepted => ObserveFlag true
  | MutWriterControlGrows => ObserveCount 4096
  | MutWriterDepthCapIntroduced => ObserveFlag true
  | MutRci2MadeAuthoritative => ObserveProfile Rci2Shadow
  | MutRci2LeavesShadowMode => ObserveProfile Rci2Shadow
  | MutCheckpointHashBecomesChannelIdentity => ObserveFlag true
  | MutLegacyHashChanged => ObserveBytes [7; 9]
  | MutProtobufEgressChanged => ObserveBytes [9; 11]
  | MutBincodeEgressChanged => ObserveBytes [11; 13]
  | MutInvalidOuterPublished => ObserveFlag true
  | MutInvalidInnerPublished => ObserveFlag true
  | MutCountMismatchPublished => ObserveFlag true
  | MutMigrationFailureOverwritesLegacy => ObserveBytes [15; 16]
  | MutMigrationPublishesTwice => ObserveCount 2
  | MutMigrationPublishesUndecodableBytes => ObserveFlag false
  | MutLegacyFallbackPreemptsRba1 => ObserveFlag true
  end.

Definition rba1_mutant_is_causally_killed (mutant : rba1_mutant) : Prop :=
  mutated_checkpoint_observation mutant <>
  reference_checkpoint_observation mutant.

Theorem every_rba1_mutant_has_a_causal_witness : forall mutant,
  rba1_mutant_is_causally_killed mutant.
Proof. intro mutant. destruct mutant; vm_compute; discriminate. Qed.

Theorem rba1_mutant_catalog_is_unique_complete_and_exact :
  NoDup rba1_mutant_catalog /\
  Forall rba1_mutant_is_causally_killed rba1_mutant_catalog /\
  length rba1_mutant_catalog = 40.
Proof.
  split.
  - unfold rba1_mutant_catalog.
    repeat match goal with
    | |- NoDup (_ :: _) => constructor; [simpl; intuition discriminate |]
    | |- NoDup [] => constructor
    end.
  - split.
    + apply Forall_forall. intros mutant _.
      apply every_rba1_mutant_has_a_causal_witness.
    + reflexivity.
Qed.

Theorem every_rba1_obligation_has_exactly_one_mutant_class :
  length rba1_mutant_catalog =
    length rba1_codec_invariant_catalog +
    length rba1_streaming_invariant_catalog +
    length rba1_compatibility_invariant_catalog.
Proof. reflexivity. Qed.

Print Assumptions every_rba1_mutant_has_a_causal_witness.
Print Assumptions rba1_mutant_catalog_is_unique_complete_and_exact.
Print Assumptions every_rba1_obligation_has_exactly_one_mutant_class.

End TwoLevelRSpaceCheckpointMutants.
