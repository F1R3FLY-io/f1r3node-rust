From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Fixpoint encode_chunks (chunks : list (list nat)) (used limit : nat)
  : option (list nat * nat) :=
  match chunks with
  | [] => Some ([], used)
  | chunk :: rest =>
      let next := used + length chunk in
      if next <=? limit then
        match encode_chunks rest next limit with
        | Some (bytes, final) => Some (chunk ++ bytes, final)
        | None => None
        end
      else None
  end.

Theorem successful_encoding_preserves_bytes : forall chunks used limit bytes final,
  encode_chunks chunks used limit = Some (bytes, final) ->
  bytes = concat chunks /\ final = used + length bytes.
Proof.
  induction chunks as [|chunk rest IH]; intros used limit bytes final result.
  - simpl in result. inversion result. simpl. auto.
  - simpl in result. destruct (used + length chunk <=? limit); [|discriminate].
    destruct (encode_chunks rest (used + length chunk) limit) as [[tail total]|] eqn:next;
      [|discriminate].
    inversion result; subst. specialize (IH _ _ _ _ next).
    destruct IH as [same counted]. split; [simpl; now rewrite same|].
    rewrite length_app. lia.
Qed.

Theorem successful_encoding_respects_limit : forall chunks used limit bytes final,
  used <= limit -> encode_chunks chunks used limit = Some (bytes, final) -> final <= limit.
Proof.
  induction chunks as [|chunk rest IH]; intros used limit bytes final fits result.
  - simpl in result. inversion result; subst. exact fits.
  - simpl in result. destruct (used + length chunk <=? limit) eqn:fits_next;
      [|discriminate].
    apply Nat.leb_le in fits_next.
    destruct (encode_chunks rest (used + length chunk) limit) as [[tail total]|] eqn:next;
      [|discriminate].
    inversion result; subst. eapply IH; eauto.
Qed.

Theorem rejected_chunk_emits_no_result : forall chunk rest used limit,
  limit < used + length chunk -> encode_chunks (chunk :: rest) used limit = None.
Proof.
  intros. simpl. destruct (used + length chunk <=? limit) eqn:fits; [|reflexivity].
  apply Nat.leb_le in fits. lia.
Qed.

Theorem equal_encodings_preserve_identity : forall (Hash : list nat -> nat)
  chunks used limit bytes final,
  encode_chunks chunks used limit = Some (bytes, final) -> Hash bytes = Hash (concat chunks).
Proof.
  intros. pose proof (successful_encoding_preserves_bytes _ _ _ _ _ H) as [same _].
  now rewrite same.
Qed.

Definition growth_charge (old additional : nat) : nat := old + additional.

Theorem growth_reserves_new_allocation_before_copy : forall old additional limit used,
  used + growth_charge old additional <= limit ->
  used + old <= limit /\ used + additional <= limit.
Proof. unfold growth_charge. intros. lia. Qed.

Example payload_only_budget_omits_framing :
  encode_chunks [[1; 0; 0; 0; 0; 0; 0; 0]; [42]] 0 1 = None.
Proof. reflexivity. Qed.

Inductive SourceCodecEpoch := SourceLegacy | SourceCostV1 | SourceCostV2.
Inductive SourceEventKind := SourceProduce | SourceConsume.

Definition cost_event_domain_v1 : list nat :=
  [102; 49; 114; 51; 110; 111; 100; 101; 58; 114; 115; 112; 97; 99; 101;
   45; 101; 118; 101; 110; 116; 45; 99; 111; 115; 116; 58; 118; 49].

Definition epoch_event_members (epoch : SourceCodecEpoch) (legs : list (list nat))
  : list (list nat) :=
  match epoch with
  | SourceLegacy => legs
  | SourceCostV1 | SourceCostV2 => cost_event_domain_v1 :: legs
  end.

Record BoundSourceEncoding := {
  bound_source_epoch : SourceCodecEpoch;
  bound_source_bytes : list nat
}.

Section AuthenticatedSourceEncoding.
  Context {Source : Type}.
  Variable event_legs : SourceCodecEpoch -> Source -> list (list nat).
  Variable outer_frame : list (list nat) -> list (list nat).

  Definition authenticated_source_chunks (epoch : SourceCodecEpoch) (source : Source)
    : list (list nat) := outer_frame (epoch_event_members epoch (event_legs epoch source)).

  Definition bind_source_encoding (authenticated_epoch : option SourceCodecEpoch)
    (source : Source) (used limit : nat) : option BoundSourceEncoding :=
    match authenticated_epoch with
    | None => None
    | Some epoch =>
        match encode_chunks (authenticated_source_chunks epoch source) used limit with
        | Some (bytes, _) =>
            Some {| bound_source_epoch := epoch; bound_source_bytes := bytes |}
        | None => None
        end
    end.

  Theorem missing_authenticated_epoch_has_no_binding : forall source used limit,
    bind_source_encoding None source used limit = None.
  Proof. reflexivity. Qed.

  Theorem binding_retains_authenticated_epoch_and_preimage :
    forall epoch source used limit bound,
    bind_source_encoding (Some epoch) source used limit = Some bound ->
    bound_source_epoch bound = epoch /\
    bound_source_bytes bound = concat (authenticated_source_chunks epoch source).
  Proof.
    intros epoch source used limit bound result.
    unfold bind_source_encoding in result.
    destruct (encode_chunks (authenticated_source_chunks epoch source) used limit)
      as [[bytes final]|] eqn:encoded; [|discriminate].
    inversion result; subst. simpl. split; [reflexivity|].
    exact (proj1 (successful_encoding_preserves_bytes _ _ _ _ _ encoded)).
  Qed.

  Theorem binding_preserves_any_hash_identity :
    forall (Hash : list nat -> nat) epoch source used limit bound,
    bind_source_encoding (Some epoch) source used limit = Some bound ->
    Hash (bound_source_bytes bound) = Hash (concat (authenticated_source_chunks epoch source)).
  Proof.
    intros Hash epoch source used limit bound result.
    unfold bind_source_encoding in result.
    destruct (encode_chunks (authenticated_source_chunks epoch source) used limit)
      as [[bytes final]|] eqn:encoded; [|discriminate].
    inversion result; subst. simpl.
    eapply equal_encodings_preserve_identity. exact encoded.
  Qed.

  Theorem independently_bounded_routes_preserve_source_binding :
    forall epoch source used1 limit1 used2 limit2 registered observed,
    bind_source_encoding (Some epoch) source used1 limit1 = Some registered ->
    bind_source_encoding (Some epoch) source used2 limit2 = Some observed ->
    registered = observed.
  Proof.
    intros epoch source used1 limit1 used2 limit2 registered observed Hreg Hobs.
    apply binding_retains_authenticated_epoch_and_preimage in Hreg.
    apply binding_retains_authenticated_epoch_and_preimage in Hobs.
    destruct registered as [epoch1 bytes1], observed as [epoch2 bytes2].
    simpl in *. destruct Hreg as [He1 Hb1], Hobs as [He2 Hb2]. now subst.
  Qed.
End AuthenticatedSourceEncoding.

Definition source_identity (Hash : list nat -> nat) (bound : BoundSourceEncoding) : nat :=
  Hash (bound_source_bytes bound).

Definition introduction_identity (DomainHash : SourceEventKind -> nat -> nat)
  (Hash : list nat -> nat) (kind : SourceEventKind) (bound : BoundSourceEncoding) : nat :=
  DomainHash kind (source_identity Hash bound).

Definition register_identity (identity authority : nat) (prior : nat -> option nat)
  : nat -> option nat :=
  fun key => if Nat.eqb key identity then Some authority else prior key.

Theorem registered_identity_lookup : forall identity authority prior,
  register_identity identity authority prior identity = Some authority.
Proof. intros. unfold register_identity. now rewrite Nat.eqb_refl. Qed.

Theorem authenticated_introduction_registration_preserves_lookup :
  forall Source (event_legs : SourceCodecEpoch -> Source -> list (list nat)) frame
    Hash DomainHash kind epoch source used1 limit1 used2 limit2 registered observed authority prior,
  bind_source_encoding event_legs frame (Some epoch) source used1 limit1 = Some registered ->
  bind_source_encoding event_legs frame (Some epoch) source used2 limit2 = Some observed ->
  register_identity (introduction_identity DomainHash Hash kind registered) authority prior
    (introduction_identity DomainHash Hash kind observed) = Some authority.
Proof.
  intros Source event_legs frame Hash DomainHash kind epoch source used1 limit1 used2 limit2
    registered observed authority prior Hreg Hobs.
  pose proof (independently_bounded_routes_preserve_source_binding
    event_legs frame _ _ _ _ _ _ _ _ Hreg Hobs) as same.
  subst observed. apply registered_identity_lookup.
Qed.

Record SourceStackReceipt := {
  stack_receipt_source_hash : nat;
  stack_receipt_stack : nat
}.

Definition source_stack_receipt Hash bound stack : SourceStackReceipt :=
  {| stack_receipt_source_hash := source_identity Hash bound; stack_receipt_stack := stack |}.

Theorem authenticated_stack_receipt_preserves_raw_source_lookup :
  forall Source (event_legs : SourceCodecEpoch -> Source -> list (list nat)) frame
    Hash epoch source used1 limit1 used2 limit2 registered observed stack prior,
  bind_source_encoding event_legs frame (Some epoch) source used1 limit1 = Some registered ->
  bind_source_encoding event_legs frame (Some epoch) source used2 limit2 = Some observed ->
  stack_receipt_source_hash (source_stack_receipt Hash registered stack) = source_identity Hash observed /\
  register_identity (stack_receipt_source_hash (source_stack_receipt Hash registered stack)) stack prior
    (source_identity Hash observed) = Some stack.
Proof.
  intros Source event_legs frame Hash epoch source used1 limit1 used2 limit2
    registered observed stack prior Hreg Hobs.
  pose proof (independently_bounded_routes_preserve_source_binding
    event_legs frame _ _ _ _ _ _ _ _ Hreg Hobs) as same.
  subst observed. split; [reflexivity|]. apply registered_identity_lookup.
Qed.

Theorem unequal_source_identities_do_not_recover_receipt : forall registered observed stack,
  observed <> registered -> register_identity registered stack (fun _ => None) observed = None.
Proof. intros. unfold register_identity. apply Nat.eqb_neq in H. now rewrite H. Qed.

Example cost_epochs_reuse_the_same_domain : forall legs,
  epoch_event_members SourceCostV1 legs = epoch_event_members SourceCostV2 legs.
Proof. reflexivity. Qed.

Definition mismatch_witness_bound epoch : BoundSourceEncoding :=
  {| bound_source_epoch := epoch;
     bound_source_bytes := concat (epoch_event_members epoch [[7]; [42]; [0]]) |}.

Example mismatched_epoch_preimage_can_lose_source_lookup :
  register_identity (source_identity (@length nat) (mismatch_witness_bound SourceLegacy))
    9 (fun _ => None) (source_identity (@length nat) (mismatch_witness_bound SourceCostV1)) = None.
Proof. reflexivity. Qed.

Example mismatched_epoch_preimage_can_lose_introduction_lookup :
  register_identity (introduction_identity (fun _ digest => S digest) (@length nat)
      SourceProduce (mismatch_witness_bound SourceLegacy)) 9 (fun _ => None)
    (introduction_identity (fun _ digest => S digest) (@length nat)
      SourceProduce (mismatch_witness_bound SourceCostV1)) = None.
Proof. reflexivity. Qed.

Example different_preimages_do_not_imply_different_hashes :
  source_identity (fun _ => 0) (mismatch_witness_bound SourceLegacy) =
  source_identity (fun _ => 0) (mismatch_witness_bound SourceCostV1).
Proof. reflexivity. Qed.

Print Assumptions binding_retains_authenticated_epoch_and_preimage.
Print Assumptions binding_preserves_any_hash_identity.
Print Assumptions independently_bounded_routes_preserve_source_binding.
Print Assumptions authenticated_introduction_registration_preserves_lookup.
Print Assumptions authenticated_stack_receipt_preserves_raw_source_lookup.
Print Assumptions missing_authenticated_epoch_has_no_binding.
Print Assumptions unequal_source_identities_do_not_recover_receipt.
Print Assumptions mismatched_epoch_preimage_can_lose_source_lookup.
Print Assumptions mismatched_epoch_preimage_can_lose_introduction_lookup.
Print Assumptions different_preimages_do_not_imply_different_hashes.
