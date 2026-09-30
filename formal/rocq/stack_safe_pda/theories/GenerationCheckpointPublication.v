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

Module JournalVersionFreshness.

Definition version := (nat * nat)%type.

Definition advance (ceiling : nat) (current : version) : version :=
  if snd current <? ceiling
  then (fst current, S (snd current))
  else (S (fst current), 0).

Definition precedes (left right : version) : Prop :=
  fst left < fst right \/ (fst left = fst right /\ snd left < snd right).

Theorem advancing_never_reuses_current_version : forall ceiling current,
  precedes current (advance ceiling current).
Proof.
  intros ceiling [identity revision]; unfold advance, precedes; simpl.
  destruct (revision <? ceiling); simpl; lia.
Qed.

Lemma precedes_transitive : forall first second third,
  precedes first second -> precedes second third -> precedes first third.
Proof.
  intros [a b] [c d] [e f]; unfold precedes; simpl; lia.
Qed.

Fixpoint after (steps ceiling : nat) (current : version) : version :=
  match steps with
  | 0 => current
  | S remaining => advance ceiling (after remaining ceiling current)
  end.

Lemma positive_history_advances : forall steps ceiling current,
  precedes current (after (S steps) ceiling current).
Proof.
  induction steps; intros ceiling current; simpl.
  - apply advancing_never_reuses_current_version.
  - eapply precedes_transitive.
    + apply IHsteps.
    + apply advancing_never_reuses_current_version.
Qed.

Theorem arbitrary_nonempty_history_cannot_return_to_old_version :
  forall steps ceiling current,
  0 < steps -> after steps ceiling current <> current.
Proof.
  intros [|steps] ceiling current positive same; [lia|].
  pose proof (positive_history_advances steps ceiling current) as later.
  rewrite same in later. destruct current as [identity revision].
  unfold precedes in later; simpl in later; lia.
Qed.

Theorem rollover_uses_a_fresh_identity : forall ceiling identity,
  advance ceiling (identity, ceiling) = (S identity, 0).
Proof.
  intros; unfold advance; simpl; rewrite Nat.ltb_irrefl; reflexivity.
Qed.

End JournalVersionFreshness.

Module JournalOwnerReadiness.

Definition binding := (nat * nat)%type.

Definition valid_binding (expected : binding) (current : option binding) : bool :=
  match current with
  | Some actual => Nat.eqb (fst expected) (fst actual) &&
                   Nat.eqb (snd expected) (snd actual)
  | None => false
  end.

Definition release_pending {Receipt : Type}
  (expected : binding) (current : option binding) (pending : Receipt) : option Receipt :=
  if valid_binding expected current then None else Some pending.

Theorem readiness_requires_exact_registry_and_catalog : forall expected current,
  valid_binding expected current = true -> current = Some expected.
Proof.
  intros [registry catalog] [[other_registry other_catalog]|] checked;
    simpl in checked; try discriminate.
  apply andb_true_iff in checked as [same_registry same_catalog].
  apply Nat.eqb_eq in same_registry. apply Nat.eqb_eq in same_catalog.
  subst; reflexivity.
Qed.

Theorem invalid_binding_retains_the_original_pending_receipt :
  forall (Receipt : Type) expected current (pending : Receipt),
  valid_binding expected current = false ->
  release_pending expected current pending = Some pending.
Proof. intros; unfold release_pending; now rewrite H. Qed.

Theorem valid_base_allows_abort_independently_of_receipt_revision :
  forall (Receipt : Type) expected (pending : Receipt),
  release_pending expected (Some expected) pending = None.
Proof.
  intros Receipt [registry catalog] pending.
  unfold release_pending, valid_binding; simpl; repeat rewrite Nat.eqb_refl; reflexivity.
Qed.

End JournalOwnerReadiness.

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

Module IncrementalReachability.

Definition counter := nat -> nat.
Definition membership := nat -> bool.

Definition replace {Value : Type} (old : nat -> Value)
  (key : nat) (value : Value) (query : nat) : Value :=
  if Nat.eqb query key then value else old query.

Fixpoint occurrences (references : list nat) (query : nat) : nat :=
  match references with
  | [] => 0
  | reference :: rest =>
      (if Nat.eqb query reference then 1 else 0) + occurrences rest query
  end.

Section Graph.
Variable children : nat -> list nat.

Fixpoint incoming (vertices : list nat) (active : membership) (query : nat) : nat :=
  match vertices with
  | [] => 0
  | parent :: rest =>
      (if active parent then occurrences (children parent) query else 0) +
      incoming rest active query
  end.

Lemma incoming_replace_absent : forall vertices active vertex enabled query,
  ~ In vertex vertices ->
  incoming vertices (replace active vertex enabled) query =
  incoming vertices active query.
Proof.
  induction vertices as [|head tail IH]; intros active vertex enabled query absent;
    [reflexivity|].
  assert (different : Nat.eqb head vertex = false).
  { apply Nat.eqb_neq. intros same. subst head. apply absent; now left. }
  cbn [incoming]. unfold replace at 1. rewrite different.
  rewrite IH; [reflexivity|]. intros member. apply absent; now right.
Qed.

Lemma incoming_replace_present : forall vertices active vertex enabled query,
  NoDup vertices -> In vertex vertices ->
  incoming vertices (replace active vertex enabled) query +
    (if active vertex then occurrences (children vertex) query else 0) =
  incoming vertices active query +
    (if enabled then occurrences (children vertex) query else 0).
Proof.
  induction vertices as [|head tail IH]; intros active vertex enabled query unique member;
    [contradiction|].
  inversion unique as [|? ? absent tail_unique]; subst.
  destruct member as [same|member].
  - subst head. cbn [incoming]. unfold replace at 1. rewrite Nat.eqb_refl.
    rewrite incoming_replace_absent by exact absent. lia.
  - assert (different : Nat.eqb head vertex = false).
    { apply Nat.eqb_neq. intros same. subst head. contradiction. }
    cbn [incoming]. unfold replace at 1. rewrite different.
    pose proof (IH active vertex enabled query tail_unique member) as conserved.
    lia.
Qed.

Record ledger := {
  counts : counter;
  active : membership
}.

Definition coherent (state : ledger) : Prop :=
  forall vertex, active state vertex = (0 <? counts state vertex).

Definition child_cursor := (nat * list nat)%type.

Fixpoint cursor_pending (cursors : list child_cursor) (query : nat) : nat :=
  match cursors with
  | [] => 0
  | (_, remaining) :: rest => occurrences remaining query + cursor_pending rest query
  end.

Definition pending (explicit : list nat) (cursors : list child_cursor) (query : nat) :=
  occurrences explicit query + cursor_pending cursors query.

Definition activation_balance (vertices : list nat) (roots : counter)
  (state : ledger) (explicit : list nat) (cursors : list child_cursor) : Prop :=
  forall vertex,
    counts state vertex + pending explicit cursors vertex =
    roots vertex + incoming vertices (active state) vertex.

Definition deactivation_balance (vertices : list nat) (roots : counter)
  (state : ledger) (explicit : list nat) (cursors : list child_cursor) : Prop :=
  forall vertex,
    counts state vertex = roots vertex + incoming vertices (active state) vertex +
      pending explicit cursors vertex.

Theorem transferring_one_cursor_occurrence_preserves_pending :
  forall explicit parent child remaining cursors query,
  pending explicit ((parent, child :: remaining) :: cursors) query =
  pending (child :: explicit) ((parent, remaining) :: cursors) query.
Proof. intros; unfold pending; cbn [cursor_pending occurrences]; lia. Qed.

Theorem discarding_an_exhausted_cursor_preserves_pending :
  forall explicit parent cursors query,
  pending explicit ((parent, []) :: cursors) query = pending explicit cursors query.
Proof. reflexivity. Qed.

Definition increment (state : ledger) (vertex : nat) : ledger :=
  {| counts := replace (counts state) vertex (S (counts state vertex));
     active := replace (active state) vertex true |}.

Definition increment_cursors (state : ledger) (vertex : nat)
  (cursors : list child_cursor) : list child_cursor :=
  if Nat.eqb (counts state vertex) 0
  then (vertex, children vertex) :: cursors else cursors.

Theorem increment_preserves_coherence : forall state vertex,
  coherent state -> coherent (increment state vertex).
Proof.
  intros state vertex agreement query.
  cbn [increment counts active]. unfold replace.
  destruct (Nat.eqb query vertex); [reflexivity|apply agreement].
Qed.

Theorem increment_preserves_activation_balance :
  forall vertices roots state vertex explicit cursors,
  NoDup vertices -> In vertex vertices -> coherent state ->
  activation_balance vertices roots state (vertex :: explicit) cursors ->
  activation_balance vertices roots (increment state vertex) explicit
    (increment_cursors state vertex cursors).
Proof.
  intros vertices roots state vertex explicit cursors unique member agreement balance query.
  pose proof (balance query) as before.
  pose proof (@incoming_replace_present vertices (active state) vertex true query
    unique member) as incoming_change.
  rewrite (agreement vertex) in incoming_change.
  unfold pending in before. cbn [occurrences] in before.
  unfold increment_cursors. destruct (counts state vertex) as [|count] eqn:old_count.
  - cbn [increment counts active pending cursor_pending] in *.
    unfold replace in *. destruct (Nat.eqb query vertex) eqn:same in *.
    + apply Nat.eqb_eq in same. subst query. try rewrite old_count in *.
      unfold pending in *. simpl in *. lia.
    + try rewrite old_count in *. unfold pending in *. simpl in *. lia.
  - cbn [increment counts active pending cursor_pending] in *.
    unfold replace in *. destruct (Nat.eqb query vertex) eqn:same in *.
    + apply Nat.eqb_eq in same. subst query. try rewrite old_count in *.
      unfold pending in *. simpl in *. lia.
    + try rewrite old_count in *. unfold pending in *. simpl in *. lia.
Qed.

Definition decrement (state : ledger) (vertex : nat) : ledger :=
  {| counts := replace (counts state) vertex (Nat.pred (counts state vertex));
     active := replace (active state) vertex (1 <? counts state vertex) |}.

Definition decrement_cursors (state : ledger) (vertex : nat)
  (cursors : list child_cursor) : list child_cursor :=
  if Nat.eqb (counts state vertex) 1
  then (vertex, children vertex) :: cursors else cursors.

Theorem pending_decrement_cannot_underflow :
  forall vertices roots state vertex explicit cursors,
  deactivation_balance vertices roots state (vertex :: explicit) cursors ->
  0 < counts state vertex.
Proof.
  intros vertices roots state vertex explicit cursors balance.
  pose proof (balance vertex) as available.
  unfold pending in available; cbn [occurrences] in available.
  rewrite Nat.eqb_refl in available. lia.
Qed.

Theorem decrement_preserves_coherence : forall state vertex,
  coherent state -> coherent (decrement state vertex).
Proof.
  intros state vertex agreement query.
  cbn [decrement counts active]. unfold replace.
  destruct (Nat.eqb query vertex); [|apply agreement].
  destruct (counts state vertex) as [|[|count]]; reflexivity.
Qed.

Theorem decrement_preserves_deactivation_balance :
  forall vertices roots state vertex explicit cursors,
  NoDup vertices -> In vertex vertices -> coherent state ->
  deactivation_balance vertices roots state (vertex :: explicit) cursors ->
  deactivation_balance vertices roots (decrement state vertex) explicit
    (decrement_cursors state vertex cursors).
Proof.
  intros vertices roots state vertex explicit cursors unique member agreement balance query.
  pose proof (@pending_decrement_cannot_underflow vertices roots state vertex explicit
    cursors balance) as available.
  pose proof (balance query) as before.
  pose proof (@incoming_replace_present vertices (active state) vertex
    (1 <? counts state vertex) query unique member) as incoming_change.
  rewrite (agreement vertex) in incoming_change.
  unfold pending in before. cbn [occurrences] in before.
  unfold decrement_cursors. destruct (counts state vertex) as [|[|count]] eqn:old_count;
    [lia| |].
  - cbn [decrement counts active pending cursor_pending] in *.
    unfold replace in *. destruct (Nat.eqb query vertex) eqn:same in *.
    + apply Nat.eqb_eq in same. subst query. try rewrite old_count in *.
      unfold pending in *. simpl in *. lia.
    + try rewrite old_count in *. unfold pending in *. simpl in *. lia.
  - cbn [decrement counts active pending cursor_pending] in *.
    unfold replace in *. destruct (Nat.eqb query vertex) eqn:same in *.
    + apply Nat.eqb_eq in same. subst query. try rewrite old_count in *.
      unfold pending in *. simpl in *. lia.
    + try rewrite old_count in *. unfold pending in *. simpl in *. lia.
Qed.

Definition quiescent (vertices : list nat) (roots : counter) (state : ledger) : Prop :=
  forall vertex, counts state vertex =
    roots vertex + incoming vertices (active state) vertex.

Definition add_roots (roots : counter) (added : list nat) : counter :=
  fun vertex => roots vertex + occurrences added vertex.

Definition remove_roots (roots : counter) (removed : list nat) : counter :=
  fun vertex => roots vertex - occurrences removed vertex.

Theorem activation_starts_with_added_root_occurrences : forall vertices roots state added,
  quiescent vertices roots state ->
  activation_balance vertices (add_roots roots added) state added [].
Proof.
  intros vertices roots state added balance query.
  unfold pending, add_roots; cbn [cursor_pending]. rewrite (balance query). lia.
Qed.

Theorem deactivation_starts_with_authorized_root_occurrences :
  forall vertices roots state removed,
  quiescent vertices roots state ->
  (forall vertex, occurrences removed vertex <= roots vertex) ->
  deactivation_balance vertices (remove_roots roots removed) state removed [].
Proof.
  intros vertices roots state removed balance authorized query.
  unfold pending, remove_roots; cbn [cursor_pending]. rewrite (balance query).
  specialize (authorized query). lia.
Qed.

Theorem drained_activation_is_quiescent : forall vertices roots state,
  activation_balance vertices roots state [] [] -> quiescent vertices roots state.
Proof.
  intros vertices roots state balance query.
  specialize (balance query). unfold pending in balance.
  cbn [occurrences cursor_pending] in balance. lia.
Qed.

Theorem drained_deactivation_is_quiescent : forall vertices roots state,
  deactivation_balance vertices roots state [] [] -> quiescent vertices roots state.
Proof.
  intros vertices roots state balance query.
  specialize (balance query). unfold pending in balance.
  cbn [occurrences cursor_pending] in balance. lia.
Qed.

Definition authenticated_cursor (cursor : child_cursor) : Prop :=
  exists consumed, children (fst cursor) = consumed ++ snd cursor.

Definition authenticated_cursors (cursors : list child_cursor) : Prop :=
  Forall authenticated_cursor cursors.

Theorem complete_child_cursor_is_authenticated : forall vertex,
  authenticated_cursor (vertex, children vertex).
Proof. intros vertex. exists []; reflexivity. Qed.

Theorem advancing_a_cursor_preserves_its_authenticated_suffix :
  forall parent child remaining,
  authenticated_cursor (parent, child :: remaining) ->
  authenticated_cursor (parent, remaining).
Proof.
  intros parent child remaining [consumed exact_suffix].
  exists (consumed ++ [child]). cbn in *. rewrite <- app_assoc. exact exact_suffix.
Qed.

Theorem increment_preserves_cursor_authentication : forall state vertex cursors,
  authenticated_cursors cursors ->
  authenticated_cursors (increment_cursors state vertex cursors).
Proof.
  intros state vertex cursors authenticated.
  unfold increment_cursors. destruct (Nat.eqb (counts state vertex) 0); auto.
  constructor; [apply complete_child_cursor_is_authenticated|exact authenticated].
Qed.

Theorem decrement_preserves_cursor_authentication : forall state vertex cursors,
  authenticated_cursors cursors ->
  authenticated_cursors (decrement_cursors state vertex cursors).
Proof.
  intros state vertex cursors authenticated.
  unfold decrement_cursors. destruct (Nat.eqb (counts state vertex) 1); auto.
  constructor; [apply complete_child_cursor_is_authenticated|exact authenticated].
Qed.

Lemma occurrences_positive : forall references query,
  0 < occurrences references query <-> In query references.
Proof.
  induction references as [|head tail IH]; intros query; cbn [occurrences In].
  - lia.
  - destruct (Nat.eqb query head) eqn:same.
    + apply Nat.eqb_eq in same. subst head. split; [auto|lia].
    + apply Nat.eqb_neq in same. rewrite <- IH. intuition lia.
Qed.

Lemma incoming_positive : forall vertices enabled query,
  0 < incoming vertices enabled query <->
  exists parent, In parent vertices /\ enabled parent = true /\
    In query (children parent).
Proof.
  induction vertices as [|head tail IH]; intros enabled query; cbn [incoming].
  - split; [lia|intros [parent [absent _]]; contradiction].
  - destruct (enabled head) eqn:head_enabled.
    + split.
      * intros positive. assert (choice : 0 < occurrences (children head) query \/
          0 < incoming tail enabled query) by lia.
        destruct choice as [edge|rest].
        -- exists head. split; [now left|]. split; [exact head_enabled|].
           now apply occurrences_positive.
        -- apply IH in rest. destruct rest as [parent [member rest]].
           exists parent. split; [now right|exact rest].
      * intros [parent [[same|member] [parent_enabled edge]]].
        -- subst parent. apply occurrences_positive in edge. lia.
        -- assert (positive : 0 < incoming tail enabled query).
           { apply IH. exists parent; auto. } lia.
    + split.
      * intros positive. apply IH in positive.
        destruct positive as [parent [member rest]].
        exists parent. split; [now right|exact rest].
      * intros [parent [[same|member] [parent_enabled edge]]].
        -- subst parent. congruence.
        -- apply IH. exists parent; auto.
Qed.

Inductive reachable (vertices : list nat) (roots : counter) : nat -> Prop :=
| reachable_root : forall vertex,
    0 < roots vertex -> reachable vertices roots vertex
| reachable_child : forall parent child,
    reachable vertices roots parent -> In parent vertices ->
    In child (children parent) -> reachable vertices roots child.

Theorem root_reachability_implies_positive_count : forall vertices roots state,
  coherent state -> quiescent vertices roots state ->
  forall vertex, reachable vertices roots vertex -> 0 < counts state vertex.
Proof.
  intros vertices roots state agreement balance vertex path.
  induction path as [vertex root|parent child path positive member edge].
  - rewrite (balance vertex). lia.
  - assert (enabled : active state parent = true).
    { rewrite (agreement parent). now apply Nat.ltb_lt. }
    assert (contribution : 0 < incoming vertices (active state) child).
    { apply incoming_positive. exists parent; auto. }
    rewrite (balance child). lia.
Qed.

Definition rank_ceiling (vertices : list nat) (rank : nat -> nat) : nat :=
  fold_right Nat.max 0 (map rank vertices).

Lemma member_rank_is_bounded : forall vertices rank vertex,
  In vertex vertices -> rank vertex <= rank_ceiling vertices rank.
Proof.
  induction vertices as [|head tail IH]; intros rank vertex member;
    [contradiction|].
  cbn [rank_ceiling map fold_right]. destruct member as [same|member].
  - subst vertex. apply Nat.le_max_l.
  - eapply Nat.le_trans; [apply IH; exact member|apply Nat.le_max_r].
Qed.

Theorem positive_count_implies_root_reachability : forall vertices roots state rank,
  coherent state -> quiescent vertices roots state ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    rank child < rank parent) ->
  forall vertex, 0 < counts state vertex -> reachable vertices roots vertex.
Proof.
  intros vertices roots state rank agreement balance descending.
  assert (by_level : forall level vertex,
    rank_ceiling vertices rank - rank vertex < level ->
    0 < counts state vertex -> reachable vertices roots vertex).
  { induction level as [|level IH]; intros vertex below positive; [lia|].
    destruct (Nat.eq_dec (roots vertex) 0) as [no_root|root].
    - assert (contribution : 0 < incoming vertices (active state) vertex).
      { rewrite (balance vertex), no_root in positive. exact positive. }
      apply incoming_positive in contribution.
      destruct contribution as [parent [member [enabled edge]]].
      assert (parent_positive : 0 < counts state parent).
      { rewrite (agreement parent) in enabled. now apply Nat.ltb_lt. }
      pose proof (@member_rank_is_bounded vertices rank parent member) as bounded.
      pose proof (descending parent vertex member edge) as lower.
      apply reachable_child with (parent := parent);
        [apply IH; [lia|exact parent_positive]|exact member|exact edge].
    - apply reachable_root. lia. }
  intros vertex positive. eapply by_level; [apply Nat.lt_succ_diag_r|exact positive].
Qed.

Theorem quiescent_support_is_exact_root_reachability : forall vertices roots state rank,
  coherent state -> quiescent vertices roots state ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    rank child < rank parent) ->
  forall vertex, active state vertex = true <-> reachable vertices roots vertex.
Proof.
  intros vertices roots state rank agreement balance descending vertex.
  rewrite (agreement vertex), Nat.ltb_lt. split.
  - eapply positive_count_implies_root_reachability; eauto.
  - eapply root_reachability_implies_positive_count; eauto.
Qed.

Lemma incoming_extensional : forall vertices left right query,
  (forall vertex, left vertex = right vertex) ->
  incoming vertices left query = incoming vertices right query.
Proof.
  induction vertices as [|head tail IH]; intros left right query agreement;
    [reflexivity|].
  cbn [incoming]. rewrite (agreement head), (IH left right query agreement). reflexivity.
Qed.

Theorem quiescent_ledger_is_unique : forall vertices roots left right rank,
  coherent left -> quiescent vertices roots left ->
  coherent right -> quiescent vertices roots right ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    rank child < rank parent) ->
  forall vertex, counts left vertex = counts right vertex /\
    active left vertex = active right vertex.
Proof.
  intros vertices roots left right rank left_coherent left_balance
    right_coherent right_balance descending.
  assert (same_support : forall vertex, active left vertex = active right vertex).
  { intros vertex.
    pose proof (@quiescent_support_is_exact_root_reachability vertices roots left rank
      left_coherent left_balance descending vertex) as left_support.
    pose proof (@quiescent_support_is_exact_root_reachability vertices roots right rank
      right_coherent right_balance descending vertex) as right_support.
    destruct (active left vertex), (active right vertex); intuition congruence. }
  intros vertex. split; [|apply same_support].
  rewrite (left_balance vertex), (right_balance vertex).
  f_equal. apply incoming_extensional. exact same_support.
Qed.

Inductive phase := Activation | Deactivation.

Record configuration := Configuration {
  resident : ledger;
  events : list nat;
  frames : list child_cursor
}.

Definition phase_balance (mode : phase) (vertices : list nat) (roots : counter)
  (machine : configuration) : Prop :=
  match mode with
  | Activation => activation_balance vertices roots (resident machine)
      (events machine) (frames machine)
  | Deactivation => deactivation_balance vertices roots (resident machine)
      (events machine) (frames machine)
  end.

Definition valid_configuration (mode : phase) (vertices : list nat) (roots : counter)
  (machine : configuration) : Prop :=
  coherent (resident machine) /\ phase_balance mode vertices roots machine /\
  Forall (fun vertex => In vertex vertices) (events machine) /\
  Forall (fun cursor => In (fst cursor) vertices /\ authenticated_cursor cursor)
    (frames machine).

Inductive cursor_step : phase -> configuration -> configuration -> Prop :=
| activate_event : forall state vertex explicit cursors,
    cursor_step Activation (Configuration state (vertex :: explicit) cursors)
      (Configuration (increment state vertex) explicit
        (increment_cursors state vertex cursors))
| deactivate_event : forall state vertex explicit cursors,
    cursor_step Deactivation (Configuration state (vertex :: explicit) cursors)
      (Configuration (decrement state vertex) explicit
        (decrement_cursors state vertex cursors))
| transfer_cursor : forall mode state explicit parent child remaining cursors,
    cursor_step mode
      (Configuration state explicit ((parent, child :: remaining) :: cursors))
      (Configuration state (child :: explicit) ((parent, remaining) :: cursors))
| discard_cursor : forall mode state explicit parent cursors,
    cursor_step mode (Configuration state explicit ((parent, []) :: cursors))
      (Configuration state explicit cursors).

Lemma authenticated_cursor_head_is_an_edge : forall parent child remaining,
  authenticated_cursor (parent, child :: remaining) -> In child (children parent).
Proof.
  intros parent child remaining [consumed suffix]. cbn in suffix.
  rewrite suffix. apply in_or_app. right. now left.
Qed.

Theorem cursor_step_preserves_configuration : forall vertices roots mode before after,
  NoDup vertices ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    In child vertices) ->
  cursor_step mode before after -> valid_configuration mode vertices roots before ->
  valid_configuration mode vertices roots after.
Proof.
  intros vertices roots mode before after unique closed transition valid.
  destruct transition as [state vertex explicit cursors|state vertex explicit cursors|
    mode state explicit parent child remaining cursors|mode state explicit parent cursors];
    destruct valid as [agreement [balance [event_domain cursor_domain]]];
    cbn [resident events frames] in *.
  - inversion event_domain as [|? ? member remaining_domain]; subst.
    split; [now apply increment_preserves_coherence|]. split.
    + eapply increment_preserves_activation_balance; eauto.
    + split; [exact remaining_domain|].
      cbn [frames]. unfold increment_cursors.
      destruct (Nat.eqb (counts state vertex) 0); [|exact cursor_domain].
      constructor; [split; [exact member|apply complete_child_cursor_is_authenticated]|
        exact cursor_domain].
  - inversion event_domain as [|? ? member remaining_domain]; subst.
    split; [now apply decrement_preserves_coherence|]. split.
    + eapply decrement_preserves_deactivation_balance; eauto.
    + split; [exact remaining_domain|].
      cbn [frames]. unfold decrement_cursors.
      destruct (Nat.eqb (counts state vertex) 1); [|exact cursor_domain].
      constructor; [split; [exact member|apply complete_child_cursor_is_authenticated]|
        exact cursor_domain].
  - inversion cursor_domain as [|? ? [parent_member authenticated] remaining_domain]; subst.
    split; [exact agreement|]. split.
    + destruct mode; intros query; specialize (balance query);
        cbn [phase_balance resident events frames] in *;
        rewrite <- transferring_one_cursor_occurrence_preserves_pending; exact balance.
    + split.
      * constructor; [|exact event_domain].
        eapply closed; [exact parent_member|].
        eapply authenticated_cursor_head_is_an_edge; exact authenticated.
      * constructor; [|exact remaining_domain]. split; [exact parent_member|].
        now apply advancing_a_cursor_preserves_its_authenticated_suffix with (child := child).
  - inversion cursor_domain as [|? ? head remaining_domain]; subst.
    split; [exact agreement|]. split.
    + destruct mode; intros query; exact (balance query).
    + split; [exact event_domain|exact remaining_domain].
Qed.

Inductive cursor_execution (mode : phase) (initial : configuration) :
  configuration -> Prop :=
| execution_start : cursor_execution mode initial initial
| execution_step : forall before after,
    cursor_execution mode initial before -> cursor_step mode before after ->
    cursor_execution mode initial after.

Theorem cursor_execution_preserves_configuration :
  forall vertices roots mode initial final,
  NoDup vertices ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    In child vertices) ->
  valid_configuration mode vertices roots initial ->
  cursor_execution mode initial final -> valid_configuration mode vertices roots final.
Proof.
  intros vertices roots mode initial final unique closed valid execution.
  induction execution; [exact valid|].
  eapply cursor_step_preserves_configuration; eauto.
Qed.

Theorem drained_execution_has_exact_root_reachability :
  forall vertices roots mode initial final rank,
  NoDup vertices ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    In child vertices) ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    rank child < rank parent) ->
  valid_configuration mode vertices roots initial ->
  cursor_execution mode initial (Configuration final [] []) ->
  coherent final /\ quiescent vertices roots final /\
    (forall vertex, active final vertex = true <-> reachable vertices roots vertex).
Proof.
  intros vertices roots mode initial final rank unique closed descending valid execution.
  pose proof (@cursor_execution_preserves_configuration vertices roots mode initial
    (Configuration final [] []) unique closed valid execution) as preserved.
  destruct preserved as [agreement [balance _]].
  assert (terminal : quiescent vertices roots final).
  { destruct mode.
    - now apply drained_activation_is_quiescent.
    - now apply drained_deactivation_is_quiescent. }
  split; [exact agreement|]. split; [exact terminal|].
  eapply quiescent_support_is_exact_root_reachability; eauto.
Qed.

Theorem activation_initial_configuration_is_valid : forall vertices roots state added,
  coherent state -> quiescent vertices roots state ->
  Forall (fun vertex => In vertex vertices) added ->
  valid_configuration Activation vertices (add_roots roots added)
    (Configuration state added []).
Proof.
  intros vertices roots state added agreement balance domain.
  split; [exact agreement|]. split.
  - now apply activation_starts_with_added_root_occurrences.
  - split; [exact domain|constructor].
Qed.

Theorem deactivation_initial_configuration_is_valid : forall vertices roots state removed,
  coherent state -> quiescent vertices roots state ->
  Forall (fun vertex => In vertex vertices) removed ->
  (forall vertex, occurrences removed vertex <= roots vertex) ->
  valid_configuration Deactivation vertices (remove_roots roots removed)
    (Configuration state removed []).
Proof.
  intros vertices roots state removed agreement balance domain authorized.
  split; [exact agreement|]. split.
  - now apply deactivation_starts_with_authorized_root_occurrences.
  - split; [exact domain|constructor].
Qed.

Theorem add_before_remove_agrees_with_cold_ledger :
  forall vertices roots initial added removed middle final cold rank,
  NoDup vertices ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    In child vertices) ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    rank child < rank parent) ->
  coherent initial -> quiescent vertices roots initial ->
  Forall (fun vertex => In vertex vertices) added ->
  Forall (fun vertex => In vertex vertices) removed ->
  (forall vertex, occurrences removed vertex <= add_roots roots added vertex) ->
  cursor_execution Activation (Configuration initial added [])
    (Configuration middle [] []) ->
  cursor_execution Deactivation (Configuration middle removed [])
    (Configuration final [] []) ->
  coherent cold -> quiescent vertices (remove_roots (add_roots roots added) removed) cold ->
  forall vertex, counts final vertex = counts cold vertex /\
    active final vertex = active cold vertex.
Proof.
  intros vertices roots initial added removed middle final cold rank unique closed
    descending initial_coherent initial_balance added_domain removed_domain authorized
    activated deactivated cold_coherent cold_balance.
  pose proof (@activation_initial_configuration_is_valid vertices roots initial added
    initial_coherent initial_balance added_domain) as activation_valid.
  pose proof (@drained_execution_has_exact_root_reachability vertices
    (add_roots roots added) Activation (Configuration initial added []) middle rank
    unique closed descending activation_valid activated) as middle_valid.
  destruct middle_valid as [middle_coherent [middle_balance _]].
  pose proof (@deactivation_initial_configuration_is_valid vertices
    (add_roots roots added) middle removed middle_coherent middle_balance
    removed_domain authorized) as deactivation_valid.
  pose proof (@drained_execution_has_exact_root_reachability vertices
    (remove_roots (add_roots roots added) removed) Deactivation
    (Configuration middle removed []) final rank unique closed descending
    deactivation_valid deactivated) as final_valid.
  destruct final_valid as [final_coherent [final_balance _]].
  eapply quiescent_ledger_is_unique; eauto.
Qed.

Fixpoint selected_sum (vertices : list nat) (selected : membership)
  (weight : nat -> nat) : nat :=
  match vertices with
  | [] => 0
  | vertex :: rest =>
      (if selected vertex then weight vertex else 0) + selected_sum rest selected weight
  end.

Lemma selected_sum_extensional : forall vertices left right weight,
  (forall vertex, left vertex = right vertex) ->
  selected_sum vertices left weight = selected_sum vertices right weight.
Proof.
  induction vertices as [|head tail IH]; intros left right weight agreement;
    [reflexivity|].
  cbn [selected_sum]. rewrite (agreement head), (IH left right weight agreement).
  reflexivity.
Qed.

Lemma selected_sum_replace_absent : forall vertices selected vertex enabled weight,
  ~ In vertex vertices ->
  selected_sum vertices (replace selected vertex enabled) weight =
  selected_sum vertices selected weight.
Proof.
  induction vertices as [|head tail IH]; intros selected vertex enabled weight absent;
    [reflexivity|].
  assert (different : Nat.eqb head vertex = false).
  { apply Nat.eqb_neq. intros same. subst head. apply absent; now left. }
  cbn [selected_sum]. unfold replace at 1. rewrite different.
  rewrite IH; [reflexivity|]. intros member. apply absent; now right.
Qed.

Lemma selected_sum_replace_present : forall vertices selected vertex enabled weight,
  NoDup vertices -> In vertex vertices ->
  selected_sum vertices (replace selected vertex enabled) weight +
    (if selected vertex then weight vertex else 0) =
  selected_sum vertices selected weight + (if enabled then weight vertex else 0).
Proof.
  induction vertices as [|head tail IH]; intros selected vertex enabled weight unique member;
    [contradiction|].
  inversion unique as [|? ? absent tail_unique]; subst.
  destruct member as [same|member].
  - subst head. cbn [selected_sum]. unfold replace at 1. rewrite Nat.eqb_refl.
    rewrite selected_sum_replace_absent by exact absent. lia.
  - assert (different : Nat.eqb head vertex = false).
    { apply Nat.eqb_neq. intros same. subst head. contradiction. }
    cbn [selected_sum]. unfold replace at 1. rewrite different.
    pose proof (IH selected vertex enabled weight tail_unique member) as conserved. lia.
Qed.

Definition expansion_credit (vertex : nat) : nat := 1 + 2 * length (children vertex).

Definition unexpanded (mode : phase) (state : ledger) (vertex : nat) : bool :=
  match mode with
  | Activation => negb (active state vertex)
  | Deactivation => active state vertex
  end.

Definition unspent_credit (mode : phase) (vertices : list nat) (state : ledger) : nat :=
  selected_sum vertices (unexpanded mode state) expansion_credit.

Fixpoint cursor_work (cursors : list child_cursor) : nat :=
  match cursors with
  | [] => 0
  | (_, remaining) :: rest => 1 + 2 * length remaining + cursor_work rest
  end.

Definition pending_work (machine : configuration) : nat :=
  length (events machine) + cursor_work (frames machine).

Definition remaining_work (mode : phase) (vertices : list nat)
  (machine : configuration) : nat :=
  pending_work machine + unspent_credit mode vertices (resident machine).

Lemma increment_releases_exact_expansion_credit : forall vertices state vertex,
  NoDup vertices -> In vertex vertices -> coherent state ->
  unspent_credit Activation vertices (increment state vertex) +
    (if Nat.eqb (counts state vertex) 0 then expansion_credit vertex else 0) =
  unspent_credit Activation vertices state.
Proof.
  intros vertices state vertex unique member agreement.
  assert (updated : unspent_credit Activation vertices (increment state vertex) =
    selected_sum vertices (replace (unexpanded Activation state) vertex false)
      expansion_credit).
  { unfold unspent_credit. apply selected_sum_extensional. intros query.
    unfold unexpanded, increment; cbn [active]. unfold replace.
    destruct (Nat.eqb query vertex); reflexivity. }
  rewrite updated.
  pose proof (@selected_sum_replace_present vertices (unexpanded Activation state)
    vertex false expansion_credit unique member) as changed.
  unfold unexpanded in changed. rewrite (agreement vertex) in changed.
  unfold unspent_credit, unexpanded.
  destruct (counts state vertex); cbn in changed |- *; lia.
Qed.

Lemma decrement_releases_exact_expansion_credit : forall vertices state vertex,
  NoDup vertices -> In vertex vertices -> coherent state ->
  unspent_credit Deactivation vertices (decrement state vertex) +
    (if Nat.eqb (counts state vertex) 1 then expansion_credit vertex else 0) =
  unspent_credit Deactivation vertices state.
Proof.
  intros vertices state vertex unique member agreement.
  pose proof (@selected_sum_replace_present vertices (active state) vertex
    (1 <? counts state vertex) expansion_credit unique member) as changed.
  rewrite (agreement vertex) in changed.
  change (selected_sum vertices (replace (active state) vertex
    (1 <? counts state vertex)) expansion_credit +
    (if Nat.eqb (counts state vertex) 1 then expansion_credit vertex else 0) =
    selected_sum vertices (active state) expansion_credit).
  destruct (counts state vertex) as [|[|count]]; cbn in changed |- *; lia.
Qed.

Theorem every_cursor_step_spends_exactly_one : forall vertices roots mode before after,
  NoDup vertices -> valid_configuration mode vertices roots before ->
  cursor_step mode before after ->
  remaining_work mode vertices before = S (remaining_work mode vertices after).
Proof.
  intros vertices roots mode before after unique valid transition.
  destruct transition as [state vertex explicit cursors|state vertex explicit cursors|
    mode state explicit parent child remaining cursors|mode state explicit parent cursors];
    destruct valid as [agreement [balance [event_domain cursor_domain]]];
    cbn [resident events frames] in *.
  - inversion event_domain as [|? ? member _]; subst.
    pose proof (@increment_releases_exact_expansion_credit vertices state vertex
      unique member agreement) as spent.
    unfold remaining_work, pending_work. cbn [resident events frames length].
    unfold increment_cursors. destruct (Nat.eqb (counts state vertex) 0);
      cbn [cursor_work] in *; unfold expansion_credit in *; lia.
  - inversion event_domain as [|? ? member _]; subst.
    pose proof (@decrement_releases_exact_expansion_credit vertices state vertex
      unique member agreement) as spent.
    unfold remaining_work, pending_work. cbn [resident events frames length].
    unfold decrement_cursors. destruct (Nat.eqb (counts state vertex) 1);
      cbn [cursor_work] in *; unfold expansion_credit in *; lia.
  - unfold remaining_work, pending_work. cbn [resident events frames cursor_work length]. lia.
  - unfold remaining_work, pending_work. cbn [resident events frames cursor_work length]. lia.
Qed.

Definition drained (machine : configuration) : Prop :=
  events machine = [] /\ frames machine = [].

Theorem zero_pending_work_is_exactly_drained : forall machine,
  pending_work machine = 0 <-> drained machine.
Proof.
  intros [state explicit cursors].
  unfold pending_work, drained. cbn [events frames]. split.
  - intros zero. destruct explicit as [|vertex explicit];
      [|cbn [length] in zero; lia].
    split; [reflexivity|]. destruct cursors as [|[parent remaining] cursors];
      [reflexivity|cbn [length cursor_work] in zero; lia].
  - intros [-> ->]. reflexivity.
Qed.

Theorem cursor_machine_progress : forall mode machine,
  drained machine \/ exists next, cursor_step mode machine next.
Proof.
  intros mode [state explicit cursors]. destruct explicit as [|vertex explicit].
  - destruct cursors as [|[parent remaining] cursors].
    + left; split; reflexivity.
    + right. destruct remaining as [|child remaining]; eexists; constructor.
  - right. destruct mode; eexists; constructor.
Qed.

Theorem drained_machine_cannot_step : forall mode machine next,
  drained machine -> ~ cursor_step mode machine next.
Proof.
  intros mode machine next done transition. destruct transition;
    destruct done as [empty_events empty_frames];
    cbn [events frames] in *; discriminate.
Qed.

Definition valid_cursor_predecessor (mode : phase) (vertices : list nat) (roots : counter)
  (after before : configuration) : Prop :=
  valid_configuration mode vertices roots before /\ cursor_step mode before after.

Theorem valid_cursor_steps_are_well_founded : forall vertices roots mode,
  NoDup vertices -> well_founded (valid_cursor_predecessor mode vertices roots).
Proof.
  intros vertices roots mode unique.
  apply (@well_founded_lt_compat configuration (remaining_work mode vertices)
    (valid_cursor_predecessor mode vertices roots)).
  intros after before [valid transition].
  pose proof (@every_cursor_step_spends_exactly_one vertices roots mode before after
    unique valid transition) as spent. lia.
Qed.

Inductive measured_execution (mode : phase) (initial : configuration) :
  nat -> configuration -> Prop :=
| measured_start : measured_execution mode initial 0 initial
| measured_step : forall steps before after,
    measured_execution mode initial steps before -> cursor_step mode before after ->
    measured_execution mode initial (S steps) after.

Lemma measured_execution_erases : forall mode initial steps final,
  measured_execution mode initial steps final -> cursor_execution mode initial final.
Proof.
  intros mode initial steps final execution. induction execution.
  - constructor.
  - eapply execution_step; eauto.
Qed.

Lemma every_execution_has_a_step_count : forall mode initial final,
  cursor_execution mode initial final ->
  exists steps, measured_execution mode initial steps final.
Proof.
  intros mode initial final execution. induction execution.
  - exists 0; constructor.
  - destruct IHexecution as [steps measured]. exists (S steps).
    eapply measured_step; eauto.
Qed.

Lemma measured_execution_prepend : forall mode initial next steps final,
  cursor_step mode initial next -> measured_execution mode next steps final ->
  measured_execution mode initial (S steps) final.
Proof.
  intros mode initial next steps final first rest. induction rest.
  - eapply measured_step; [constructor|exact first].
  - eapply measured_step; eauto.
Qed.

Theorem execution_conserves_exact_work : forall vertices roots mode initial steps final,
  NoDup vertices ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    In child vertices) ->
  valid_configuration mode vertices roots initial ->
  measured_execution mode initial steps final ->
  steps + remaining_work mode vertices final = remaining_work mode vertices initial.
Proof.
  intros vertices roots mode initial steps final unique closed valid execution.
  induction execution; [reflexivity|].
  pose proof (@measured_execution_erases mode initial steps before execution) as erased.
  pose proof (@cursor_execution_preserves_configuration vertices roots mode initial before
    unique closed valid erased) as before_valid.
  pose proof (@every_cursor_step_spends_exactly_one vertices roots mode before after
    unique before_valid H) as spent. lia.
Qed.

Theorem every_valid_machine_reaches_drainage : forall vertices roots mode,
  NoDup vertices ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    In child vertices) ->
  forall initial, valid_configuration mode vertices roots initial ->
    exists steps final,
      measured_execution mode initial steps (Configuration final [] []).
Proof.
  intros vertices roots mode unique closed.
  apply (@well_founded_induction configuration
    (valid_cursor_predecessor mode vertices roots)
    (@valid_cursor_steps_are_well_founded vertices roots mode unique)
    (fun initial => valid_configuration mode vertices roots initial ->
      exists steps final,
        measured_execution mode initial steps (Configuration final [] []))).
  intros initial IH valid.
  destruct (cursor_machine_progress mode initial) as [done|[next transition]].
  - destruct initial as [state explicit cursors].
    destruct done as [empty_events empty_frames]. cbn [events frames] in *.
    subst explicit cursors. exists 0, state. constructor.
  - assert (next_valid : valid_configuration mode vertices roots next).
    { eapply cursor_step_preserves_configuration; eauto. }
    destruct (IH next (conj valid transition) next_valid) as [steps [final execution]].
    exists (S steps), final. eapply measured_execution_prepend; eauto.
Qed.

Theorem cursor_step_cannot_restore_expansion_credit : forall mode before after,
  coherent (resident before) -> cursor_step mode before after ->
  forall vertex, unexpanded mode (resident after) vertex = true ->
    unexpanded mode (resident before) vertex = true.
Proof.
  intros mode before after agreement transition query enabled.
  destruct transition; cbn [resident] in *; [| |exact enabled|exact enabled].
  - unfold unexpanded, increment in enabled; cbn [active] in enabled.
    unfold replace in enabled. destruct (Nat.eqb query vertex);
      [discriminate|exact enabled].
  - unfold unexpanded, decrement in enabled; cbn [active] in enabled.
    unfold replace in enabled. destruct (Nat.eqb query vertex) eqn:same;
      [|exact enabled].
    apply Nat.eqb_eq in same. subst query.
    unfold unexpanded. rewrite (agreement vertex). apply Nat.ltb_lt.
    apply Nat.ltb_lt in enabled. lia.
Qed.

Theorem cursor_execution_cannot_restore_expansion_credit :
  forall vertices roots mode initial final,
  NoDup vertices ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    In child vertices) ->
  valid_configuration mode vertices roots initial ->
  cursor_execution mode initial final ->
  forall vertex, unexpanded mode (resident final) vertex = true ->
    unexpanded mode (resident initial) vertex = true.
Proof.
  intros vertices roots mode initial final unique closed valid execution.
  induction execution; [auto|].
  pose proof (@cursor_execution_preserves_configuration vertices roots mode initial before
    unique closed valid execution) as before_valid.
  destruct before_valid as [agreement _]. intros vertex enabled.
  apply IHexecution. eapply cursor_step_cannot_restore_expansion_credit; eauto.
Qed.

Lemma selected_sum_partition : forall vertices before after weight,
  (forall vertex, after vertex = true -> before vertex = true) ->
  selected_sum vertices before weight = selected_sum vertices after weight +
    selected_sum vertices (fun vertex => before vertex && negb (after vertex)) weight.
Proof.
  induction vertices as [|head tail IH]; intros before after weight monotone;
    [reflexivity|].
  cbn [selected_sum]. rewrite (IH before after weight monotone).
  specialize (monotone head).
  destruct (before head), (after head); cbn in *; try congruence; lia.
Qed.

Definition changed_expansion_work (mode : phase) (vertices : list nat)
  (before after : ledger) : nat :=
  selected_sum vertices
    (fun vertex => unexpanded mode before vertex && negb (unexpanded mode after vertex))
    expansion_credit.

Theorem drained_execution_has_exact_changed_frontier_work :
  forall vertices roots mode initial steps final,
  NoDup vertices ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    In child vertices) ->
  valid_configuration mode vertices roots initial ->
  measured_execution mode initial steps (Configuration final [] []) ->
  steps = pending_work initial +
    changed_expansion_work mode vertices (resident initial) final.
Proof.
  intros vertices roots mode initial steps final unique closed valid execution.
  pose proof (@execution_conserves_exact_work vertices roots mode initial steps
    (Configuration final [] []) unique closed valid execution) as conserved.
  pose proof (@measured_execution_erases mode initial steps
    (Configuration final [] []) execution) as erased.
  pose proof (@cursor_execution_cannot_restore_expansion_credit vertices roots mode
    initial (Configuration final [] []) unique closed valid erased) as monotone.
  pose proof (@selected_sum_partition vertices (unexpanded mode (resident initial))
    (unexpanded mode final) expansion_credit monotone) as partitioned.
  unfold remaining_work, unspent_credit in conserved.
  unfold pending_work in conserved at 1.
  cbn [events frames resident cursor_work length] in conserved.
  unfold changed_expansion_work. lia.
Qed.

Theorem root_delta_execution_has_exact_changed_frontier_work :
  forall vertices roots mode initial explicit steps final,
  NoDup vertices ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    In child vertices) ->
  valid_configuration mode vertices roots (Configuration initial explicit []) ->
  measured_execution mode (Configuration initial explicit []) steps
    (Configuration final [] []) ->
  steps = length explicit + changed_expansion_work mode vertices initial final.
Proof.
  intros vertices roots mode initial explicit steps final unique closed valid execution.
  pose proof (@drained_execution_has_exact_changed_frontier_work vertices roots mode
    (Configuration initial explicit []) steps final unique closed valid execution) as exact_work.
  unfold pending_work in exact_work. cbn [events frames resident cursor_work] in exact_work.
  lia.
Qed.

Lemma no_roots_no_reachability : forall vertices roots,
  (forall vertex, roots vertex = 0) ->
  forall vertex, ~ reachable vertices roots vertex.
Proof.
  intros vertices roots empty vertex path. induction path.
  - rewrite empty in H. lia.
  - exact IHpath.
Qed.

Theorem cursor_suffix_does_not_establish_processed_history : forall parent child remaining,
  children parent = child :: remaining ->
  authenticated_cursor (parent, remaining) /\
    pending [] [(parent, child :: remaining)] child =
      S (pending [] [(parent, remaining)] child).
Proof.
  intros parent child remaining full. split.
  - exists [child]. exact full.
  - unfold pending. cbn [occurrences cursor_pending]. rewrite Nat.eqb_refl. lia.
Qed.

End Graph.

Theorem rootless_cycle_has_a_spurious_quiescent_ledger :
  let edges := fun _ : nat => [0] in
  let roots := fun _ : nat => 0 in
  let state := {| counts := fun vertex => if Nat.eqb vertex 0 then 1 else 0;
                  active := fun vertex => Nat.eqb vertex 0 |} in
  coherent state /\ quiescent edges [0] roots state /\
    active state 0 = true /\ ~ reachable edges [0] roots 0.
Proof.
  cbn zeta. split.
  - intros vertex. cbn [active counts]. destruct (Nat.eqb vertex 0); reflexivity.
  - split.
    + intros vertex. cbn [counts active incoming occurrences].
      destruct (Nat.eqb vertex 0); reflexivity.
    + split; [reflexivity|]. apply no_roots_no_reachability. reflexivity.
Qed.

End IncrementalReachability.

Module CheckedReachability.

Import IncrementalReachability.

Definition checked_add (limit left right : nat) : option nat :=
  if left + right <=? limit then Some (left + right) else None.

Definition checked_sub (left right : nat) : option nat :=
  if right <=? left then Some (left - right) else None.

Theorem checked_add_is_exact : forall limit left right result,
  checked_add limit left right = Some result <->
  result = left + right /\ left + right <= limit.
Proof.
  intros limit left right result. unfold checked_add.
  destruct (left + right <=? limit) eqn:fits.
  - apply Nat.leb_le in fits. split; intros success; [inversion success; auto|].
    destruct success as [-> _]. reflexivity.
  - apply Nat.leb_gt in fits. split; intros success; [discriminate|].
    destruct success; lia.
Qed.

Theorem checked_add_refuses_exactly_above_limit : forall limit left right,
  checked_add limit left right = None <-> limit < left + right.
Proof.
  intros limit left right. unfold checked_add.
  destruct (left + right <=? limit) eqn:fits;
    [apply Nat.leb_le in fits|apply Nat.leb_gt in fits]; split; intros; try discriminate;
    try reflexivity; lia.
Qed.

Theorem checked_sub_is_exact : forall left right result,
  checked_sub left right = Some result <-> result + right = left /\ right <= left.
Proof.
  intros left right result. unfold checked_sub.
  destruct (right <=? left) eqn:funded.
  - apply Nat.leb_le in funded. split.
    + intros success; inversion success; lia.
    + intros [exact enough]. f_equal; lia.
  - apply Nat.leb_gt in funded. split; intros success; [discriminate|].
    destruct success; lia.
Qed.

Theorem checked_sub_refuses_exactly_underflow : forall left right,
  checked_sub left right = None <-> left < right.
Proof.
  intros left right. unfold checked_sub.
  destruct (right <=? left) eqn:funded;
    [apply Nat.leb_le in funded|apply Nat.leb_gt in funded]; split; intros; try discriminate;
    try reflexivity; lia.
Qed.

Definition checked_increment (limit : nat) (state : ledger) (vertex : nat) : option ledger :=
  if counts state vertex <? limit then Some (increment state vertex) else None.

Definition checked_decrement (state : ledger) (vertex : nat) : option ledger :=
  if 0 <? counts state vertex then Some (decrement state vertex) else None.

Theorem checked_increment_refines_natural_update : forall limit state vertex next,
  checked_increment limit state vertex = Some next <->
  next = increment state vertex /\ counts state vertex < limit.
Proof.
  intros limit state vertex next. unfold checked_increment.
  destruct (counts state vertex <? limit) eqn:fits.
  - apply Nat.ltb_lt in fits. split; intros success; [inversion success; auto|].
    destruct success as [-> _]; reflexivity.
  - apply Nat.ltb_ge in fits. split; intros success; [discriminate|].
    destruct success; lia.
Qed.

Theorem checked_increment_refuses_exactly_at_capacity : forall limit state vertex,
  checked_increment limit state vertex = None <-> limit <= counts state vertex.
Proof.
  intros limit state vertex. unfold checked_increment.
  destruct (counts state vertex <? limit) eqn:fits;
    [apply Nat.ltb_lt in fits|apply Nat.ltb_ge in fits]; split; intros; try discriminate;
    try reflexivity; lia.
Qed.

Theorem checked_decrement_refines_natural_update : forall state vertex next,
  checked_decrement state vertex = Some next <->
  next = decrement state vertex /\ 0 < counts state vertex.
Proof.
  intros state vertex next. unfold checked_decrement.
  destruct (0 <? counts state vertex) eqn:funded.
  - apply Nat.ltb_lt in funded. split; intros success; [inversion success; auto|].
    destruct success as [-> _]; reflexivity.
  - apply Nat.ltb_ge in funded. split; intros success; [discriminate|].
    destruct success; lia.
Qed.

Theorem checked_decrement_refuses_exactly_zero : forall state vertex,
  checked_decrement state vertex = None <-> counts state vertex = 0.
Proof.
  intros state vertex. unfold checked_decrement.
  destruct (counts state vertex); cbn; split; intros; try discriminate; reflexivity.
Qed.

Definition bounded_counts (limit : nat) (vertices : list nat) (state : ledger) : Prop :=
  forall vertex, In vertex vertices -> counts state vertex <= limit.

Theorem checked_increment_preserves_bounded_counts : forall limit vertices state vertex next,
  bounded_counts limit vertices state ->
  checked_increment limit state vertex = Some next -> bounded_counts limit vertices next.
Proof.
  intros limit vertices state vertex next bounded success.
  apply checked_increment_refines_natural_update in success.
  destruct success as [-> available]. intros query member.
  cbn [increment counts]. unfold replace.
  destruct (Nat.eqb query vertex); [lia|apply bounded; exact member].
Qed.

Theorem checked_decrement_preserves_bounded_counts : forall limit vertices state vertex next,
  bounded_counts limit vertices state ->
  checked_decrement state vertex = Some next -> bounded_counts limit vertices next.
Proof.
  intros limit vertices state vertex next bounded success.
  apply checked_decrement_refines_natural_update in success.
  destruct success as [-> available]. intros query member.
  cbn [decrement counts]. unfold replace.
  destruct (Nat.eqb query vertex) eqn:same.
  - apply Nat.eqb_eq in same. subst query. specialize (bounded vertex member); lia.
  - apply bounded; exact member.
Qed.

Theorem checked_updates_preserve_coherence : forall limit state vertex,
  coherent state ->
  (forall next, checked_increment limit state vertex = Some next -> coherent next) /\
  (forall next, checked_decrement state vertex = Some next -> coherent next).
Proof.
  intros limit state vertex agreement. split; intros next success.
  - apply checked_increment_refines_natural_update in success. destruct success as [-> _].
    now apply increment_preserves_coherence.
  - apply checked_decrement_refines_natural_update in success. destruct success as [-> _].
    now apply decrement_preserves_coherence.
Qed.

Definition inventory (vertices : list nat) (state : ledger) (weight : nat -> nat) : nat :=
  selected_sum vertices (active state) weight.

Definition activated_weight (state : ledger) (vertex : nat) (weight : nat -> nat) : nat :=
  if counts state vertex =? 0 then weight vertex else 0.

Definition deactivated_weight (state : ledger) (vertex : nat) (weight : nat -> nat) : nat :=
  if counts state vertex =? 1 then weight vertex else 0.

Theorem increment_changes_unique_inventory : forall vertices state vertex weight,
  NoDup vertices -> In vertex vertices -> coherent state ->
  inventory vertices (increment state vertex) weight =
    inventory vertices state weight + activated_weight state vertex weight.
Proof.
  intros vertices state vertex weight unique member agreement.
  pose proof (@selected_sum_replace_present vertices (active state) vertex true weight
    unique member) as changed.
  rewrite (agreement vertex) in changed.
  unfold inventory, activated_weight. cbn [increment active].
  destruct (counts state vertex); cbn in changed |- *; lia.
Qed.

Theorem decrement_changes_unique_inventory : forall vertices state vertex weight,
  NoDup vertices -> In vertex vertices -> coherent state ->
  inventory vertices state weight =
    inventory vertices (decrement state vertex) weight + deactivated_weight state vertex weight.
Proof.
  intros vertices state vertex weight unique member agreement.
  pose proof (@selected_sum_replace_present vertices (active state) vertex
    (1 <? counts state vertex) weight unique member) as changed.
  rewrite (agreement vertex) in changed.
  unfold inventory, deactivated_weight. cbn [decrement active].
  destruct (counts state vertex) as [|[|count]]; cbn in changed |- *; lia.
Qed.

Theorem deactivated_inventory_is_funded : forall vertices state vertex weight,
  NoDup vertices -> In vertex vertices -> coherent state ->
  deactivated_weight state vertex weight <= inventory vertices state weight.
Proof.
  intros vertices state vertex weight unique member agreement.
  pose proof (@decrement_changes_unique_inventory vertices state vertex weight
    unique member agreement). lia.
Qed.

Theorem checked_inventory_add_is_exact : forall limit vertices state vertex weight cached result,
  NoDup vertices -> In vertex vertices -> coherent state ->
  cached = inventory vertices state weight ->
  checked_add limit cached (activated_weight state vertex weight) = Some result ->
  result = inventory vertices (increment state vertex) weight /\ result <= limit.
Proof.
  intros limit vertices state vertex weight cached result unique member agreement exact success.
  apply checked_add_is_exact in success.
  pose proof (@increment_changes_unique_inventory vertices state vertex weight
    unique member agreement). subst cached. destruct success; split; lia.
Qed.

Theorem checked_inventory_remove_is_exact : forall vertices state vertex weight cached result,
  NoDup vertices -> In vertex vertices -> coherent state ->
  cached = inventory vertices state weight ->
  checked_sub cached (deactivated_weight state vertex weight) = Some result ->
  result = inventory vertices (decrement state vertex) weight.
Proof.
  intros vertices state vertex weight cached result unique member agreement exact success.
  apply checked_sub_is_exact in success.
  pose proof (@decrement_changes_unique_inventory vertices state vertex weight
    unique member agreement). subst cached. destruct success; lia.
Qed.

Theorem exact_inventory_removal_cannot_underflow : forall vertices state vertex weight,
  NoDup vertices -> In vertex vertices -> coherent state ->
  exists result,
    checked_sub (inventory vertices state weight) (deactivated_weight state vertex weight) =
      Some result.
Proof.
  intros vertices state vertex weight unique member agreement.
  exists (inventory vertices (decrement state vertex) weight).
  apply checked_sub_is_exact. split.
  - symmetry. now apply decrement_changes_unique_inventory.
  - now apply deactivated_inventory_is_funded.
Qed.

End CheckedReachability.

Module CatalogAppointedPrefix.

Theorem discarding_unselected_suffix_preserves_appointed_bytes :
  forall (prefix suffix : list nat),
    firstn (length prefix) (prefix ++ suffix) = prefix.
Proof.
  intros prefix; induction prefix; intros suffix; simpl; auto.
  now rewrite IHprefix.
Qed.

Theorem retry_after_suffix_discard_preserves_appointed_bytes :
  forall (prefix orphan replacement : list nat),
    firstn (length prefix)
      (firstn (length prefix) (prefix ++ orphan) ++ replacement) = prefix.
Proof.
  intros; repeat rewrite discarding_unselected_suffix_preserves_appointed_bytes.
  reflexivity.
Qed.

End CatalogAppointedPrefix.

Print Assumptions IncrementalReachability.selected_sum_extensional.
Print Assumptions IncrementalReachability.selected_sum_replace_absent.
Print Assumptions IncrementalReachability.selected_sum_replace_present.
Print Assumptions IncrementalReachability.increment_releases_exact_expansion_credit.
Print Assumptions IncrementalReachability.decrement_releases_exact_expansion_credit.
Print Assumptions IncrementalReachability.every_cursor_step_spends_exactly_one.
Print Assumptions IncrementalReachability.zero_pending_work_is_exactly_drained.
Print Assumptions IncrementalReachability.cursor_machine_progress.
Print Assumptions IncrementalReachability.drained_machine_cannot_step.
Print Assumptions IncrementalReachability.valid_cursor_steps_are_well_founded.
Print Assumptions IncrementalReachability.measured_execution_erases.
Print Assumptions IncrementalReachability.every_execution_has_a_step_count.
Print Assumptions IncrementalReachability.measured_execution_prepend.
Print Assumptions IncrementalReachability.execution_conserves_exact_work.
Print Assumptions IncrementalReachability.every_valid_machine_reaches_drainage.
Print Assumptions IncrementalReachability.cursor_step_cannot_restore_expansion_credit.
Print Assumptions IncrementalReachability.cursor_execution_cannot_restore_expansion_credit.
Print Assumptions IncrementalReachability.selected_sum_partition.
Print Assumptions IncrementalReachability.drained_execution_has_exact_changed_frontier_work.
Print Assumptions IncrementalReachability.root_delta_execution_has_exact_changed_frontier_work.

Print Assumptions IncrementalReachability.incoming_replace_absent.
Print Assumptions IncrementalReachability.incoming_replace_present.
Print Assumptions IncrementalReachability.transferring_one_cursor_occurrence_preserves_pending.
Print Assumptions IncrementalReachability.discarding_an_exhausted_cursor_preserves_pending.
Print Assumptions IncrementalReachability.increment_preserves_coherence.
Print Assumptions IncrementalReachability.increment_preserves_activation_balance.
Print Assumptions IncrementalReachability.pending_decrement_cannot_underflow.
Print Assumptions IncrementalReachability.decrement_preserves_coherence.
Print Assumptions IncrementalReachability.decrement_preserves_deactivation_balance.
Print Assumptions IncrementalReachability.activation_starts_with_added_root_occurrences.
Print Assumptions IncrementalReachability.deactivation_starts_with_authorized_root_occurrences.
Print Assumptions IncrementalReachability.drained_activation_is_quiescent.
Print Assumptions IncrementalReachability.drained_deactivation_is_quiescent.
Print Assumptions IncrementalReachability.complete_child_cursor_is_authenticated.
Print Assumptions IncrementalReachability.advancing_a_cursor_preserves_its_authenticated_suffix.
Print Assumptions IncrementalReachability.increment_preserves_cursor_authentication.
Print Assumptions IncrementalReachability.decrement_preserves_cursor_authentication.
Print Assumptions IncrementalReachability.occurrences_positive.
Print Assumptions IncrementalReachability.incoming_positive.
Print Assumptions IncrementalReachability.root_reachability_implies_positive_count.
Print Assumptions IncrementalReachability.member_rank_is_bounded.
Print Assumptions IncrementalReachability.positive_count_implies_root_reachability.
Print Assumptions IncrementalReachability.quiescent_support_is_exact_root_reachability.
Print Assumptions IncrementalReachability.incoming_extensional.
Print Assumptions IncrementalReachability.quiescent_ledger_is_unique.
Print Assumptions IncrementalReachability.authenticated_cursor_head_is_an_edge.
Print Assumptions IncrementalReachability.cursor_step_preserves_configuration.
Print Assumptions IncrementalReachability.cursor_execution_preserves_configuration.
Print Assumptions IncrementalReachability.drained_execution_has_exact_root_reachability.
Print Assumptions IncrementalReachability.activation_initial_configuration_is_valid.
Print Assumptions IncrementalReachability.deactivation_initial_configuration_is_valid.
Print Assumptions IncrementalReachability.add_before_remove_agrees_with_cold_ledger.
Print Assumptions IncrementalReachability.no_roots_no_reachability.
Print Assumptions IncrementalReachability.cursor_suffix_does_not_establish_processed_history.
Print Assumptions IncrementalReachability.rootless_cycle_has_a_spurious_quiescent_ledger.

Print Assumptions CatalogAppointedPrefix.discarding_unselected_suffix_preserves_appointed_bytes.
Print Assumptions JournalVersionFreshness.advancing_never_reuses_current_version.
Print Assumptions JournalVersionFreshness.arbitrary_nonempty_history_cannot_return_to_old_version.
Print Assumptions JournalVersionFreshness.rollover_uses_a_fresh_identity.
Print Assumptions JournalOwnerReadiness.readiness_requires_exact_registry_and_catalog.
Print Assumptions JournalOwnerReadiness.invalid_binding_retains_the_original_pending_receipt.
Print Assumptions JournalOwnerReadiness.valid_base_allows_abort_independently_of_receipt_revision.
Print Assumptions CatalogAppointedPrefix.retry_after_suffix_discard_preserves_appointed_bytes.
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
