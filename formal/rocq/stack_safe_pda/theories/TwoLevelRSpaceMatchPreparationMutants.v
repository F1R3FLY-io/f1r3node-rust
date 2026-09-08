From Stdlib Require Import Bool List Arith NArith.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority
  RhoPathV1Datum
  TwoLevelRSpaceMatchPreparation.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import RhoPathV1Datum.
Import TwoLevelRSpaceMatchPreparation.
Open Scope nat_scope.

Module TwoLevelRSpaceMatchPreparationMutants.

Definition marker_a : Path := datum_channel_marker [1] [0; 2].
Definition marker_b : Path := datum_channel_marker [1] [0; 3].
Definition relative_a : Path := relative_datum_key [4; 0] [5; 0].
Definition absolute_a : Path := marker_a ++ relative_a.
Definition absolute_b : Path := marker_b ++ relative_a.
Definition malformed_path : Path := [255].

Definition fixture_delta : FlatBagDelta :=
  FlatBagDeltaState [(absolute_a, 2%N)] [(absolute_b, 3%N)].

Definition expected_groups : list PartitionedChannelDelta :=
  [PartitionedChannelDeltaState marker_a
     (FlatBagDeltaState [(relative_a, 2%N)] []);
   PartitionedChannelDeltaState marker_b
     (FlatBagDeltaState [] [(relative_a, 3%N)])].

Example exact_partition_positive_control :
  flat_delta_well_formedb fixture_delta = true /\
  partition_flat_delta fixture_delta = BuildOk expected_groups.
Proof. vm_compute. auto. Qed.

Example wrong_channel_partition_is_distinguished :
  partition_flat_delta fixture_delta <>
    BuildOk [PartitionedChannelDeltaState marker_b
      (FlatBagDeltaState [(relative_a, 2%N)] [(relative_a, 3%N)])].
Proof. vm_compute. discriminate. Qed.

Example shifted_channel_boundary_is_distinguished :
  skipn (S (length marker_a)) absolute_a <> relative_a.
Proof. vm_compute. discriminate. Qed.

Example polarity_swap_is_distinguished :
  partition_flat_delta fixture_delta <>
    BuildOk [PartitionedChannelDeltaState marker_a
      (FlatBagDeltaState [] [(relative_a, 2%N)]);
    PartitionedChannelDeltaState marker_b
      (FlatBagDeltaState [(relative_a, 3%N)] [])].
Proof. vm_compute. discriminate. Qed.

Example omitted_partition_is_distinguished :
  partition_flat_delta fixture_delta <>
    BuildOk [PartitionedChannelDeltaState marker_a
      (FlatBagDeltaState [(relative_a, 2%N)] [])].
Proof. vm_compute. discriminate. Qed.

Example opposite_supports_are_rejected_even_when_net_zero :
  flat_delta_well_formedb
    (FlatBagDeltaState [(absolute_a, 2%N)] [(absolute_a, 2%N)]) = false.
Proof. vm_compute. reflexivity. Qed.

Example exact_cancellation_distinguishes_skipping_and_doubling :
  flat_record_removal (FlatBagDeltaState [(absolute_a, 5%N)] []) absolute_a 3%N =
    Some (FlatBagDeltaState [(absolute_a, 2%N)] []) /\
  flat_record_removal (FlatBagDeltaState [(absolute_a, 5%N)] []) absolute_a 3%N <>
    Some (FlatBagDeltaState [(absolute_a, 5%N)] [(absolute_a, 3%N)]) /\
  flat_record_removal (FlatBagDeltaState [(absolute_a, 5%N)] []) absolute_a 3%N <>
    Some (FlatBagDeltaState [] [(absolute_a, 1%N)]).
Proof. vm_compute. repeat split; discriminate. Qed.

Example same_key_arrival_and_removal_cancel :
  flat_record_addition (FlatBagDeltaState [] [(absolute_a, 1%N)]) absolute_a 1%N =
    Some empty_flat_delta.
Proof. vm_compute. reflexivity. Qed.

Example overflow_record_is_refused :
  flat_record_addition (FlatBagDeltaState [(absolute_a, multiplicity_max)] [])
    absolute_a 1%N = None /\
  flat_record_removal (FlatBagDeltaState [] [(absolute_a, multiplicity_max)])
    absolute_a 1%N = None.
Proof. vm_compute. auto. Qed.

Example valid_prefix_then_malformed_partition_is_refused :
  partition_flat_delta
    (FlatBagDeltaState [(absolute_a, 1%N); (malformed_path, 1%N)] []) =
    BuildRefused (RefuseFlatDeltaPartition malformed_path).
Proof. vm_compute. reflexivity. Qed.

Example malformed_fresh_construction_is_refused : forall codec,
  construct_fresh_candidate codec malformed_path =
    BuildRefused (RefuseMalformedFreshPath malformed_path).
Proof. intros codec. reflexivity. Qed.

Example malformed_frame_and_nonbyte_construction_are_refused :
  parse_absolute_datum (marker_a ++ [0; 254]) = None /\
  absolute_datum_key_checked [1] [2] [256] [3] = None.
Proof. vm_compute. auto. Qed.

Print Assumptions exact_partition_positive_control.
Print Assumptions wrong_channel_partition_is_distinguished.
Print Assumptions shifted_channel_boundary_is_distinguished.
Print Assumptions polarity_swap_is_distinguished.
Print Assumptions omitted_partition_is_distinguished.
Print Assumptions opposite_supports_are_rejected_even_when_net_zero.
Print Assumptions exact_cancellation_distinguishes_skipping_and_doubling.
Print Assumptions same_key_arrival_and_removal_cancel.
Print Assumptions overflow_record_is_refused.
Print Assumptions valid_prefix_then_malformed_partition_is_refused.
Print Assumptions malformed_fresh_construction_is_refused.
Print Assumptions malformed_frame_and_nonbyte_construction_are_refused.

End TwoLevelRSpaceMatchPreparationMutants.
