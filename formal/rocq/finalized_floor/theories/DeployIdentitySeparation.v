From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Lists.List.
Import ListNotations.

Inductive protocol_domain := Legacy | V6.

Definition protocol_domain_eq_dec :
  forall left right : protocol_domain, {left = right} + {left <> right}.
Proof. decide equality. Defined.

Record tagged_deploy_id := {
  identity_domain : protocol_domain;
  identity_payload : nat
}.

Definition tagged_deploy_id_eq_dec :
  forall left right : tagged_deploy_id, {left = right} + {left <> right}.
Proof. decide equality; [apply Nat.eq_dec | apply protocol_domain_eq_dec]. Defined.

Definition rejected
  (tombstones : list tagged_deploy_id)
  (candidate : tagged_deploy_id) : Prop :=
  In candidate tombstones.

Definition reject
  (tombstones : list tagged_deploy_id)
  (candidate : tagged_deploy_id) : list tagged_deploy_id :=
  if in_dec tagged_deploy_id_eq_dec candidate tombstones
  then tombstones
  else candidate :: tombstones.

Theorem equal_payload_cross_domain_ids_are_distinct :
  forall payload,
    {| identity_domain := Legacy; identity_payload := payload |} <>
    {| identity_domain := V6; identity_payload := payload |}.
Proof.
  intros payload equality.
  discriminate equality.
Qed.

Theorem v6_rejection_preserves_equal_payload_legacy_identity :
  forall tombstones payload,
    ~ rejected tombstones
        {| identity_domain := Legacy; identity_payload := payload |} ->
    ~ rejected
        (reject tombstones
          {| identity_domain := V6; identity_payload := payload |})
        {| identity_domain := Legacy; identity_payload := payload |}.
Proof.
  intros tombstones payload legacy_active legacy_rejected.
  unfold rejected, reject in legacy_rejected.
  destruct (in_dec tagged_deploy_id_eq_dec
    {| identity_domain := V6; identity_payload := payload |}
    tombstones) as [already_rejected | newly_rejected].
  - exact (legacy_active legacy_rejected).
  - simpl in legacy_rejected.
    destruct legacy_rejected as [domains_equal | legacy_was_rejected].
    + discriminate domains_equal.
    + exact (legacy_active legacy_was_rejected).
Qed.

Theorem legacy_rejection_preserves_equal_payload_v6_identity :
  forall tombstones payload,
    ~ rejected tombstones
        {| identity_domain := V6; identity_payload := payload |} ->
    ~ rejected
        (reject tombstones
          {| identity_domain := Legacy; identity_payload := payload |})
        {| identity_domain := V6; identity_payload := payload |}.
Proof.
  intros tombstones payload v6_active v6_rejected.
  unfold rejected, reject in v6_rejected.
  destruct (in_dec tagged_deploy_id_eq_dec
    {| identity_domain := Legacy; identity_payload := payload |}
    tombstones) as [already_rejected | newly_rejected].
  - exact (v6_active v6_rejected).
  - simpl in v6_rejected.
    destruct v6_rejected as [domains_equal | v6_was_rejected].
    + discriminate domains_equal.
    + exact (v6_active v6_was_rejected).
Qed.
