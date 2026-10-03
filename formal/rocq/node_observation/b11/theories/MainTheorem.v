From Stdlib Require Import List NArith Sorting.Sorted Sorting.Permutation.
From NodeObservationB11 Require Import Wire Schema.
Import ListNotations.

Theorem canonical_integer_roundtrip : forall width value,
  (value < modulus width)%N -> read_big (big width value) = value.
Proof. exact big_roundtrip. Qed.

Theorem canonical_metadata_roundtrip : forall value suffix,
  valid metadata_codec value ->
  parse metadata_codec (emit metadata_codec value ++ suffix) = Some (value, suffix).
Proof. exact (roundtrip metadata_codec). Qed.

Theorem canonical_snapshot_roundtrip : forall value suffix,
  valid snapshot_codec value ->
  parse snapshot_codec (emit snapshot_codec value ++ suffix) = Some (value, suffix).
Proof. exact (roundtrip snapshot_codec). Qed.

Theorem canonical_metadata_injective : forall left right,
  valid metadata_codec left -> valid metadata_codec right ->
  emit metadata_codec left = emit metadata_codec right -> left = right.
Proof. exact (codec_injective metadata metadata_codec). Qed.

Theorem canonical_snapshot_injective : forall left right,
  valid snapshot_codec left -> valid snapshot_codec right ->
  emit snapshot_codec left = emit snapshot_codec right -> left = right.
Proof. exact (codec_injective snapshot snapshot_codec). Qed.

Theorem canonical_ordered_parents_preserved : forall left right,
  valid metadata_codec left -> valid metadata_codec right ->
  emit metadata_codec left = emit metadata_codec right ->
  metadata_parents left = metadata_parents right.
Proof.
  intros left right Hl Hr Heq.
  rewrite (canonical_metadata_injective left right Hl Hr Heq). reflexivity.
Qed.

Theorem canonical_availability_distinct : forall A (c : codec A) value,
  emit (optional c) None <> emit (optional c) (Some value).
Proof. intros A c value H. discriminate H. Qed.

Theorem canonical_collection_permutation : forall A (R : A -> A -> Prop) (c : codec A),
  (forall x y : A, {x = y} + {x <> y}) ->
  (forall x y, R x y -> R y x -> False) ->
  forall left right, StronglySorted R left -> StronglySorted R right ->
  Permutation left right -> emit (list_codec c) left = emit (list_codec c) right.
Proof.
  intros A R c decide asym left right Hl Hr Hp.
  rewrite (ordered_permutation_unique A R decide asym left right Hl Hr Hp). reflexivity.
Qed.
