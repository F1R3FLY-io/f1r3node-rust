From Stdlib Require Import Bool List Arith Lia.
From Stdlib Require Import Sorting.Permutation Sorting.Sorted.

Import ListNotations.
Set Implicit Arguments.

(**
  Formal-first refinement theory for a fresh RSpace generation checkpoint.

  The first section proves the allocation-free ordered merge independently of
  any concrete object type.  Equality of digest keys is an error, matching the
  Rust visitor; a successful merge is an exact permutation, preserves strict
  digest order, adds exactly one object and its declared canonical bytes, and
  carries the exact six L/R/D inventory fields.

  The second section models the successful affine publication path with an
  independent state record and guarded one-step relation.  Barriers are added
  one at a time; selection, recovery, semantic equality, acknowledgement, and
  authority transfer are established only by their corresponding transition.
  TLA+ separately exhausts failures, raw torn-slot crash images, restart, and
  outcome classification.  Real filesystem synchronization, checksums,
  bounded parsers, and Rust ownership remain executable refinement duties.
*)

Section OrderedCheckpointInventory.

Context {Object : Type}.
Variable object_key : Object -> nat.
Variable canonical_bytes : Object -> nat.

Fixpoint insert_unique_by_key
    (item : Object)
    (retained : list Object) : option (list Object) :=
  match retained with
  | [] => Some [item]
  | head :: tail =>
      match Nat.compare (object_key item) (object_key head) with
      | Lt => Some (item :: retained)
      | Eq => None
      | Gt =>
          match insert_unique_by_key item tail with
          | Some merged => Some (head :: merged)
          | None => None
          end
      end
  end.

Fixpoint canonical_byte_sum (objects : list Object) : nat :=
  match objects with
  | [] => 0
  | head :: tail => canonical_bytes head + canonical_byte_sum tail
  end.

Lemma canonical_byte_sum_permutation :
  forall left right,
    Permutation left right ->
    canonical_byte_sum left = canonical_byte_sum right.
Proof.
  intros left right permutation; induction permutation; simpl; lia.
Qed.

Theorem successful_merge_is_exact_permutation :
  forall item retained merged,
    insert_unique_by_key item retained = Some merged ->
    Permutation merged (item :: retained).
Proof.
  intros item retained; induction retained as [|head tail IH]; intros merged merge.
  - simpl in merge. inversion merge; subst; apply Permutation_refl.
  - simpl in merge.
    destruct (Nat.compare (object_key item) (object_key head)) eqn:comparison.
    + discriminate.
    + inversion merge; subst; apply Permutation_refl.
    + destruct (insert_unique_by_key item tail) as [tail_merged|] eqn:tail_merge;
        try discriminate.
      inversion merge; subst.
      eapply Permutation_trans.
      * apply perm_skip. exact (IH tail_merged eq_refl).
      * apply perm_swap.
Qed.

Arguments successful_merge_is_exact_permutation item retained merged _ : clear implicits.

Theorem successful_merge_adds_exactly_one_member :
  forall item retained merged,
    insert_unique_by_key item retained = Some merged ->
    length merged = S (length retained).
Proof.
  intros item retained merged merge.
  pose proof (successful_merge_is_exact_permutation
    item retained merged merge) as permutation.
  apply Permutation_length in permutation. simpl in permutation. exact permutation.
Qed.

Arguments successful_merge_adds_exactly_one_member item retained merged _ : clear implicits.

Theorem successful_merge_adds_exactly_declared_bytes :
  forall item retained merged,
    insert_unique_by_key item retained = Some merged ->
    canonical_byte_sum merged =
      canonical_bytes item + canonical_byte_sum retained.
Proof.
  intros item retained merged merge.
  change (canonical_byte_sum merged = canonical_byte_sum (item :: retained)).
  apply canonical_byte_sum_permutation.
  exact (successful_merge_is_exact_permutation item retained merged merge).
Qed.

Arguments successful_merge_adds_exactly_declared_bytes item retained merged _ : clear implicits.

Theorem successful_merge_membership_is_exact :
  forall item retained merged candidate,
    insert_unique_by_key item retained = Some merged ->
    (In candidate merged <-> candidate = item \/ In candidate retained).
Proof.
  intros item retained merged candidate merge.
  pose proof (successful_merge_is_exact_permutation
    item retained merged merge) as permutation.
  split; intro member.
  - pose proof (@Permutation_in Object merged (item :: retained)
    candidate permutation member) as exact_member.
    simpl in exact_member. destruct exact_member as [same|retained_member].
    + left. symmetry. exact same.
    + right. exact retained_member.
  - apply (@Permutation_in Object (item :: retained) merged candidate
      (Permutation_sym permutation)). simpl.
    destruct member as [same|retained_member].
    + left. symmetry. exact same.
    + right. exact retained_member.
Qed.

Definition key_lt (left right : Object) : Prop :=
  object_key left < object_key right.

Theorem successful_merge_preserves_strict_digest_order :
  forall item retained merged,
    StronglySorted key_lt retained ->
    insert_unique_by_key item retained = Some merged ->
    StronglySorted key_lt merged.
Proof.
  intros item retained; induction retained as [|head tail IH];
    intros merged sorted merge.
  - simpl in merge. inversion merge; constructor; constructor.
  - inversion sorted as [|? ? tail_sorted head_before_tail]; subst.
    simpl in merge.
    destruct (Nat.compare (object_key item) (object_key head)) eqn:comparison.
    + discriminate.
    + inversion merge; subst.
      constructor.
      * exact sorted.
      * apply Forall_forall. intros candidate member.
        simpl in member. destruct member as [candidate_is_head|candidate_in_tail].
        -- subst. apply Nat.compare_lt_iff in comparison. exact comparison.
        -- apply Forall_forall with (x := candidate) in head_before_tail;
             try exact candidate_in_tail.
           unfold key_lt in *. apply Nat.compare_lt_iff in comparison. lia.
    + destruct (insert_unique_by_key item tail) as [tail_merged|] eqn:tail_merge;
        try discriminate.
      inversion merge; subst.
      constructor.
      * exact (IH tail_merged tail_sorted eq_refl).
      * apply Forall_forall. intros candidate candidate_in_merged.
        apply successful_merge_membership_is_exact
          with (candidate := candidate) in tail_merge.
        apply tail_merge in candidate_in_merged.
        destruct candidate_in_merged as [candidate_is_item|candidate_in_tail].
        -- subst. apply Nat.compare_gt_iff in comparison. exact comparison.
        -- apply Forall_forall with (x := candidate) in head_before_tail;
             assumption.
Qed.

Theorem existing_digest_is_rejected :
  forall item retained,
    StronglySorted key_lt retained ->
    (exists candidate,
      In candidate retained /\ object_key candidate = object_key item) ->
    insert_unique_by_key item retained = None.
Proof.
  intros item retained; induction retained as [|head tail IH];
    intros sorted alias.
  - destruct alias as [candidate [member _]]. inversion member.
  - inversion sorted as [|? ? tail_sorted head_before_tail]; subst.
    destruct alias as [candidate [[candidate_is_head|candidate_in_tail] key_equal]].
    + subst candidate. simpl. rewrite <- key_equal, Nat.compare_refl. reflexivity.
    + simpl.
      destruct (Nat.compare (object_key item) (object_key head)) eqn:comparison.
      * reflexivity.
      * apply Nat.compare_lt_iff in comparison.
        apply Forall_forall with (x := candidate) in head_before_tail;
          try exact candidate_in_tail.
        unfold key_lt in head_before_tail. lia.
      * rewrite (IH tail_sorted).
        -- reflexivity.
        -- exists candidate. split; assumption.
Qed.

Record checkpoint_descriptor : Type := CheckpointDescriptor {
  current_objects : nat;
  current_canonical_bytes : nat;
  retained_objects : nat;
  retained_canonical_bytes : nat;
  data_objects : nat;
  data_canonical_bytes : nat;
  ordered_members : list Object
}.

Definition make_checkpoint_descriptor
    (current retained merged : list Object) : checkpoint_descriptor :=
  CheckpointDescriptor
    (length current)
    (canonical_byte_sum current)
    (length retained)
    (canonical_byte_sum retained)
    (length merged)
    (canonical_byte_sum merged)
    merged.

Definition descriptor_refines_merge
    (current retained : list Object)
    (retention_manifest : Object)
    (descriptor : checkpoint_descriptor) : Prop :=
  exists merged,
    insert_unique_by_key retention_manifest retained = Some merged /\
    descriptor = make_checkpoint_descriptor current retained merged.

Theorem refined_descriptor_carries_exact_six_inventories :
  forall current retained retention_manifest descriptor,
    descriptor_refines_merge current retained retention_manifest descriptor ->
    current_objects descriptor = length current /\
    current_canonical_bytes descriptor = canonical_byte_sum current /\
    retained_objects descriptor = length retained /\
    retained_canonical_bytes descriptor = canonical_byte_sum retained /\
    data_objects descriptor = S (length retained) /\
    data_canonical_bytes descriptor =
      canonical_bytes retention_manifest + canonical_byte_sum retained.
Proof.
  intros current retained retention_manifest descriptor
    [merged [merge descriptor_is_exact]].
  subst descriptor. simpl.
  rewrite (successful_merge_adds_exactly_one_member
    retention_manifest retained merged merge).
  rewrite (successful_merge_adds_exactly_declared_bytes
    retention_manifest retained merged merge).
  repeat split; reflexivity.
Qed.

Definition prepare_fixed_len : nat := 288.
Definition prepare_reference_len : nat := 40.
Definition prepare_checksum_len : nat := 8.
Definition pack_header_len : nat := 128.
Definition index_header_len : nat := 64.
Definition index_record_len : nat := 72.
Definition commit_record_len : nat := 400.

Definition checkpoint_prepare_len (descriptor : checkpoint_descriptor) : nat :=
  prepare_fixed_len +
  prepare_reference_len * data_objects descriptor +
  data_canonical_bytes descriptor +
  prepare_checksum_len.

Definition checkpoint_index_tail (descriptor : checkpoint_descriptor) : nat :=
  index_header_len + index_record_len * data_objects descriptor.

Definition checkpoint_commit_address (descriptor : checkpoint_descriptor) : nat :=
  pack_header_len + checkpoint_prepare_len descriptor.

Definition checkpoint_pack_tail (descriptor : checkpoint_descriptor) : nat :=
  checkpoint_commit_address descriptor + commit_record_len.

Theorem refined_descriptor_has_exact_wire_layout :
  forall current retained retention_manifest descriptor,
    descriptor_refines_merge current retained retention_manifest descriptor ->
    checkpoint_prepare_len descriptor =
      prepare_fixed_len +
      prepare_reference_len * S (length retained) +
      canonical_bytes retention_manifest +
      canonical_byte_sum retained +
      prepare_checksum_len /\
    checkpoint_index_tail descriptor =
      index_header_len + index_record_len * S (length retained) /\
    checkpoint_commit_address descriptor =
      pack_header_len + checkpoint_prepare_len descriptor /\
    checkpoint_pack_tail descriptor =
      checkpoint_commit_address descriptor + commit_record_len.
Proof.
  intros current retained retention_manifest descriptor refinement.
  pose proof (refined_descriptor_carries_exact_six_inventories refinement)
    as [_ [_ [_ [_ [objects bytes]]]]].
  repeat split.
  - unfold checkpoint_prepare_len. rewrite objects, bytes. lia.
  - unfold checkpoint_index_tail. rewrite objects. lia.
Qed.

End OrderedCheckpointInventory.

(**
  Receipt authority is modeled independently from its concrete byte offsets
  and cryptographic algorithm.  Each natural is an opaque authority token at
  this layer.  The executable refinement binds these facts to the typed V1
  PREPARE identity, index authority, and terminal COMMIT receipt.
*)
Section CheckpointReceiptBinding.

Inductive checkpoint_binding_fact : Type :=
| BindingSequence
| BindingStore
| BindingPreviousTerminal
| BindingPreparePack
| BindingPrepareOffset
| BindingPrepareLength
| BindingPrepareDigest
| BindingDescriptor
| BindingIndexFirstLive
| BindingIndexGeneration
| BindingIndexTail
| BindingIndexCount
| BindingIndexDigest
| BindingIndexRetention
| BindingIndexCurrentBytes
| BindingIndexRetainedBytes.

Record checkpoint_binding_state : Type := CheckpointBindingState {
  binding_sequence : nat;
  binding_store : nat;
  binding_previous_terminal : nat;
  binding_prepare_pack : nat;
  binding_prepare_offset : nat;
  binding_prepare_length : nat;
  binding_prepare_digest : nat;
  binding_descriptor : nat;
  binding_index_first_live : nat;
  binding_index_generation : nat;
  binding_index_tail : nat;
  binding_index_count : nat;
  binding_index_digest : nat;
  binding_index_retention : nat;
  binding_index_current_bytes : nat;
  binding_index_retained_bytes : nat
}.

Definition receipt_binding_is_exact
    (expected observed : checkpoint_binding_state) : Prop :=
  binding_sequence observed = binding_sequence expected /\
  binding_store observed = binding_store expected /\
  binding_previous_terminal observed = binding_previous_terminal expected /\
  binding_prepare_pack observed = binding_prepare_pack expected /\
  binding_prepare_offset observed = binding_prepare_offset expected /\
  binding_prepare_length observed = binding_prepare_length expected /\
  binding_prepare_digest observed = binding_prepare_digest expected /\
  binding_descriptor observed = binding_descriptor expected /\
  binding_index_first_live observed = binding_index_first_live expected /\
  binding_index_generation observed = binding_index_generation expected /\
  binding_index_tail observed = binding_index_tail expected /\
  binding_index_count observed = binding_index_count expected /\
  binding_index_digest observed = binding_index_digest expected /\
  binding_index_retention observed = binding_index_retention expected /\
  binding_index_current_bytes observed = binding_index_current_bytes expected /\
  binding_index_retained_bytes observed = binding_index_retained_bytes expected.

Definition mutate_checkpoint_binding
    (fact : checkpoint_binding_fact)
    (binding : checkpoint_binding_state) : checkpoint_binding_state :=
  match fact with
  | BindingSequence =>
      {| binding_sequence := S (binding_sequence binding);
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingStore =>
      {| binding_sequence := binding_sequence binding;
         binding_store := S (binding_store binding);
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingPreviousTerminal =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := S (binding_previous_terminal binding);
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingPreparePack =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := S (binding_prepare_pack binding);
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingPrepareOffset =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := S (binding_prepare_offset binding);
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingPrepareLength =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := S (binding_prepare_length binding);
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingPrepareDigest =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := S (binding_prepare_digest binding);
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingDescriptor =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := S (binding_descriptor binding);
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingIndexFirstLive =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := S (binding_index_first_live binding);
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingIndexGeneration =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := S (binding_index_generation binding);
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingIndexTail =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := S (binding_index_tail binding);
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingIndexCount =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := S (binding_index_count binding);
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingIndexDigest =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := S (binding_index_digest binding);
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingIndexRetention =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := S (binding_index_retention binding);
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingIndexCurrentBytes =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := S (binding_index_current_bytes binding);
         binding_index_retained_bytes := binding_index_retained_bytes binding |}
  | BindingIndexRetainedBytes =>
      {| binding_sequence := binding_sequence binding;
         binding_store := binding_store binding;
         binding_previous_terminal := binding_previous_terminal binding;
         binding_prepare_pack := binding_prepare_pack binding;
         binding_prepare_offset := binding_prepare_offset binding;
         binding_prepare_length := binding_prepare_length binding;
         binding_prepare_digest := binding_prepare_digest binding;
         binding_descriptor := binding_descriptor binding;
         binding_index_first_live := binding_index_first_live binding;
         binding_index_generation := binding_index_generation binding;
         binding_index_tail := binding_index_tail binding;
         binding_index_count := binding_index_count binding;
         binding_index_digest := binding_index_digest binding;
         binding_index_retention := binding_index_retention binding;
         binding_index_current_bytes := binding_index_current_bytes binding;
         binding_index_retained_bytes := S (binding_index_retained_bytes binding) |}
  end.

Theorem every_single_binding_mutation_is_rejected :
  forall expected fact,
    ~receipt_binding_is_exact expected
      (mutate_checkpoint_binding fact expected).
Proof.
  intros expected fact exact; destruct fact;
    unfold receipt_binding_is_exact in exact; simpl in exact; lia.
Qed.

End CheckpointReceiptBinding.

(** Exact, pre-mutation resource admission over a generic resource universe. *)
Section CheckpointResourcePreflight.

Inductive checkpoint_resource_family : Type :=
| DurableResource
| WalResource
| SemanticAdmissionResource.

Inductive resource_preflight_decision : Type :=
| ResourceAccepted
| ResourceRefused
| ResourceConfigurationRefused.

Record checkpoint_resource_case : Type := CheckpointResourceCase {
  resource_family : checkpoint_resource_family;
  resource_required : nat;
  resource_minimum : nat;
  resource_limit : nat
}.

Definition exact_resource_boundary (resource : checkpoint_resource_case) : nat :=
  Nat.max (resource_required resource) (resource_minimum resource).

Definition decide_resource_preflight
    (resource : checkpoint_resource_case) : resource_preflight_decision :=
  if resource_minimum resource <=? resource_limit resource then
    if resource_required resource <=? resource_limit resource
    then ResourceAccepted
    else ResourceRefused
  else ResourceConfigurationRefused.

Definition with_resource_limit
    (resource : checkpoint_resource_case)
    (limit : nat) : checkpoint_resource_case :=
  CheckpointResourceCase
    (resource_family resource)
    (resource_required resource)
    (resource_minimum resource)
    limit.

Theorem exact_resource_boundary_is_accepted :
  forall resource,
    decide_resource_preflight
      (with_resource_limit resource (exact_resource_boundary resource)) =
      ResourceAccepted.
Proof.
  intros resource.
  unfold decide_resource_preflight, with_resource_limit,
    exact_resource_boundary; simpl.
  assert (minimum_ok :
    (resource_minimum resource <=?
      Nat.max (resource_required resource) (resource_minimum resource)) = true).
  { apply Nat.leb_le. apply Nat.le_max_r. }
  assert (required_ok :
    (resource_required resource <=?
      Nat.max (resource_required resource) (resource_minimum resource)) = true).
  { apply Nat.leb_le. apply Nat.le_max_l. }
  rewrite minimum_ok, required_ok.
  reflexivity.
Qed.

Theorem required_minus_one_is_refused :
  forall resource,
    0 < exact_resource_boundary resource ->
    decide_resource_preflight
      (with_resource_limit resource (Nat.pred (exact_resource_boundary resource))) <>
      ResourceAccepted.
Proof.
  intros resource positive.
  unfold decide_resource_preflight, with_resource_limit; simpl.
  destruct (resource_minimum resource <=?
    Nat.pred (exact_resource_boundary resource)) eqn:minimum_ok;
    try discriminate.
  destruct (resource_required resource <=?
    Nat.pred (exact_resource_boundary resource)) eqn:required_ok;
    try discriminate.
  apply Nat.leb_le in minimum_ok.
  apply Nat.leb_le in required_ok.
  unfold exact_resource_boundary in positive.
  assert (boundary_le :
    Nat.max (resource_required resource) (resource_minimum resource) <=
    Nat.pred (Nat.max (resource_required resource) (resource_minimum resource))).
  { apply Nat.max_lub; assumption. }
  lia.
Qed.

Record resource_refusal_evidence : Type := ResourceRefusalEvidence {
  refusal_cleanup_started : bool;
  refusal_prepare_written : bool;
  refusal_index_written : bool;
  refusal_commit_written : bool;
  refusal_catalog_write_started : bool;
  refusal_uncertainty_started : bool;
  refusal_selected_new_authority : bool
}.

Definition refuse_resource_before_mutation : resource_refusal_evidence :=
  ResourceRefusalEvidence false false false false false false false.

Theorem refused_resource_preflight_has_no_mutation_or_new_authority :
  refusal_cleanup_started refuse_resource_before_mutation = false /\
  refusal_prepare_written refuse_resource_before_mutation = false /\
  refusal_index_written refuse_resource_before_mutation = false /\
  refusal_commit_written refuse_resource_before_mutation = false /\
  refusal_catalog_write_started refuse_resource_before_mutation = false /\
  refusal_uncertainty_started refuse_resource_before_mutation = false /\
  refusal_selected_new_authority refuse_resource_before_mutation = false.
Proof. repeat split; reflexivity. Qed.

End CheckpointResourcePreflight.

Section CrashSelection.

Inductive durable_slot_image : Type :=
| DurableOldImage
| DurableTornImage
| DurableCompleteNewImage.

Inductive recovery_selection : Type :=
| SelectOld
| SelectNew
| RejectDamagedNew.

Definition select_after_crash
    (slot : durable_slot_image)
    (restart_grade_generation : bool) : recovery_selection :=
  match slot with
  | DurableOldImage | DurableTornImage => SelectOld
  | DurableCompleteNewImage =>
      if restart_grade_generation then SelectNew else RejectDamagedNew
  end.

Theorem raw_slot_recovery_selects_old_complete_new_or_rejects :
  forall slot restart_grade_generation,
    select_after_crash slot restart_grade_generation = SelectOld \/
    (select_after_crash slot restart_grade_generation = SelectNew /\
     slot = DurableCompleteNewImage /\
     restart_grade_generation = true) \/
    (select_after_crash slot restart_grade_generation = RejectDamagedNew /\
     slot = DurableCompleteNewImage /\
     restart_grade_generation = false).
Proof.
  intros slot restart_grade_generation.
  destruct slot; simpl; try (left; reflexivity).
  destruct restart_grade_generation; simpl.
  - right; left; repeat split; reflexivity.
  - right; right; repeat split; reflexivity.
Qed.

End CrashSelection.

Section AffinePublicationProtocol.

Context {Object SemanticState : Type}.

Inductive publication_phase : Type :=
| OldAdmitted
| Preflighted
| CleanupComplete
| PrepareWritten
| PrepareDurable
| IndexWritten
| IndexDurable
| CommitWritten
| CommitDurable
| ArtifactDirectoryDurable
| CasVerified
| CatalogWriteStarted
| CatalogWritten
| CatalogFileDurable
| CatalogDirectoryDurable
| PhysicalRecovered
| SemanticReadmitted
| Acknowledged.

Inductive owner_token : Type :=
| OldWriter
| Publisher
| NewWriter.

Inductive barrier_certificate : Type :=
| PreflightCertificate
| CleanupCertificate
| PrepareWriteCertificate
| PrepareSyncCertificate
| IndexWriteCertificate
| IndexSyncCertificate
| CommitWriteCertificate
| CommitSyncCertificate
| ArtifactDirectoryCertificate
| ExactCatalogCasCertificate
| CatalogWriteStartCertificate
| CatalogWriteCompleteCertificate
| CatalogFileSyncCertificate
| CatalogDirectorySyncCertificate
| PhysicalRecoveryCertificate
| SemanticReadmissionCertificate.

Inductive authority : Type := OldAuthority | NewAuthority.

(**
  Abstracts the complete executable RuntimeBudget reconciliation snapshot.
  The fields correspond to consensus COMM cost, remaining budget, the full
  diagnostic trace digest and event count, the last out-of-phlogiston event,
  and per-lane demand.  Publication is required to preserve an arbitrary
  seeded value, not merely a zero-initialized counter.
*)
Record runtime_budget_snapshot : Type := RuntimeBudgetSnapshot {
  snapshot_total_cost : nat;
  snapshot_remaining : nat;
  snapshot_trace_digest : nat;
  snapshot_trace_event_count : nat;
  snapshot_last_oop_event : option nat;
  snapshot_per_lane_demand : list nat
}.

Record physical_diagnostic_snapshot : Type := PhysicalDiagnosticSnapshot {
  diagnostic_pack_bytes : nat;
  diagnostic_index_bytes : nat;
  diagnostic_catalog_bytes : nat;
  diagnostic_dead_bytes : nat;
  diagnostic_work_units : nat
}.

Variable initial_accounting : runtime_budget_snapshot.
Variable initial_physical_diagnostics : physical_diagnostic_snapshot.

Record publication_state : Type := PublicationState {
  state_phase : publication_phase;
  mutable_owners : list owner_token;
  certificates : list barrier_certificate;
  prepare_members : list Object;
  index_members : list Object;
  selected_authority : authority;
  physical_authority : option authority;
  semantic_before : SemanticState;
  semantic_after : option SemanticState;
  semantics_are_readmitted : bool;
  result_is_acknowledged : bool;
  accounting_before : runtime_budget_snapshot;
  accounting_after : runtime_budget_snapshot;
  physical_diagnostics : physical_diagnostic_snapshot
}.

Definition initial_publication_state (semantic : SemanticState) : publication_state :=
  PublicationState OldAdmitted [OldWriter] [] [] [] OldAuthority None
    semantic None false false initial_accounting initial_accounting
    initial_physical_diagnostics.

Definition advance
    (next_phase : publication_phase)
    (certificate : barrier_certificate)
    (state : publication_state) : publication_state :=
  PublicationState next_phase (mutable_owners state)
    (certificate :: certificates state)
    (prepare_members state) (index_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (accounting_before state) (accounting_after state)
    (physical_diagnostics state).

Definition transfer_and_advance
    (next_phase : publication_phase)
    (owner : owner_token)
    (certificate : barrier_certificate)
    (state : publication_state) : publication_state :=
  PublicationState next_phase [owner] (certificate :: certificates state)
    (prepare_members state) (index_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (accounting_before state) (accounting_after state)
    (physical_diagnostics state).

Definition write_prepare
    (expected : list Object)
    (state : publication_state) : publication_state :=
  PublicationState PrepareWritten (mutable_owners state)
    (PrepareWriteCertificate :: certificates state)
    expected (index_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (accounting_before state) (accounting_after state)
    (physical_diagnostics state).

Definition write_index
    (state : publication_state) : publication_state :=
  PublicationState IndexWritten (mutable_owners state)
    (IndexWriteCertificate :: certificates state)
    (prepare_members state) (prepare_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (accounting_before state) (accounting_after state)
    (physical_diagnostics state).

Definition select_new_catalog (state : publication_state) : publication_state :=
  PublicationState CatalogFileDurable (mutable_owners state)
    (CatalogFileSyncCertificate :: certificates state)
    (prepare_members state) (index_members state)
    NewAuthority (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (accounting_before state) (accounting_after state)
    (physical_diagnostics state).

Definition recover_new (state : publication_state) : publication_state :=
  PublicationState PhysicalRecovered (mutable_owners state)
    (PhysicalRecoveryCertificate :: certificates state)
    (prepare_members state) (index_members state)
    (selected_authority state) (Some NewAuthority)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) (result_is_acknowledged state)
    (accounting_before state) (accounting_after state)
    (physical_diagnostics state).

Definition readmit (state : publication_state) : publication_state :=
  PublicationState SemanticReadmitted (mutable_owners state)
    (SemanticReadmissionCertificate :: certificates state)
    (prepare_members state) (index_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (Some (semantic_before state))
    true (result_is_acknowledged state)
    (accounting_before state) (accounting_after state)
    (physical_diagnostics state).

Definition acknowledge (state : publication_state) : publication_state :=
  PublicationState Acknowledged [NewWriter] (certificates state)
    (prepare_members state) (index_members state)
    (selected_authority state) (physical_authority state)
    (semantic_before state) (semantic_after state)
    (semantics_are_readmitted state) true
    (accounting_before state) (accounting_after state)
    (physical_diagnostics state).

Inductive publication_step
    (expected : list Object) : publication_state -> publication_state -> Prop :=
| StepPreflight : forall before,
    state_phase before = OldAdmitted ->
    mutable_owners before = [OldWriter] ->
    publication_step expected before
      (transfer_and_advance Preflighted Publisher PreflightCertificate before)
| StepCleanup : forall before,
    state_phase before = Preflighted ->
    publication_step expected before
      (advance CleanupComplete CleanupCertificate before)
| StepWritePrepare : forall before,
    state_phase before = CleanupComplete ->
    publication_step expected before (write_prepare expected before)
| StepSyncPrepare : forall before,
    state_phase before = PrepareWritten ->
    prepare_members before = expected ->
    publication_step expected before
      (advance PrepareDurable PrepareSyncCertificate before)
| StepWriteIndex : forall before,
    state_phase before = PrepareDurable ->
    prepare_members before = expected ->
    publication_step expected before (write_index before)
| StepSyncIndex : forall before,
    state_phase before = IndexWritten ->
    index_members before = expected ->
    publication_step expected before
      (advance IndexDurable IndexSyncCertificate before)
| StepWriteCommit : forall before,
    state_phase before = IndexDurable ->
    publication_step expected before
      (advance CommitWritten CommitWriteCertificate before)
| StepSyncCommit : forall before,
    state_phase before = CommitWritten ->
    publication_step expected before
      (advance CommitDurable CommitSyncCertificate before)
| StepSyncArtifactDirectory : forall before,
    state_phase before = CommitDurable ->
    publication_step expected before
      (advance ArtifactDirectoryDurable ArtifactDirectoryCertificate before)
| StepVerifyCas : forall before,
    state_phase before = ArtifactDirectoryDurable ->
    publication_step expected before
      (advance CasVerified ExactCatalogCasCertificate before)
| StepStartCatalogWrite : forall before,
    state_phase before = CasVerified ->
    publication_step expected before
      (advance CatalogWriteStarted CatalogWriteStartCertificate before)
| StepCompleteCatalogWrite : forall before,
    state_phase before = CatalogWriteStarted ->
    publication_step expected before
      (advance CatalogWritten CatalogWriteCompleteCertificate before)
| StepSyncCatalogFile : forall before,
    state_phase before = CatalogWritten ->
    publication_step expected before (select_new_catalog before)
| StepSyncCatalogDirectory : forall before,
    state_phase before = CatalogFileDurable ->
    selected_authority before = NewAuthority ->
    publication_step expected before
      (advance CatalogDirectoryDurable CatalogDirectorySyncCertificate before)
| StepRecover : forall before,
    state_phase before = CatalogDirectoryDurable ->
    selected_authority before = NewAuthority ->
    publication_step expected before (recover_new before)
| StepReadmit : forall before,
    state_phase before = PhysicalRecovered ->
    physical_authority before = Some NewAuthority ->
    publication_step expected before (readmit before)
| StepAcknowledge : forall before,
    state_phase before = SemanticReadmitted ->
    semantic_after before = Some (semantic_before before) ->
    publication_step expected before (acknowledge before).

Inductive reachable
    (expected : list Object)
    (semantic : SemanticState) : publication_state -> Prop :=
| ReachInitial : reachable expected semantic (initial_publication_state semantic)
| ReachStep : forall before after,
    reachable expected semantic before ->
    publication_step expected before after ->
    reachable expected semantic after.

Theorem reachable_authority_is_affine_and_continuous :
  forall expected semantic state,
    reachable expected semantic state ->
    mutable_owners state = [OldWriter] \/
    mutable_owners state = [Publisher] \/
    mutable_owners state = [NewWriter].
Proof.
  intros expected semantic state path; induction path.
  - left; reflexivity.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge;
      simpl; auto.
Qed.

Definition expected_certificates_for_phase
    (phase : publication_phase) : list barrier_certificate :=
  match phase with
  | OldAdmitted => []
  | Preflighted => [PreflightCertificate]
  | CleanupComplete => [CleanupCertificate; PreflightCertificate]
  | PrepareWritten =>
      [PrepareWriteCertificate; CleanupCertificate; PreflightCertificate]
  | PrepareDurable =>
      [PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | IndexWritten =>
      [IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | IndexDurable =>
      [IndexSyncCertificate; IndexWriteCertificate; PrepareSyncCertificate;
       PrepareWriteCertificate; CleanupCertificate; PreflightCertificate]
  | CommitWritten =>
      [CommitWriteCertificate; IndexSyncCertificate; IndexWriteCertificate;
       PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CommitDurable =>
      [CommitSyncCertificate; CommitWriteCertificate; IndexSyncCertificate;
       IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | ArtifactDirectoryDurable =>
      [ArtifactDirectoryCertificate; CommitSyncCertificate;
       CommitWriteCertificate; IndexSyncCertificate; IndexWriteCertificate;
       PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CasVerified =>
      [ExactCatalogCasCertificate; ArtifactDirectoryCertificate;
       CommitSyncCertificate; CommitWriteCertificate; IndexSyncCertificate;
       IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CatalogWriteStarted =>
      [CatalogWriteStartCertificate; ExactCatalogCasCertificate;
       ArtifactDirectoryCertificate; CommitSyncCertificate;
       CommitWriteCertificate; IndexSyncCertificate; IndexWriteCertificate;
       PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CatalogWritten =>
      [CatalogWriteCompleteCertificate; CatalogWriteStartCertificate;
       ExactCatalogCasCertificate; ArtifactDirectoryCertificate;
       CommitSyncCertificate; CommitWriteCertificate; IndexSyncCertificate;
       IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CatalogFileDurable =>
      [CatalogFileSyncCertificate; CatalogWriteCompleteCertificate;
       CatalogWriteStartCertificate; ExactCatalogCasCertificate;
       ArtifactDirectoryCertificate; CommitSyncCertificate;
       CommitWriteCertificate; IndexSyncCertificate; IndexWriteCertificate;
       PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | CatalogDirectoryDurable =>
      [CatalogDirectorySyncCertificate; CatalogFileSyncCertificate;
       CatalogWriteCompleteCertificate; CatalogWriteStartCertificate;
       ExactCatalogCasCertificate; ArtifactDirectoryCertificate;
       CommitSyncCertificate; CommitWriteCertificate; IndexSyncCertificate;
       IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | PhysicalRecovered =>
      [PhysicalRecoveryCertificate; CatalogDirectorySyncCertificate;
       CatalogFileSyncCertificate; CatalogWriteCompleteCertificate;
       CatalogWriteStartCertificate; ExactCatalogCasCertificate;
       ArtifactDirectoryCertificate; CommitSyncCertificate;
       CommitWriteCertificate; IndexSyncCertificate; IndexWriteCertificate;
       PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  | SemanticReadmitted | Acknowledged =>
      [SemanticReadmissionCertificate; PhysicalRecoveryCertificate;
       CatalogDirectorySyncCertificate; CatalogFileSyncCertificate;
       CatalogWriteCompleteCertificate; CatalogWriteStartCertificate;
       ExactCatalogCasCertificate; ArtifactDirectoryCertificate;
       CommitSyncCertificate; CommitWriteCertificate; IndexSyncCertificate;
       IndexWriteCertificate; PrepareSyncCertificate; PrepareWriteCertificate;
       CleanupCertificate; PreflightCertificate]
  end.

Lemma reachable_certificates_are_exact :
  forall expected semantic state,
    reachable expected semantic state ->
    certificates state = expected_certificates_for_phase (state_phase state).
Proof.
  intros expected semantic state path; induction path.
  - reflexivity.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge;
      simpl in *.
    all: repeat match goal with
      | phase_guard : state_phase ?before = ?phase |- _ =>
          rewrite phase_guard in IHpath
      end.
    all: simpl in IHpath; rewrite IHpath; reflexivity.
Qed.

Lemma reachable_prepare_members_are_exact_after_write :
  forall expected semantic state,
    reachable expected semantic state ->
    In PrepareWriteCertificate (certificates state) ->
    prepare_members state = expected.
Proof.
  intros expected semantic state path; induction path; intro prepared.
  - inversion prepared.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *.
    all: try reflexivity.
    all: apply IHpath.
    all: firstorder congruence.
Qed.

Lemma reachable_index_members_are_exact_after_write :
  forall expected semantic state,
    reachable expected semantic state ->
    In IndexWriteCertificate (certificates state) ->
    index_members state = expected.
Proof.
  intros expected semantic state path; induction path; intro indexed.
  - inversion indexed.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *.
    all: try assumption.
    all: try reflexivity.
    all: apply IHpath.
    all: firstorder congruence.
Qed.

Lemma selected_new_has_catalog_file_certificate :
  forall expected semantic state,
    reachable expected semantic state ->
    selected_authority state = NewAuthority ->
    In CatalogFileSyncCertificate (certificates state).
Proof.
  intros expected semantic state path; induction path; intro selected.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *; try (apply IHpath; assumption); auto.
Qed.

Lemma selected_new_has_published_phase :
  forall expected semantic state,
    reachable expected semantic state ->
    selected_authority state = NewAuthority ->
    state_phase state = CatalogFileDurable \/
    state_phase state = CatalogDirectoryDurable \/
    state_phase state = PhysicalRecovered \/
    state_phase state = SemanticReadmitted \/
    state_phase state = Acknowledged.
Proof.
  intros expected semantic state path; induction path; intro selected.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *; try (apply IHpath in selected; firstorder congruence);
      firstorder congruence.
Qed.

Theorem selected_new_has_exact_members_and_all_durable_barriers :
  forall expected semantic state,
    reachable expected semantic state ->
    selected_authority state = NewAuthority ->
    prepare_members state = expected /\
    index_members state = expected /\
    In PrepareSyncCertificate (certificates state) /\
    In IndexSyncCertificate (certificates state) /\
    In CommitSyncCertificate (certificates state) /\
    In ArtifactDirectoryCertificate (certificates state) /\
    In ExactCatalogCasCertificate (certificates state) /\
    In CatalogWriteStartCertificate (certificates state) /\
    In CatalogWriteCompleteCertificate (certificates state) /\
    In CatalogFileSyncCertificate (certificates state).
Proof.
  intros expected semantic state path selected.
  pose proof (selected_new_has_catalog_file_certificate path selected)
    as catalog_file.
  pose proof (reachable_certificates_are_exact path) as exact_certificates.
  pose proof (reachable_prepare_members_are_exact_after_write path) as prepare_exact.
  pose proof (reachable_index_members_are_exact_after_write path) as index_exact.
  pose proof (selected_new_has_published_phase path selected) as published_phase.
  assert (prepare_written : In PrepareWriteCertificate (certificates state)).
  { rewrite exact_certificates.
    destruct published_phase as [phase | [phase | [phase | [phase | phase]]]];
      rewrite phase; simpl; tauto. }
  assert (index_written : In IndexWriteCertificate (certificates state)).
  { rewrite exact_certificates.
    destruct published_phase as [phase | [phase | [phase | [phase | phase]]]];
      rewrite phase; simpl; tauto. }
  specialize (prepare_exact prepare_written).
  specialize (index_exact index_written).
  split; [exact prepare_exact |].
  split; [exact index_exact |].
  destruct published_phase as [phase | [phase | [phase | [phase | phase]]]];
    rewrite exact_certificates, phase; simpl; repeat split;
    repeat first [left; reflexivity | right].
Qed.

Lemma physical_new_has_selected_new :
  forall expected semantic state,
    reachable expected semantic state ->
    physical_authority state = Some NewAuthority ->
    selected_authority state = NewAuthority.
Proof.
  intros expected semantic state path; induction path; intro recovered.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *; try (apply IHpath; assumption); auto.
Qed.

Lemma physical_new_has_recovered_phase :
  forall expected semantic state,
    reachable expected semantic state ->
    physical_authority state = Some NewAuthority ->
    state_phase state = PhysicalRecovered \/
    state_phase state = SemanticReadmitted \/
    state_phase state = Acknowledged.
Proof.
  intros expected semantic state path; induction path; intro recovered.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *; try (apply IHpath in recovered; firstorder congruence);
      firstorder congruence.
Qed.

Lemma physical_new_has_catalog_directory_certificate :
  forall expected semantic state,
    reachable expected semantic state ->
    physical_authority state = Some NewAuthority ->
    In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros expected semantic state path recovered.
  pose proof (reachable_certificates_are_exact path) as exact_certificates.
  pose proof (physical_new_has_recovered_phase path recovered) as phase.
  destruct phase as [phase | [phase | phase]];
    rewrite exact_certificates, phase; simpl;
    repeat first [left; reflexivity | right].
Qed.

Theorem physical_recovery_requires_post_catalog_directory_barrier :
  forall expected semantic state,
    reachable expected semantic state ->
    physical_authority state = Some NewAuthority ->
    selected_authority state = NewAuthority /\
    In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros expected semantic state path recovered.
  split.
  - exact (physical_new_has_selected_new path recovered).
  - exact (physical_new_has_catalog_directory_certificate path recovered).
Qed.

Lemma semantic_readmitted_phase_is_exact :
  forall expected semantic state,
    reachable expected semantic state ->
    state_phase state = SemanticReadmitted ->
    selected_authority state = NewAuthority /\
    physical_authority state = Some NewAuthority /\
    semantics_are_readmitted state = true /\
    semantic_after state = Some (semantic_before state) /\
    In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros expected semantic state path phase.
  induction path.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in phase;
      simpl in phase; try discriminate.
    match goal with
    | recovered : physical_authority ?before = Some NewAuthority |- _ =>
        pose proof
          (physical_recovery_requires_post_catalog_directory_barrier
            path recovered) as recovered_contract
    end.
    destruct recovered_contract as [selected directory].
    unfold readmit; simpl.
    split; [exact selected |].
    split; [assumption |].
    split; [reflexivity |].
    split; [reflexivity |].
    right; exact directory.
Qed.

Lemma acknowledged_has_acknowledged_phase :
  forall expected semantic state,
    reachable expected semantic state ->
    result_is_acknowledged state = true ->
    state_phase state = Acknowledged.
Proof.
  intros expected semantic state path; induction path; intro acknowledged.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in *;
      simpl in *; try (apply IHpath in acknowledged; congruence); reflexivity.
Qed.

Lemma acknowledged_phase_is_exact :
  forall expected semantic state,
    reachable expected semantic state ->
    state_phase state = Acknowledged ->
    mutable_owners state = [NewWriter] /\
    selected_authority state = NewAuthority /\
    physical_authority state = Some NewAuthority /\
    semantics_are_readmitted state = true /\
    semantic_after state = Some (semantic_before state) /\
    In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros expected semantic state path phase.
  induction path.
  - discriminate.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge in phase;
      simpl in phase; try discriminate.
    match goal with
    | readmitted : state_phase ?before = SemanticReadmitted |- _ =>
        pose proof
          (semantic_readmitted_phase_is_exact path readmitted)
          as readmitted_contract
    end.
    destruct readmitted_contract as
      [selected [recovered [readmitted [same_semantics directory]]]].
    unfold acknowledge; simpl.
    repeat split; assumption || reflexivity.
Qed.

Theorem acknowledgement_requires_recovery_readmission_and_same_semantics :
  forall expected semantic state,
    reachable expected semantic state ->
    result_is_acknowledged state = true ->
    mutable_owners state = [NewWriter] /\
    selected_authority state = NewAuthority /\
    physical_authority state = Some NewAuthority /\
    semantics_are_readmitted state = true /\
    semantic_after state = Some (semantic_before state) /\
    In CatalogDirectorySyncCertificate (certificates state).
Proof.
  intros expected semantic state path acknowledged.
  apply acknowledged_phase_is_exact with
    (expected := expected) (semantic := semantic).
  - exact path.
  - exact (acknowledged_has_acknowledged_phase path acknowledged).
Qed.

Theorem checkpoint_publication_preserves_seeded_runtime_budget :
  forall expected semantic state,
    reachable expected semantic state ->
    accounting_before state = initial_accounting /\
    accounting_after state = initial_accounting.
Proof.
  intros expected semantic state path; induction path.
  - split; reflexivity.
  - inversion H; subst; unfold transfer_and_advance, advance, write_prepare,
      write_index, select_new_catalog, recover_new, readmit, acknowledge;
      simpl; assumption.
Qed.

End AffinePublicationProtocol.

Module HotStoreJournalAcknowledgement.

Record journal := Journal {
  data_delta : list nat;
  continuation_delta : list nat;
  continuation_content : list nat;
  continuation_sources : list nat;
  join_delta : list nat
}.

Definition journal_eq_dec (left right : journal) : {left = right} + {left <> right}.
Proof. decide equality; apply list_eq_dec; apply Nat.eq_dec. Defined.

Definition empty_journal := Journal [] [] [] [] [].

Definition acknowledge_journal {Live : Type}
    (state : Live * journal) (expected : journal) : bool * (Live * journal) :=
  if journal_eq_dec (snd state) expected
  then (true, (fst state, empty_journal))
  else (false, state).

Theorem acknowledgement_preserves_live_state : forall Live
    (state : Live * journal) expected,
  fst (snd (acknowledge_journal state expected)) = fst state.
Proof.
  intros. unfold acknowledge_journal.
  destruct (journal_eq_dec (snd state) expected); reflexivity.
Qed.

Theorem exact_acknowledgement_clears_all_five_journals : forall Live
    (state : Live * journal),
  acknowledge_journal state (snd state) = (true, (fst state, empty_journal)).
Proof.
  intros. unfold acknowledge_journal.
  destruct (journal_eq_dec (snd state) (snd state)); [reflexivity | contradiction].
Qed.

Theorem stale_acknowledgement_changes_nothing : forall Live
    (state : Live * journal) expected,
  snd state <> expected -> acknowledge_journal state expected = (false, state).
Proof.
  intros Live state expected H. unfold acknowledge_journal.
  destruct (journal_eq_dec (snd state) expected); [contradiction | reflexivity].
Qed.

Theorem successful_acknowledgement_requires_exact_journal : forall Live
    (state : Live * journal) expected,
  fst (acknowledge_journal state expected) = true -> snd state = expected.
Proof.
  intros Live state expected. unfold acknowledge_journal.
  destruct (journal_eq_dec (snd state) expected); simpl; congruence.
Qed.

Theorem post_state_addition_recovers_base : forall before added,
  (before + added) - added = before.
Proof. intros; lia. Qed.

Theorem post_state_removal_recovers_base : forall before removed,
  removed <= before -> (before - removed) + removed = before.
Proof. intros; lia. Qed.

Theorem one_occurrence_removal_requires_unit_delta : forall before,
  0 < before -> (before - 1) + 1 = before.
Proof. intros; lia. Qed.

Definition invert_removal_observation (observed : option nat) (removed : nat) :=
  match observed with None => None | Some after => Some (after + removed) end.

Theorem missing_count_is_distinct_from_captured_zero : forall removed,
  invert_removal_observation None removed = None /\
  invert_removal_observation (Some 0) removed = Some removed.
Proof. intro; split; reflexivity. Qed.

Definition native_record_key (prefix record : list nat) := prefix ++ record.

Definition singleton_count (stored queried : list nat) :=
  if list_eq_dec Nat.eq_dec stored queried then 1 else 0.

Theorem journal_lookup_uses_exact_native_record_key : forall prefix record,
  singleton_count (native_record_key prefix record)
    (native_record_key prefix record) = 1.
Proof.
  intros. unfold singleton_count.
  destruct (list_eq_dec Nat.eq_dec
    (native_record_key prefix record) (native_record_key prefix record));
    [reflexivity | contradiction].
Qed.

Theorem reframed_join_key_misses_native_record : forall prefix record framed,
  length record < length framed ->
  singleton_count (native_record_key prefix record)
    (native_record_key prefix framed) = 0.
Proof.
  intros prefix record framed Hlength.
  unfold singleton_count, native_record_key.
  destruct (list_eq_dec Nat.eq_dec (prefix ++ record) (prefix ++ framed))
    as [Heq | Hneq]; [| reflexivity].
  apply app_inv_head in Heq. subst. lia.
Qed.

End HotStoreJournalAcknowledgement.

Module ExactSuccessorInventory.

Section Partition.
Context {Object : Type}.
Variable weight : Object -> nat.
Let bytes := @canonical_byte_sum Object weight.

Lemma byte_sum_app : forall left right,
  bytes (left ++ right) = bytes left + bytes right.
Proof.
  intros left right. unfold bytes. induction left; simpl; lia.
Qed.

Theorem replacement_conserves_inventory : forall old target removed added shared,
  Permutation old (removed ++ shared) ->
  Permutation target (added ++ shared) ->
  bytes old + bytes added = bytes target + bytes removed.
Proof.
  intros old target removed added shared Hold Htarget.
  assert (Ho : bytes old = bytes (removed ++ shared)).
  { apply canonical_byte_sum_permutation; exact Hold. }
  assert (Ht : bytes target = bytes (added ++ shared)).
  { apply canonical_byte_sum_permutation; exact Htarget. }
  rewrite byte_sum_app in Ho, Ht. lia.
Qed.

Theorem removed_inventory_is_bounded : forall old removed shared,
  Permutation old (removed ++ shared) -> bytes removed <= bytes old.
Proof.
  intros old removed shared Hold.
  assert (Ho : bytes old = bytes (removed ++ shared)).
  { apply canonical_byte_sum_permutation; exact Hold. }
  rewrite byte_sum_app in Ho. lia.
Qed.

Theorem exact_replacement_inventory : forall old target removed added shared,
  Permutation old (removed ++ shared) ->
  Permutation target (added ++ shared) ->
  bytes old - bytes removed + bytes added = bytes target.
Proof.
  intros old target removed added shared Hold Htarget.
  assert (Hconserve : bytes old + bytes added = bytes target + bytes removed).
  { eapply replacement_conserves_inventory; eassumption. }
  assert (Hbounded : bytes removed <= bytes old).
  { eapply removed_inventory_is_bounded; eassumption. }
  lia.
Qed.

End Partition.

Inductive example_object := OldManifest | OldRoot | Shared | TargetManifest | TargetRoot.

Definition example_weight (object : example_object) :=
  match object with
  | OldManifest | TargetManifest => 20
  | OldRoot => 10
  | Shared => 30
  | TargetRoot => 11
  end.

Theorem staged_only_inventory_counterexample :
  canonical_byte_sum example_weight [OldManifest; OldRoot; Shared] +
  canonical_byte_sum example_weight [TargetManifest; TargetRoot] -
  example_weight TargetManifest <>
  canonical_byte_sum example_weight [TargetManifest; TargetRoot; Shared].
Proof. discriminate. Qed.

End ExactSuccessorInventory.

Print Assumptions ExactSuccessorInventory.replacement_conserves_inventory.
Print Assumptions ExactSuccessorInventory.removed_inventory_is_bounded.
Print Assumptions ExactSuccessorInventory.exact_replacement_inventory.
Print Assumptions ExactSuccessorInventory.staged_only_inventory_counterexample.

Print Assumptions HotStoreJournalAcknowledgement.acknowledgement_preserves_live_state.
Print Assumptions HotStoreJournalAcknowledgement.exact_acknowledgement_clears_all_five_journals.
Print Assumptions HotStoreJournalAcknowledgement.stale_acknowledgement_changes_nothing.
Print Assumptions HotStoreJournalAcknowledgement.successful_acknowledgement_requires_exact_journal.
Print Assumptions HotStoreJournalAcknowledgement.post_state_addition_recovers_base.
Print Assumptions HotStoreJournalAcknowledgement.post_state_removal_recovers_base.
Print Assumptions HotStoreJournalAcknowledgement.one_occurrence_removal_requires_unit_delta.
Print Assumptions HotStoreJournalAcknowledgement.missing_count_is_distinct_from_captured_zero.
Print Assumptions HotStoreJournalAcknowledgement.journal_lookup_uses_exact_native_record_key.
Print Assumptions HotStoreJournalAcknowledgement.reframed_join_key_misses_native_record.

Print Assumptions successful_merge_is_exact_permutation.
Print Assumptions successful_merge_adds_exactly_one_member.
Print Assumptions successful_merge_adds_exactly_declared_bytes.
Print Assumptions successful_merge_preserves_strict_digest_order.
Print Assumptions existing_digest_is_rejected.
Print Assumptions refined_descriptor_carries_exact_six_inventories.
Print Assumptions refined_descriptor_has_exact_wire_layout.
Print Assumptions every_single_binding_mutation_is_rejected.
Print Assumptions exact_resource_boundary_is_accepted.
Print Assumptions required_minus_one_is_refused.
Print Assumptions refused_resource_preflight_has_no_mutation_or_new_authority.
Print Assumptions raw_slot_recovery_selects_old_complete_new_or_rejects.
Print Assumptions reachable_authority_is_affine_and_continuous.
Print Assumptions selected_new_has_exact_members_and_all_durable_barriers.
Print Assumptions physical_recovery_requires_post_catalog_directory_barrier.
Print Assumptions acknowledgement_requires_recovery_readmission_and_same_semantics.
Print Assumptions checkpoint_publication_preserves_seeded_runtime_budget.
