From Stdlib Require Import Bool List Arith.

Module ConcreteNewQueryCorrespondence.

Record generated_new (Body Uri Injections Metadata : Type) : Type := GeneratedNew {
  bind_count : nat;
  body : Body;
  uri : Uri;
  injections : Injections;
  locally_free : Metadata
}.

Arguments bind_count {_ _ _ _} _.
Arguments body {_ _ _ _} _.
Arguments uri {_ _ _ _} _.

Definition spatial_new_accepts
    {Body Uri Injections Metadata : Type}
    (body_accepts : Body -> Body -> Prop)
    (target pattern : generated_new Body Uri Injections Metadata) : Prop :=
  bind_count target = bind_count pattern /\
  body_accepts (body target) (body pattern).

Definition projected_new_query_accepts
    {Body Uri Injections Metadata : Type}
    (body_query_accepts : Body -> Body -> bool)
    (target pattern : generated_new Body Uri Injections Metadata) : bool :=
  Nat.eqb (bind_count target) (bind_count pattern) &&
  body_query_accepts (body target) (body pattern).

Theorem generated_new_projection_preserves_spatial_success :
  forall (Body Uri Injections Metadata : Type)
    (body_accepts : Body -> Body -> Prop)
    (body_query_accepts : Body -> Body -> bool)
    (target pattern : generated_new Body Uri Injections Metadata),
    (forall actual expected,
      body_accepts actual expected -> body_query_accepts actual expected = true) ->
    spatial_new_accepts body_accepts target pattern ->
    projected_new_query_accepts body_query_accepts target pattern = true.
Proof.
  intros Body Uri Injections Metadata body_accepts body_query_accepts
    target pattern body_superset [same_count body_matches].
  unfold projected_new_query_accepts.
  rewrite same_count, Nat.eqb_refl.
  now apply body_superset.
Qed.

Definition encoded_exact_query_accepts
    {Body Uri Injections Metadata Bytes : Type}
    (encode : generated_new Body Uri Injections Metadata -> Bytes)
    (target pattern : generated_new Body Uri Injections Metadata) : Prop :=
  encode target = encode pattern.

Theorem ignored_uri_difference_rejects_encoded_exact_query :
  forall (Body Uri Injections Metadata Bytes : Type)
    (encode : generated_new Body Uri Injections Metadata -> Bytes)
    (target pattern : generated_new Body Uri Injections Metadata),
    (forall left right, encode left = encode right -> uri left = uri right) ->
    uri target <> uri pattern ->
    ~ encoded_exact_query_accepts encode target pattern.
Proof.
  intros Body Uri Injections Metadata Bytes encode target pattern
    uri_preserved different accepted.
  apply different.
  now apply uri_preserved.
Qed.

Definition ground_new (identifier : nat)
    : generated_new nat nat nat nat :=
  GeneratedNew nat nat nat nat 1 7 identifier 0 0.

Definition ground_new_image
    (value : generated_new nat nat nat nat) : nat * nat * nat * nat * nat :=
  (bind_count value, body value, uri value,
   injections nat nat nat nat value, locally_free nat nat nat nat value).

Theorem generated_ground_new_uri_mutant_is_a_false_negative :
  spatial_new_accepts eq (ground_new 1) (ground_new 2) /\
  ~ encoded_exact_query_accepts ground_new_image (ground_new 1) (ground_new 2).
Proof.
  split.
  - split; reflexivity.
  - discriminate.
Qed.

Print Assumptions generated_new_projection_preserves_spatial_success.
Print Assumptions ignored_uri_difference_rejects_encoded_exact_query.
Print Assumptions generated_ground_new_uri_mutant_is_a_false_negative.

End ConcreteNewQueryCorrespondence.
