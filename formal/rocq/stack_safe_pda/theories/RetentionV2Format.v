From Stdlib Require Import Bool List Arith Lia.
From Stdlib Require Import Sorting.Permutation Sorting.Sorted.
From StackSafePDA Require ChannelImage.
From StackSafePDA Require Import GenerationCheckpointPublication IncrementalLedgerTrial
  IncrementalDomainComposition.

Import ListNotations.

(**
  Shared retention format V2.

  The V1 retention manifest stores the current retained state and the whole
  list of historical retained states, so every successor re-encodes the whole
  history.  The V2 format keeps the catalog envelope and the V1 commit byte
  language unchanged.  The existing opaque retention reference appoints a
  constant-size anchor object in the new domain [RSpaceDAG.RetA].  The anchor
  carries the V1 current entry verbatim, the exact historical entry count and
  the root of a radix tree over 48-byte keys whose nodes reuse the canonical
  DAG node body under the new domain [RSpaceDAG.RetR].  A leaf key is the
  complete state identity [BE64(generation) ++ digest ++ BE64(encoded_len)]
  and the leaf endpoint is the state's byte claim, so key-to-entry equality
  holds by construction.

  The development proves, without axioms:
  - the key is an exact 48-byte bijection whose byte order is the V1 entry
    order; little-endian and lossy keys are refuted by closed counterexamples;
  - the eighteen 16-byte domain constants of the persistence format are
    pairwise distinct, so collision-checked storage never aliases a metadata
    object with an object of an existing domain, while radix nodes under the
    semantic node domain would alias the empty metadata and semantic roots;
  - the radix count denotes entry occurrences, every unfolded key has the full
    width and the keys strictly increase, while counting distinct leaf objects
    undercounts shared leaves;
  - V2 checkpoint data is the disjoint union of the retained semantic objects
    and the metadata closure, the V1 "+1" rule refuses every V2 inventory, and
    an anchor-only inventory is not reference-closed;
  - the one-time V1-to-V2 migration preserves entries, order, count, aliases
    and the current and retained root counters of [CheckedDomainComposition];
  - a transfer-session binding that includes the retention format separates
    the V1 and V2 representations of one state, while a binding over the state
    version alone resumes across formats;
  - the cold reader may count before it enumerates: a successful count
    guarantees an enumeration with exactly that many entries, while a count
    without the inner-node shape check accepts an empty node below the root;
  - a memoized count keyed by (object, remaining width) agrees with the
    count, while a memo keyed by the object alone accepts a misplaced shared
    leaf;
  - duplicate-free data that includes a disjoint retained union and metadata
    closure, with a matching count, is exactly their disjoint union, while
    without disjointness the same checks accept an unauthenticated object.

  Old-reader refusal of a V2 slot without a stale fallback is a selection and
  recovery property; the TLA+ module [RSpaceRetentionV2Upgrade] checks it.
*)

Module RetentionV2Format.

(** * Generic order lemmas *)

Lemma strongly_sorted_app {A : Type} (R : A -> A -> Prop) :
  forall left right,
    StronglySorted R left ->
    StronglySorted R right ->
    (forall x y, In x left -> In y right -> R x y) ->
    StronglySorted R (left ++ right).
Proof.
  induction left as [|head tail IH]; intros right left_sorted right_sorted across.
  - exact right_sorted.
  - apply StronglySorted_inv in left_sorted as [tail_sorted head_before].
    cbn [app]. constructor.
    + apply IH; [exact tail_sorted | exact right_sorted |].
      intros x y x_member y_member.
      apply across; [apply in_cons; exact x_member | exact y_member].
    + apply Forall_app. split; [exact head_before|].
      apply Forall_forall. intros y y_member.
      apply across; [apply in_eq | exact y_member].
Qed.

Lemma strongly_sorted_map {A B : Type}
    (R : A -> A -> Prop) (R' : B -> B -> Prop) (f : A -> B) :
  (forall x y, R x y -> R' (f x) (f y)) ->
  forall values, StronglySorted R values -> StronglySorted R' (map f values).
Proof.
  intros preserves values sorted.
  induction sorted as [|head tail tail_sorted IH head_before]; cbn [map]; constructor.
  - exact IH.
  - apply Forall_forall. intros image image_member.
    apply in_map_iff in image_member as [source [source_image source_member]].
    subst image. rewrite Forall_forall in head_before.
    apply preserves. exact (head_before source source_member).
Qed.

Lemma sorted_map_on {A B : Type}
    (P : A -> Prop) (R : A -> A -> Prop) (R' : B -> B -> Prop) (f : A -> B) :
  (forall x y, P x -> P y -> R x y -> R' (f x) (f y)) ->
  forall values, Forall P values -> Sorted R values -> Sorted R' (map f values).
Proof.
  intros preserves values all_valid sorted. revert all_valid.
  induction sorted as [|head tail tail_sorted IH head_relation];
    intro all_valid; cbn [map].
  - constructor.
  - constructor.
    + exact (IH (Forall_inv_tail all_valid)).
    + revert all_valid.
      destruct head_relation as [|next rest head_next];
        intro all_valid; cbn [map]; constructor.
      apply preserves;
        [exact (Forall_inv all_valid)
        |exact (Forall_inv (Forall_inv_tail all_valid))
        |exact head_next].
Qed.

(** * Byte strings *)

Fixpoint take_exact (count : nat) (input : list nat) : option (list nat * list nat) :=
  match count with
  | 0 => Some ([], input)
  | S remaining =>
      match input with
      | [] => None
      | head :: tail =>
          match take_exact remaining tail with
          | Some (taken, rest) => Some (head :: taken, rest)
          | None => None
          end
      end
  end.

Lemma take_exact_app_length :
  forall prefix suffix count,
    length prefix = count ->
    take_exact count (prefix ++ suffix) = Some (prefix, suffix).
Proof.
  induction prefix as [|head tail IH]; intros suffix count prefix_length; subst count.
  - reflexivity.
  - cbn [length app take_exact].
    rewrite (IH suffix (length tail) eq_refl). reflexivity.
Qed.

Lemma take_exact_some :
  forall count input taken rest,
    take_exact count input = Some (taken, rest) ->
    length taken = count /\ input = taken ++ rest.
Proof.
  induction count as [|count IH]; intros input taken rest.
  - cbn [take_exact]. intro taken_exact.
    injection taken_exact as taken_is rest_is. subst taken rest.
    split; reflexivity.
  - cbn [take_exact].
    destruct input as [|head tail]; [intro impossible; discriminate impossible|].
    destruct (take_exact count tail) as [[tail_taken tail_rest]|] eqn:tail_exact;
      cbv beta iota; [|intro impossible; discriminate impossible].
    intro taken_exact. injection taken_exact as taken_is rest_is. subst taken rest.
    destruct (IH tail tail_taken tail_rest tail_exact) as [taken_length tail_split].
    cbn [length app]. rewrite taken_length, tail_split. split; reflexivity.
Qed.

Lemma decode_fixed_be_some :
  forall width input value rest,
    ChannelImage.decode_fixed_be width input = Some (value, rest) ->
    value < 256 ^ width /\ input = ChannelImage.encode_fixed_be width value ++ rest.
Proof.
  induction width as [|width IH]; intros input value rest.
  - cbn [ChannelImage.decode_fixed_be]. intro decoded.
    injection decoded as value_is rest_is. subst value rest.
    split; [cbn; lia | reflexivity].
  - cbn [ChannelImage.decode_fixed_be].
    destruct (ChannelImage.decode_fixed_be width input) as [[prefix after]|] eqn:prefix_decoded;
      cbv beta iota; [|intro impossible; discriminate impossible].
    destruct after as [|current tail]; cbv beta iota;
      [intro impossible; discriminate impossible|].
    destruct (current <? 256) eqn:current_bound; cbv beta iota;
      [|intro impossible; discriminate impossible].
    intro decoded. injection decoded as value_is rest_is. subst value rest.
    apply Nat.ltb_lt in current_bound.
    destruct (IH input prefix (current :: tail) prefix_decoded) as [prefix_bound input_split].
    assert (quotient : (prefix * 256 + current) / 256 = prefix).
    { symmetry. apply (Nat.div_unique (prefix * 256 + current) 256 prefix current); lia. }
    assert (remainder : (prefix * 256 + current) mod 256 = current).
    { symmetry. apply (Nat.mod_unique (prefix * 256 + current) 256 prefix current); lia. }
    split.
    + rewrite Nat.pow_succ_r by lia. nia.
    + cbn [ChannelImage.encode_fixed_be].
      rewrite quotient, remainder, <- app_assoc. exact input_split.
Qed.

Lemma bytes_ltb_app_same_length :
  forall left_prefix right_prefix left_suffix right_suffix,
    length left_prefix = length right_prefix ->
    ChannelImage.bytes_ltb (left_prefix ++ left_suffix) (right_prefix ++ right_suffix) =
      if ChannelImage.bytes_ltb left_prefix right_prefix then true
      else if ChannelImage.bytes_ltb right_prefix left_prefix then false
      else ChannelImage.bytes_ltb left_suffix right_suffix.
Proof.
  induction left_prefix as [|left_head left_tail IH];
    intros [|right_head right_tail] left_suffix right_suffix same_length;
    cbn [length] in same_length; try discriminate same_length.
  - reflexivity.
  - injection same_length as tail_length. simpl.
    rewrite (IH right_tail left_suffix right_suffix tail_length).
    destruct (Nat.ltb_spec left_head right_head);
      destruct (Nat.ltb_spec right_head left_head); try lia; reflexivity.
Qed.

Lemma bytes_ltb_same_prefix :
  forall prefix left right,
    ChannelImage.bytes_ltb (prefix ++ left) (prefix ++ right) =
      ChannelImage.bytes_ltb left right.
Proof.
  intros prefix left right.
  rewrite bytes_ltb_app_same_length by reflexivity.
  rewrite ChannelImage.bytes_ltb_irreflexive. reflexivity.
Qed.

Lemma bytes_ltb_singletons :
  forall left right, ChannelImage.bytes_ltb [left] [right] = (left <? right).
Proof.
  intros left right. simpl.
  destruct (Nat.ltb_spec left right); destruct (Nat.ltb_spec right left);
    try lia; reflexivity.
Qed.

Lemma bytes_ltb_cons_lt :
  forall left_head right_head left_tail right_tail,
    left_head < right_head ->
    ChannelImage.bytes_ltb (left_head :: left_tail) (right_head :: right_tail) = true.
Proof.
  intros left_head right_head left_tail right_tail below. simpl.
  apply Nat.ltb_lt in below. rewrite below. reflexivity.
Qed.

Lemma bytes_ltb_transitive :
  forall left middle right,
    ChannelImage.bytes_ltb left middle = true ->
    ChannelImage.bytes_ltb middle right = true ->
    ChannelImage.bytes_ltb left right = true.
Proof.
  induction left as [|left_head left_tail IH];
    intros [|middle_head middle_tail] [|right_head right_tail];
    simpl; intros left_middle middle_right;
    try discriminate left_middle; try discriminate middle_right; try reflexivity.
  revert left_middle middle_right.
  destruct (Nat.ltb_spec left_head middle_head);
    destruct (Nat.ltb_spec middle_head left_head);
    destruct (Nat.ltb_spec middle_head right_head);
    destruct (Nat.ltb_spec right_head middle_head);
    destruct (Nat.ltb_spec left_head right_head);
    destruct (Nat.ltb_spec right_head left_head);
    intros left_middle middle_right; cbv beta iota in left_middle, middle_right |- *;
    try discriminate left_middle; try discriminate middle_right; try lia; try reflexivity;
    exact (IH middle_tail right_tail left_middle middle_right).
Qed.

Lemma encode_fixed_be_order :
  forall width left right,
    left < 256 ^ width ->
    right < 256 ^ width ->
    ChannelImage.bytes_ltb
      (ChannelImage.encode_fixed_be width left)
      (ChannelImage.encode_fixed_be width right) = (left <? right).
Proof.
  induction width as [|width IH]; intros left right left_bound right_bound.
  - cbn in left_bound, right_bound.
    assert (left = 0) as left_zero by lia. assert (right = 0) as right_zero by lia.
    subst left right. reflexivity.
  - rewrite Nat.pow_succ_r in left_bound, right_bound by lia.
    cbn [ChannelImage.encode_fixed_be].
    rewrite bytes_ltb_app_same_length
      by (rewrite !ChannelImage.encode_fixed_be_length; reflexivity).
    pose proof (Nat.div_mod left 256 ltac:(lia)) as left_division.
    pose proof (Nat.div_mod right 256 ltac:(lia)) as right_division.
    pose proof (Nat.mod_upper_bound left 256 ltac:(lia)) as left_remainder.
    pose proof (Nat.mod_upper_bound right 256 ltac:(lia)) as right_remainder.
    assert (left_quotient : left / 256 < 256 ^ width).
    { apply Nat.Private_NDivProp.div_lt_upper_bound; lia. }
    assert (right_quotient : right / 256 < 256 ^ width).
    { apply Nat.Private_NDivProp.div_lt_upper_bound; lia. }
    rewrite (IH (left / 256) (right / 256) left_quotient right_quotient).
    rewrite (IH (right / 256) (left / 256) right_quotient left_quotient).
    rewrite bytes_ltb_singletons.
    destruct (Nat.ltb_spec (left / 256) (right / 256));
      destruct (Nat.ltb_spec (right / 256) (left / 256));
      destruct (Nat.ltb_spec (left mod 256) (right mod 256));
      destruct (Nat.ltb_spec left right); try lia; reflexivity.
Qed.

(** * Retention keys *)

Definition object_envelope_header_len : nat := 32.
Definition retention_key_len : nat := 48.

(** A retained state identity: the generation and the [ObjectRefV1] of the
    state manifest, that is its 32-byte digest and its encoded length. *)
Record retention_identity := RetentionIdentity {
  identity_generation : nat;
  identity_digest : list nat;
  identity_encoded_len : nat
}.

Definition identity_ok (identity : retention_identity) : bool :=
  ChannelImage.fits_fixed_be 8 (identity_generation identity) &&
  Nat.eqb (length (identity_digest identity)) 32 &&
  ChannelImage.bytes_ok (identity_digest identity) &&
  ChannelImage.fits_fixed_be 8 (identity_encoded_len identity) &&
  Nat.leb object_envelope_header_len (identity_encoded_len identity).

Definition retention_key (identity : retention_identity) : list nat :=
  ChannelImage.encode_u64_be (identity_generation identity) ++
  identity_digest identity ++
  ChannelImage.encode_u64_be (identity_encoded_len identity).

Definition decode_retention_key (input : list nat) : option retention_identity :=
  match ChannelImage.decode_u64_be input with
  | Some (generation, after_generation) =>
      match take_exact 32 after_generation with
      | Some (digest, after_digest) =>
          match ChannelImage.decode_u64_be after_digest with
          | Some (encoded_len, []) =>
              if ChannelImage.bytes_ok digest &&
                 Nat.leb object_envelope_header_len encoded_len
              then Some (RetentionIdentity generation digest encoded_len)
              else None
          | _ => None
          end
      | None => None
      end
  | None => None
  end.

(** The V1 entry order: generation, then the manifest reference ordered by
    its digest bytes and then by its encoded length. *)
Definition identity_ltb (left right : retention_identity) : bool :=
  if identity_generation left <? identity_generation right then true
  else if identity_generation right <? identity_generation left then false
  else if ChannelImage.bytes_ltb (identity_digest left) (identity_digest right) then true
  else if ChannelImage.bytes_ltb (identity_digest right) (identity_digest left) then false
  else identity_encoded_len left <? identity_encoded_len right.

Lemma identity_ok_true_iff :
  forall identity,
    identity_ok identity = true <->
    identity_generation identity < 256 ^ 8 /\
    length (identity_digest identity) = 32 /\
    ChannelImage.bytes_ok (identity_digest identity) = true /\
    identity_encoded_len identity < 256 ^ 8 /\
    object_envelope_header_len <= identity_encoded_len identity.
Proof.
  intro identity. unfold identity_ok.
  rewrite !andb_true_iff, !ChannelImage.fits_fixed_be_true_iff, Nat.eqb_eq, Nat.leb_le.
  tauto.
Qed.

Lemma retention_key_length :
  forall identity,
    length (identity_digest identity) = 32 ->
    length (retention_key identity) = retention_key_len.
Proof.
  intros identity digest_length.
  unfold retention_key, ChannelImage.encode_u64_be.
  rewrite !length_app, !ChannelImage.encode_fixed_be_length, digest_length.
  reflexivity.
Qed.

Lemma decode_retention_key_encode :
  forall identity,
    identity_ok identity = true ->
    decode_retention_key (retention_key identity) = Some identity.
Proof.
  intros [generation digest encoded_len] ok.
  destruct (proj1 (identity_ok_true_iff _) ok)
    as (generation_bound & digest_length & digest_bytes & len_bound & len_lower).
  cbn [identity_generation identity_digest identity_encoded_len] in *.
  unfold decode_retention_key, retention_key,
    ChannelImage.decode_u64_be, ChannelImage.encode_u64_be.
  cbn [identity_generation identity_digest identity_encoded_len].
  rewrite (@ChannelImage.decode_encode_fixed_be 8 generation
             (digest ++ ChannelImage.encode_fixed_be 8 encoded_len) generation_bound).
  cbv beta iota.
  rewrite (take_exact_app_length digest (ChannelImage.encode_fixed_be 8 encoded_len)
             32 digest_length).
  cbv beta iota.
  rewrite <- (app_nil_r (ChannelImage.encode_fixed_be 8 encoded_len)).
  rewrite (@ChannelImage.decode_encode_fixed_be 8 encoded_len [] len_bound).
  cbv beta iota.
  apply Nat.leb_le in len_lower.
  rewrite digest_bytes, len_lower.
  reflexivity.
Qed.

Lemma decode_retention_key_some :
  forall input identity,
    decode_retention_key input = Some identity ->
    identity_ok identity = true /\ retention_key identity = input.
Proof.
  intros input identity.
  unfold decode_retention_key, ChannelImage.decode_u64_be.
  destruct (ChannelImage.decode_fixed_be 8 input)
    as [[generation after_generation]|] eqn:generation_decoded;
    cbv beta iota; [|intro impossible; discriminate impossible].
  destruct (take_exact 32 after_generation) as [[digest after_digest]|] eqn:digest_taken;
    cbv beta iota; [|intro impossible; discriminate impossible].
  destruct (ChannelImage.decode_fixed_be 8 after_digest)
    as [[encoded_len rest]|] eqn:len_decoded;
    cbv beta iota; [|intro impossible; discriminate impossible].
  destruct rest as [|extra rest]; cbv beta iota;
    [|intro impossible; discriminate impossible].
  destruct (ChannelImage.bytes_ok digest && Nat.leb object_envelope_header_len encoded_len)
    eqn:checks; cbv beta iota; [|intro impossible; discriminate impossible].
  intro decoded. injection decoded as identity_is. subst identity.
  apply andb_prop in checks as [digest_bytes len_lower].
  apply Nat.leb_le in len_lower.
  destruct (decode_fixed_be_some 8 input generation after_generation generation_decoded)
    as [generation_bound input_split].
  destruct (take_exact_some 32 after_generation digest after_digest digest_taken)
    as [digest_length after_generation_split].
  destruct (decode_fixed_be_some 8 after_digest encoded_len [] len_decoded)
    as [len_bound after_digest_split].
  split.
  - apply identity_ok_true_iff.
    cbn [identity_generation identity_digest identity_encoded_len].
    repeat split; assumption.
  - unfold retention_key, ChannelImage.encode_u64_be.
    cbn [identity_generation identity_digest identity_encoded_len].
    rewrite input_split, after_generation_split, after_digest_split, app_nil_r.
    reflexivity.
Qed.

(** Capstone 1: the key is a 48-byte bijection between valid identities and
    accepted key bytes. *)
Theorem retention_key_is_an_exact_48_byte_bijection :
  (forall identity,
      identity_ok identity = true ->
      length (retention_key identity) = retention_key_len /\
      decode_retention_key (retention_key identity) = Some identity) /\
  (forall input identity,
      decode_retention_key input = Some identity ->
      identity_ok identity = true /\ retention_key identity = input) /\
  (forall left right,
      identity_ok left = true ->
      identity_ok right = true ->
      retention_key left = retention_key right ->
      left = right).
Proof.
  split; [|split].
  - intros identity ok. split.
    + apply retention_key_length.
      destruct (proj1 (identity_ok_true_iff identity) ok) as (_ & digest_length & _).
      exact digest_length.
    + exact (decode_retention_key_encode identity ok).
  - exact decode_retention_key_some.
  - intros left right left_ok right_ok same_key.
    pose proof (decode_retention_key_encode left left_ok) as left_decoded.
    rewrite same_key, (decode_retention_key_encode right right_ok) in left_decoded.
    injection left_decoded as same. symmetry. exact same.
Qed.

(** Capstone 2: lexicographic key bytes order valid identities exactly as the
    V1 entry order does. *)
Theorem retention_key_order_is_entry_order :
  forall left right,
    identity_ok left = true ->
    identity_ok right = true ->
    ChannelImage.bytes_ltb (retention_key left) (retention_key right) =
      identity_ltb left right.
Proof.
  intros left right left_ok right_ok.
  destruct (proj1 (identity_ok_true_iff left) left_ok)
    as (left_generation & left_digest & _ & left_len & _).
  destruct (proj1 (identity_ok_true_iff right) right_ok)
    as (right_generation & right_digest & _ & right_len & _).
  unfold retention_key, identity_ltb, ChannelImage.encode_u64_be.
  rewrite bytes_ltb_app_same_length
    by (rewrite !ChannelImage.encode_fixed_be_length; reflexivity).
  rewrite (encode_fixed_be_order 8 _ _ left_generation right_generation).
  rewrite (encode_fixed_be_order 8 _ _ right_generation left_generation).
  rewrite bytes_ltb_app_same_length
    by (transitivity 32; [exact left_digest | symmetry; exact right_digest]).
  rewrite (encode_fixed_be_order 8 _ _ left_len right_len).
  reflexivity.
Qed.

Definition zero_digest : list nat := repeat 0 32.
Definition digest_one : list nat := repeat 0 31 ++ [1].

Definition little_endian_key_mutant (identity : retention_identity) : list nat :=
  rev (ChannelImage.encode_u64_be (identity_generation identity)) ++
  identity_digest identity ++
  ChannelImage.encode_u64_be (identity_encoded_len identity).

Definition early_identity : retention_identity := RetentionIdentity 1 zero_digest 50.
Definition late_identity : retention_identity := RetentionIdentity 256 zero_digest 50.

(** Capstone 3 (negative control): a little-endian generation sorts
    generation 256 before generation 1. *)
Theorem little_endian_key_mutant_inverts_generation_order :
  identity_ok early_identity = true /\
  identity_ok late_identity = true /\
  identity_ltb early_identity late_identity = true /\
  ChannelImage.bytes_ltb (retention_key early_identity) (retention_key late_identity) = true /\
  ChannelImage.bytes_ltb
    (little_endian_key_mutant late_identity)
    (little_endian_key_mutant early_identity) = true.
Proof. repeat split; vm_compute; reflexivity. Qed.

Definition generation_only_key_mutant (identity : retention_identity) : list nat :=
  ChannelImage.encode_u64_be (identity_generation identity).

Definition reference_only_key_mutant (identity : retention_identity) : list nat :=
  identity_digest identity ++ ChannelImage.encode_u64_be (identity_encoded_len identity).

Definition length_free_key_mutant (identity : retention_identity) : list nat :=
  ChannelImage.encode_u64_be (identity_generation identity) ++ identity_digest identity.

(** Generation alias: two references at one generation.  Reference alias: one
    reference at two generations.  Length alias: one generation and one digest
    with two encoded lengths.  V1 accepts each pair in strict entry order. *)
Definition generation_alias_left : retention_identity := RetentionIdentity 5 zero_digest 50.
Definition generation_alias_right : retention_identity := RetentionIdentity 5 digest_one 50.
Definition reference_alias_left : retention_identity := RetentionIdentity 4 zero_digest 50.
Definition reference_alias_right : retention_identity := RetentionIdentity 5 zero_digest 50.
Definition length_alias_left : retention_identity := RetentionIdentity 5 zero_digest 50.
Definition length_alias_right : retention_identity := RetentionIdentity 5 zero_digest 51.

Definition key_collapses
    (mutant : retention_identity -> list nat)
    (left right : retention_identity) : Prop :=
  identity_ok left = true /\
  identity_ok right = true /\
  identity_ltb left right = true /\
  mutant left = mutant right /\
  retention_key left <> retention_key right.

(** Capstone 4 (negative controls): each lossy key maps two distinct entries
    that V1 accepts to one key, while the exact keys stay distinct. *)
Theorem lossy_key_mutants_collapse_v1_aliases :
  key_collapses generation_only_key_mutant generation_alias_left generation_alias_right /\
  key_collapses reference_only_key_mutant reference_alias_left reference_alias_right /\
  key_collapses length_free_key_mutant length_alias_left length_alias_right.
Proof.
  unfold key_collapses.
  repeat split; try (vm_compute; reflexivity);
    intro equal; vm_compute in equal; discriminate equal.
Qed.

(** * Domains *)

Definition dag_node_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 68; 65; 71; 46; 78; 111; 100; 101; 0; 0].
Definition dag_blob_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 68; 65; 71; 46; 66; 108; 111; 98; 0; 0].
Definition blob_source_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 46; 66; 108; 111; 98; 46; 67; 83; 114; 99].
Definition semantic_manifest_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 68; 65; 71; 46; 83; 77; 97; 110; 0; 0].
Definition affine_manifest_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 68; 65; 71; 46; 65; 77; 97; 110; 0; 0].
Definition retention_manifest_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 68; 65; 71; 46; 82; 101; 116; 110; 0; 0].
Definition checkpoint_v1_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 68; 65; 71; 46; 67; 104; 107; 112; 116; 0].
Definition inventory_reference_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 73; 110; 118; 46; 82; 101; 102; 86; 49; 0].
Definition wal_prepare_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 87; 65; 76; 46; 80; 114; 101; 112; 0; 0].
Definition wal_commit_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 87; 65; 76; 46; 67; 111; 109; 109; 0; 0].
Definition mutation_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 68; 65; 71; 46; 77; 117; 116; 0; 0; 0].
Definition catalog_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 67; 97; 116; 97; 108; 111; 103; 86; 49; 0].
Definition index_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 73; 110; 100; 101; 120; 46; 86; 49; 0; 0].
Definition pack_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 80; 97; 99; 107; 46; 86; 49; 0; 0; 0].
Definition profile_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 46; 82; 117; 115; 116; 46; 86; 49; 0; 0].
Definition retention_anchor_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 68; 65; 71; 46; 82; 101; 116; 65; 0; 0].
Definition retention_radix_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 68; 65; 71; 46; 82; 101; 116; 82; 0; 0].
Definition checkpoint_v2_domain : list nat :=
  [82; 83; 112; 97; 99; 101; 68; 65; 71; 46; 67; 104; 107; 112; 50; 0].

Definition existing_domains : list (list nat) :=
  [dag_node_domain; dag_blob_domain; blob_source_domain; semantic_manifest_domain;
   affine_manifest_domain; retention_manifest_domain; checkpoint_v1_domain;
   inventory_reference_domain; wal_prepare_domain; wal_commit_domain; mutation_domain;
   catalog_domain; index_domain; pack_domain; profile_domain].

Definition metadata_domains : list (list nat) :=
  [retention_anchor_domain; retention_radix_domain].

Definition persistence_domains : list (list nat) :=
  existing_domains ++ metadata_domains ++ [checkpoint_v2_domain].

Fixpoint bytes_eqb (left right : list nat) : bool :=
  match left, right with
  | [], [] => true
  | left_head :: left_tail, right_head :: right_tail =>
      Nat.eqb left_head right_head && bytes_eqb left_tail right_tail
  | _, _ => false
  end.

Lemma bytes_eqb_true_iff :
  forall left right, bytes_eqb left right = true <-> left = right.
Proof.
  induction left as [|left_head left_tail IH];
    intros [|right_head right_tail]; cbn [bytes_eqb].
  - split; intros; reflexivity.
  - split; intro impossible; discriminate impossible.
  - split; intro impossible; discriminate impossible.
  - rewrite andb_true_iff, Nat.eqb_eq, IH. split.
    + intros [same_head same_tail]. subst. reflexivity.
    + intro same. injection same as same_head same_tail. split; assumption.
Qed.

Fixpoint pairwise_distinct (values : list (list nat)) : bool :=
  match values with
  | [] => true
  | head :: tail => negb (existsb (bytes_eqb head) tail) && pairwise_distinct tail
  end.

Lemma pairwise_distinct_nodup :
  forall values, pairwise_distinct values = true -> NoDup values.
Proof.
  induction values as [|head tail IH]; intro distinct.
  - constructor.
  - cbn [pairwise_distinct] in distinct.
    apply andb_prop in distinct as [head_fresh tail_distinct].
    constructor.
    + intro member. rewrite negb_true_iff in head_fresh.
      assert (found : existsb (bytes_eqb head) tail = true).
      { apply existsb_exists. exists head.
        split; [exact member | apply bytes_eqb_true_iff; reflexivity]. }
      rewrite found in head_fresh. discriminate head_fresh.
    + exact (IH tail_distinct).
Qed.

(** Capstone 5: the eighteen 16-byte domain constants are pairwise distinct. *)
Theorem retention_domains_are_pairwise_distinct :
  NoDup persistence_domains /\
  Forall (fun domain => length domain = 16) persistence_domains /\
  length persistence_domains = 18.
Proof.
  split; [|split].
  - apply pairwise_distinct_nodup. vm_compute. reflexivity.
  - apply Forall_forall. intros domain member.
    assert (check :
      forallb (fun candidate => Nat.eqb (length candidate) 16) persistence_domains = true)
      by (vm_compute; reflexivity).
    rewrite forallb_forall in check. apply Nat.eqb_eq. exact (check domain member).
  - vm_compute. reflexivity.
Qed.

Definition object_domain (canonical : list nat) : list nat := firstn 16 canonical.

Definition universes_disjoint : bool :=
  forallb (fun metadata => negb (existsb (bytes_eqb metadata) existing_domains))
    metadata_domains.

Lemma metadata_and_existing_domains_are_disjoint :
  forall domain, In domain metadata_domains -> In domain existing_domains -> False.
Proof.
  intros domain metadata_member existing_member.
  assert (check : universes_disjoint = true) by (vm_compute; reflexivity).
  unfold universes_disjoint in check. rewrite forallb_forall in check.
  specialize (check domain metadata_member). cbv beta in check.
  rewrite negb_true_iff in check.
  assert (found : existsb (bytes_eqb domain) existing_domains = true).
  { apply existsb_exists. exists domain.
    split; [exact existing_member | apply bytes_eqb_true_iff; reflexivity]. }
  rewrite found in check. discriminate check.
Qed.

(** Capstone 6: under collision-checked storage (equal digests imply equal
    canonical bytes), no metadata object shares a digest with an object of an
    existing domain. *)
Theorem domain_separation_excludes_cross_universe_digest_aliases :
  forall (digest : list nat -> nat) (metadata_object existing_object : list nat),
    (digest metadata_object = digest existing_object ->
      metadata_object = existing_object) ->
    In (object_domain metadata_object) metadata_domains ->
    In (object_domain existing_object) existing_domains ->
    digest metadata_object <> digest existing_object.
Proof.
  intros digest metadata_object existing_object collision_checked
    metadata_domain existing_domain same_digest.
  apply collision_checked in same_digest. subst existing_object.
  exact (metadata_and_existing_domains_are_disjoint _ metadata_domain existing_domain).
Qed.

Definition encode_u64_le (value : nat) : list nat := rev (ChannelImage.encode_u64_be value).

(** The canonical empty DAG node body: prefix length 0, absent endpoint, leaf
    branch kind and child count 0. *)
Definition empty_node_body : list nat := encode_u64_le 0 ++ [0] ++ [0] ++ encode_u64_le 0.

Definition object_envelope (domain body : list nat) : list nat :=
  domain ++ encode_u64_le 1 ++ encode_u64_le (length body) ++ body.

Definition empty_retention_radix_node : list nat :=
  object_envelope retention_radix_domain empty_node_body.
Definition empty_semantic_node : list nat :=
  object_envelope dag_node_domain empty_node_body.
Definition shared_domain_empty_radix_node_mutant : list nat :=
  object_envelope dag_node_domain empty_node_body.

(** Capstone 7 (negative control): radix nodes under the semantic node domain
    make the empty metadata root byte-identical to the empty semantic root, so
    the checked inventory merge refuses it as an existing digest. *)
Theorem shared_node_domain_mutant_aliases_empty_metadata_and_semantic_roots :
  length empty_retention_radix_node = 50 /\
  length empty_semantic_node = 50 /\
  empty_retention_radix_node <> empty_semantic_node /\
  shared_domain_empty_radix_node_mutant = empty_semantic_node /\
  (forall digest : list nat -> nat,
      insert_unique_by_key fst
        (digest shared_domain_empty_radix_node_mutant, 50)
        [(digest empty_semantic_node, 50)] = None).
Proof.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [intro equal; vm_compute in equal; discriminate equal|].
  split; [reflexivity|].
  intro digest.
  change shared_domain_empty_radix_node_mutant with empty_semantic_node.
  cbn [insert_unique_by_key fst].
  rewrite Nat.compare_refl. reflexivity.
Qed.

(** * Radix history *)

(** A retention radix node.  [radix_claim] is the [Multiplicity] endpoint,
    which holds the byte claim of the entry whose key ends at the node, and
    [radix_children] lists (edge byte, child object) pairs. *)
Record radix_node := RadixNode {
  radix_prefix : list nat;
  radix_claim : option nat;
  radix_children : list (nat * nat)
}.

Record radix_leaf := RadixLeaf {
  leaf_key : list nat;
  leaf_claim : nat;
  leaf_object : nat
}.

Fixpoint lookup_radix_node (store : list (nat * radix_node)) (object : nat)
    : option radix_node :=
  match store with
  | [] => None
  | (candidate, node) :: rest =>
      if Nat.eqb candidate object then Some node else lookup_radix_node rest object
  end.

Fixpoint edges_above (edge : nat) (children : list (nat * nat)) : bool :=
  match children with
  | [] => true
  | (next, _) :: rest => (edge <? next) && edges_above next rest
  end.

Definition edges_strictly_increasing (children : list (nat * nat)) : bool :=
  match children with
  | [] => true
  | (first, _) :: rest => edges_above first rest
  end.

Definition prepend_leaf (prefix : list nat) (leaf : radix_leaf) : radix_leaf :=
  RadixLeaf (prefix ++ leaf_key leaf) (leaf_claim leaf) (leaf_object leaf).

Fixpoint unfold_children_with
    (unfold_child : nat -> option (list radix_leaf))
    (children : list (nat * nat)) : option (list radix_leaf) :=
  match children with
  | [] => Some []
  | (edge, child) :: rest =>
      match unfold_child child, unfold_children_with unfold_child rest with
      | Some leaves, Some later => Some (map (prepend_leaf [edge]) leaves ++ later)
      | _, _ => None
      end
  end.

Fixpoint count_children_with
    (count_child : nat -> option nat)
    (children : list (nat * nat)) : option nat :=
  match children with
  | [] => Some 0
  | (_, child) :: rest =>
      match count_child child, count_children_with count_child rest with
      | Some here, Some later => Some (here + later)
      | _, _ => None
      end
  end.

(** [radix_unfold] enumerates the entries below a node whose keys still need
    [remaining] bytes.  An entry ends exactly at the full key width; an inner
    node carries no endpoint and at least two children with strictly
    increasing edges.  [radix_count] is the count recursion that the anchor
    stores and the validator recomputes. *)
Fixpoint radix_unfold
    (fuel : nat) (store : list (nat * radix_node)) (remaining object : nat)
    {struct fuel} : option (list radix_leaf) :=
  match fuel with
  | 0 => None
  | S smaller =>
      match lookup_radix_node store object with
      | None => None
      | Some node =>
          match Nat.compare (length (radix_prefix node)) remaining,
                radix_claim node, radix_children node with
          | Eq, Some claim, [] => Some [RadixLeaf (radix_prefix node) claim object]
          | Lt, None, children =>
              if edges_strictly_increasing children && (2 <=? length children) then
                match unfold_children_with
                        (radix_unfold smaller store
                           (remaining - S (length (radix_prefix node))))
                        children with
                | Some leaves => Some (map (prepend_leaf (radix_prefix node)) leaves)
                | None => None
                end
              else None
          | _, _, _ => None
          end
      end
  end.

Fixpoint radix_count
    (fuel : nat) (store : list (nat * radix_node)) (remaining object : nat)
    {struct fuel} : option nat :=
  match fuel with
  | 0 => None
  | S smaller =>
      match lookup_radix_node store object with
      | None => None
      | Some node =>
          match Nat.compare (length (radix_prefix node)) remaining,
                radix_claim node, radix_children node with
          | Eq, Some _, [] => Some 1
          | Lt, None, children =>
              if edges_strictly_increasing children && (2 <=? length children) then
                count_children_with
                  (radix_count smaller store (remaining - S (length (radix_prefix node))))
                  children
              else None
          | _, _, _ => None
          end
      end
  end.

Definition leaf_before (left right : radix_leaf) : Prop :=
  ChannelImage.bytes_ltb (leaf_key left) (leaf_key right) = true.

Lemma prepend_preserves_leaf_order :
  forall prefix left right,
    leaf_before left right ->
    leaf_before (prepend_leaf prefix left) (prepend_leaf prefix right).
Proof.
  intros prefix left right before. unfold leaf_before in *.
  unfold prepend_leaf. cbn [leaf_key].
  rewrite bytes_ltb_same_prefix. exact before.
Qed.

Lemma edges_above_lower_bound :
  forall children edge,
    edges_above edge children = true ->
    forall next child, In (next, child) children -> edge < next.
Proof.
  induction children as [|[candidate target] rest IH];
    intros edge above next child member.
  - destruct member.
  - cbn [edges_above] in above. apply andb_prop in above as [edge_below rest_above].
    apply Nat.ltb_lt in edge_below.
    destruct member as [same|later].
    + injection same as next_is child_is. subst next. exact edge_below.
    + pose proof (IH candidate rest_above next child later) as candidate_below. lia.
Qed.

Lemma edges_above_tail :
  forall children edge,
    edges_above edge children = true -> edges_strictly_increasing children = true.
Proof.
  intros [|[candidate target] rest] edge above.
  - reflexivity.
  - cbn [edges_above] in above. apply andb_prop in above as [_ rest_above].
    cbn [edges_strictly_increasing]. exact rest_above.
Qed.

Lemma unfold_children_with_spec :
  forall (unfold_child : nat -> option (list radix_leaf))
         (count_child : nat -> option nat) depth,
    (forall object leaves,
        unfold_child object = Some leaves ->
        count_child object = Some (length leaves) /\
        Forall (fun leaf => length (leaf_key leaf) = depth) leaves /\
        StronglySorted leaf_before leaves /\
        leaves <> []) ->
    forall children leaves,
      edges_strictly_increasing children = true ->
      unfold_children_with unfold_child children = Some leaves ->
      count_children_with count_child children = Some (length leaves) /\
      Forall (fun leaf => length (leaf_key leaf) = S depth) leaves /\
      StronglySorted leaf_before leaves /\
      (children <> [] -> leaves <> []) /\
      (forall leaf, In leaf leaves ->
        exists edge child suffix,
          In (edge, child) children /\ leaf_key leaf = edge :: suffix).
Proof.
  intros unfold_child count_child depth child_spec children.
  induction children as [|[edge child] rest IH]; intros leaves edges_ok.
  - cbn [unfold_children_with]. intro unfolded.
    injection unfolded as leaves_is. subst leaves.
    split; [reflexivity|]. split; [constructor|]. split; [constructor|].
    split; [intro absurd; exfalso; exact (absurd eq_refl)|].
    intros leaf member. destruct member.
  - cbn [edges_strictly_increasing] in edges_ok.
    pose proof (edges_above_tail rest edge edges_ok) as rest_ok.
    pose proof (edges_above_lower_bound rest edge edges_ok) as rest_above.
    cbn [unfold_children_with count_children_with].
    destruct (unfold_child child) as [here|] eqn:here_unfolded;
      cbv beta iota; [|intro impossible; discriminate impossible].
    destruct (unfold_children_with unfold_child rest) as [later|] eqn:later_unfolded;
      cbv beta iota; [|intro impossible; discriminate impossible].
    intro unfolded. injection unfolded as leaves_is. subst leaves.
    destruct (child_spec child here here_unfolded)
      as (here_count & here_lengths & here_sorted & here_nonempty).
    destruct (IH later rest_ok eq_refl)
      as (later_count & later_lengths & later_sorted & _ & later_edges).
    split.
    { rewrite here_count, later_count. cbv beta iota.
      rewrite length_app, length_map. reflexivity. }
    split.
    { apply Forall_app. split.
      - apply Forall_map. eapply Forall_impl; [|exact here_lengths].
        intros leaf leaf_length. cbv beta in leaf_length |- *.
        unfold prepend_leaf. cbn [leaf_key app length].
        rewrite leaf_length. reflexivity.
      - exact later_lengths. }
    split.
    { apply strongly_sorted_app.
      - apply (strongly_sorted_map leaf_before leaf_before (prepend_leaf [edge]));
          [|exact here_sorted].
        intros left right before. exact (prepend_preserves_leaf_order [edge] left right before).
      - exact later_sorted.
      - intros left right left_member right_member.
        apply in_map_iff in left_member as [left_source [left_is _]]. subst left.
        destruct (later_edges right right_member)
          as (next & next_child & suffix & next_member & right_key).
        pose proof (rest_above next next_child next_member) as edge_below.
        unfold leaf_before, prepend_leaf. cbn [leaf_key app]. rewrite right_key.
        exact (bytes_ltb_cons_lt edge next (leaf_key left_source) suffix edge_below). }
    split.
    { intros _ empty. apply app_eq_nil in empty as [here_empty _].
      apply map_eq_nil in here_empty. exact (here_nonempty here_empty). }
    { intros leaf member. apply in_app_or in member as [here_member|later_member].
      - apply in_map_iff in here_member as [source [leaf_is _]]. subst leaf.
        exists edge, child, (leaf_key source). split; [apply in_eq | reflexivity].
      - destruct (later_edges leaf later_member)
          as (next & next_child & suffix & next_member & leaf_key_is).
        exists next, next_child, suffix.
        split; [apply in_cons; exact next_member | exact leaf_key_is]. }
Qed.

(** Capstone 8: whenever the radix unfolds, its count is the number of entry
    occurrences, every key has the full remaining width, the keys strictly
    increase, and a non-empty radix node denotes at least one entry (so a zero
    count belongs to the empty root alone). *)
Theorem radix_count_denotes_entry_occurrences :
  forall fuel store remaining object leaves,
    radix_unfold fuel store remaining object = Some leaves ->
    radix_count fuel store remaining object = Some (length leaves) /\
    Forall (fun leaf => length (leaf_key leaf) = remaining) leaves /\
    StronglySorted leaf_before leaves /\
    leaves <> [].
Proof.
  induction fuel as [|smaller IH]; intros store remaining object leaves.
  - intro impossible. discriminate impossible.
  - cbn [radix_unfold radix_count].
    destruct (lookup_radix_node store object) as [[prefix claim children]|];
      cbv beta iota; [|intro impossible; discriminate impossible].
    cbn [radix_prefix radix_claim radix_children].
    destruct (Nat.compare_spec (length prefix) remaining)
      as [at_depth|above_depth|below_depth]; cbv beta iota.
    + destruct claim as [claim|]; destruct children as [|first rest]; cbv beta iota;
        try (intro impossible; discriminate impossible).
      intro unfolded. injection unfolded as leaves_is. subst leaves.
      split; [reflexivity|].
      split; [constructor; [exact at_depth|constructor]|].
      split; [constructor; constructor|].
      discriminate.
    + destruct claim as [claim|]; cbv beta iota; [intro impossible; discriminate impossible|].
      destruct (edges_strictly_increasing children && (2 <=? length children)) eqn:shape;
        cbv beta iota; [|intro impossible; discriminate impossible].
      apply andb_prop in shape as [edges_ok wide]. apply Nat.leb_le in wide.
      destruct (unfold_children_with
                  (radix_unfold smaller store (remaining - S (length prefix))) children)
        as [blocks|] eqn:blocks_unfolded;
        cbv beta iota; [|intro impossible; discriminate impossible].
      intro unfolded. injection unfolded as leaves_is. subst leaves.
      destruct (unfold_children_with_spec
                  (radix_unfold smaller store (remaining - S (length prefix)))
                  (radix_count smaller store (remaining - S (length prefix)))
                  (remaining - S (length prefix))
                  (IH store (remaining - S (length prefix)))
                  children blocks edges_ok blocks_unfolded)
        as (blocks_count & blocks_lengths & blocks_sorted & blocks_nonempty & _).
      split; [rewrite blocks_count, length_map; reflexivity|].
      split.
      { apply Forall_map. eapply Forall_impl; [|exact blocks_lengths].
        intros leaf leaf_length. cbv beta in leaf_length |- *.
        unfold prepend_leaf. cbn [leaf_key].
        rewrite length_app, leaf_length. lia. }
      split.
      { apply (strongly_sorted_map leaf_before leaf_before (prepend_leaf prefix));
          [|exact blocks_sorted].
        intros left right before. exact (prepend_preserves_leaf_order prefix left right before). }
      { intro empty. apply map_eq_nil in empty. subst blocks.
        apply blocks_nonempty; [|reflexivity].
        intro no_children. subst children. cbn in wide. lia. }
    + destruct claim as [claim|]; destruct children as [|first rest]; cbv beta iota;
        intro impossible; discriminate impossible.
Qed.

Fixpoint distinct_naturals (values : list nat) : list nat :=
  match values with
  | [] => []
  | head :: tail =>
      if existsb (Nat.eqb head) tail then distinct_naturals tail
      else head :: distinct_naturals tail
  end.

Definition distinct_leaf_count_mutant (leaves : list radix_leaf) : nat :=
  length (distinct_naturals (map leaf_object leaves)).

(** One reference retained at generations 0 and 1: both entries share the
    leaf object that holds the 40-byte suffix [digest ++ BE64(50)]. *)
Definition shared_leaf_store : list (nat * radix_node) :=
  [(1, RadixNode (repeat 0 7) None [(0, 2); (1, 2)]);
   (2, RadixNode (zero_digest ++ ChannelImage.encode_u64_be 50) (Some 50) [])].

(** Capstone 9 (negative control): counting distinct leaf objects reports one
    entry for two retained reference aliases that share their leaf. *)
Theorem distinct_leaf_count_mutant_undercounts_shared_entries :
  radix_count 3 shared_leaf_store retention_key_len 1 = Some 2 /\
  option_map (@length radix_leaf) (radix_unfold 3 shared_leaf_store retention_key_len 1) =
    Some 2 /\
  option_map distinct_leaf_count_mutant
    (radix_unfold 3 shared_leaf_store retention_key_len 1) = Some 1 /\
  option_map (map (fun leaf => decode_retention_key (leaf_key leaf)))
    (radix_unfold 3 shared_leaf_store retention_key_len 1) =
    Some [Some (RetentionIdentity 0 zero_digest 50); Some (RetentionIdentity 1 zero_digest 50)].
Proof. repeat split; vm_compute; reflexivity. Qed.

(** * Checkpoint inventory *)

(** An inventory object is its (digest, canonical byte length) pair. *)
Definition inventory_object : Type := (nat * nat)%type.

Fixpoint merge_all (metadata retained : list inventory_object)
    : option (list inventory_object) :=
  match metadata with
  | [] => Some retained
  | object :: rest =>
      match merge_all rest retained with
      | Some merged => insert_unique_by_key fst object merged
      | None => None
      end
  end.

Definition v1_inventory_rule
    (retained_objects retained_bytes retention_len data_objects data_bytes : nat) : bool :=
  Nat.eqb data_objects (S retained_objects) &&
  Nat.eqb data_bytes (retained_bytes + retention_len).

Definition v2_inventory_rule
    (retained_objects retained_bytes metadata_objects metadata_bytes
     data_objects data_bytes : nat) : bool :=
  Nat.eqb data_objects (retained_objects + metadata_objects) &&
  Nat.eqb data_bytes (retained_bytes + metadata_bytes).

Lemma canonical_byte_sum_app {Object : Type} :
  forall (bytes : Object -> nat) left right,
    canonical_byte_sum bytes (left ++ right) =
      canonical_byte_sum bytes left + canonical_byte_sum bytes right.
Proof.
  intros bytes left right.
  induction left as [|head tail IH]; cbn [app canonical_byte_sum].
  - reflexivity.
  - rewrite IH. lia.
Qed.

Lemma insert_unique_by_key_none_alias {Object : Type} (object_key : Object -> nat) :
  forall item retained,
    insert_unique_by_key object_key item retained = None ->
    exists candidate, In candidate retained /\ object_key candidate = object_key item.
Proof.
  intros item retained. induction retained as [|head tail IH]; cbn [insert_unique_by_key].
  - intro refused. discriminate refused.
  - destruct (Nat.compare_spec (object_key item) (object_key head)) as [same|before|after];
      cbv beta iota.
    + intros _. exists head. split; [apply in_eq | symmetry; exact same].
    + intro refused. discriminate refused.
    + destruct (insert_unique_by_key object_key item tail) as [merged|] eqn:tail_insert;
        cbv beta iota.
      * intro refused. discriminate refused.
      * intros _. destruct (IH eq_refl) as [candidate [candidate_member candidate_key]].
        exists candidate. split; [apply in_cons; exact candidate_member | exact candidate_key].
Qed.

Lemma merge_all_some :
  forall metadata retained merged,
    StronglySorted (key_lt fst) retained ->
    merge_all metadata retained = Some merged ->
    Permutation merged (metadata ++ retained) /\ StronglySorted (key_lt fst) merged.
Proof.
  induction metadata as [|object rest IH]; intros retained merged retained_sorted.
  - cbn [merge_all]. intro merged_all.
    injection merged_all as merged_is. subst merged.
    split; [apply Permutation_refl | exact retained_sorted].
  - cbn [merge_all].
    destruct (merge_all rest retained) as [partial|] eqn:partial_merged;
      cbv beta iota; [|intro impossible; discriminate impossible].
    intro merged_all.
    destruct (IH retained partial retained_sorted partial_merged)
      as [partial_permutation partial_sorted].
    split.
    + apply Permutation_trans with (object :: partial).
      * exact (successful_merge_is_exact_permutation fst object partial merged_all).
      * apply perm_skip. exact partial_permutation.
    + exact (successful_merge_preserves_strict_digest_order object partial_sorted merged_all).
Qed.

Lemma merge_all_complete :
  forall metadata retained,
    StronglySorted (key_lt fst) retained ->
    NoDup (map fst (metadata ++ retained)) ->
    exists merged, merge_all metadata retained = Some merged.
Proof.
  induction metadata as [|object rest IH]; intros retained retained_sorted distinct.
  - exists retained. reflexivity.
  - cbn [app map] in distinct.
    apply NoDup_cons_iff in distinct as [object_fresh rest_distinct].
    destruct (IH retained retained_sorted rest_distinct) as [partial partial_merged].
    destruct (merge_all_some rest retained partial retained_sorted partial_merged)
      as [partial_permutation _].
    cbn [merge_all]. rewrite partial_merged. cbv beta iota.
    destruct (insert_unique_by_key fst object partial) as [merged|] eqn:inserted.
    + exists merged. reflexivity.
    + exfalso. apply object_fresh.
      destruct (insert_unique_by_key_none_alias fst object partial inserted)
        as [candidate [candidate_member candidate_key]].
      rewrite <- candidate_key. apply in_map.
      eapply Permutation_in; [exact partial_permutation | exact candidate_member].
Qed.

(** Capstone 10: a successful checked merge of the metadata closure into the
    retained union is an exact, strictly ordered disjoint union that meets the
    V2 inventory rule, and the merge succeeds whenever the digests are
    disjoint. *)
Theorem v2_checkpoint_inventory_is_retained_plus_metadata :
  (forall metadata retained merged : list inventory_object,
      StronglySorted (key_lt fst) retained ->
      merge_all metadata retained = Some merged ->
      Permutation merged (metadata ++ retained) /\
      StronglySorted (key_lt fst) merged /\
      v2_inventory_rule
        (length retained) (canonical_byte_sum snd retained)
        (length metadata) (canonical_byte_sum snd metadata)
        (length merged) (canonical_byte_sum snd merged) = true) /\
  (forall metadata retained : list inventory_object,
      StronglySorted (key_lt fst) retained ->
      NoDup (map fst (metadata ++ retained)) ->
      exists merged, merge_all metadata retained = Some merged).
Proof.
  split.
  - intros metadata retained merged retained_sorted merged_all.
    destruct (merge_all_some metadata retained merged retained_sorted merged_all)
      as [permutation sorted].
    split; [exact permutation|]. split; [exact sorted|].
    unfold v2_inventory_rule. apply andb_true_iff. split; apply Nat.eqb_eq.
    + rewrite (Permutation_length permutation), length_app. apply Nat.add_comm.
    + rewrite (canonical_byte_sum_permutation snd permutation), canonical_byte_sum_app.
      apply Nat.add_comm.
  - exact merge_all_complete.
Qed.

(** Capstone 11 (negative control): the V1 rule [data = retained + 1] refuses
    every V2 inventory, whose metadata closure holds at least the anchor and
    the radix root. *)
Theorem plus_one_rule_mutant_rejects_every_v2_inventory :
  forall retained_objects retained_bytes metadata_objects metadata_bytes
         data_objects data_bytes retention_len,
    2 <= metadata_objects ->
    v2_inventory_rule retained_objects retained_bytes metadata_objects metadata_bytes
      data_objects data_bytes = true ->
    v1_inventory_rule retained_objects retained_bytes retention_len
      data_objects data_bytes = false.
Proof.
  intros retained_objects retained_bytes metadata_objects metadata_bytes
    data_objects data_bytes retention_len at_least_two v2_rule.
  unfold v2_inventory_rule in v2_rule.
  apply andb_prop in v2_rule as [objects_rule _].
  apply Nat.eqb_eq in objects_rule.
  unfold v1_inventory_rule. apply andb_false_intro1. apply Nat.eqb_neq. lia.
Qed.

Definition reference_closed (members : list nat) (references : nat -> list nat) : Prop :=
  forall member, In member members ->
  forall target, In target (references member) -> In target members.

Definition reference_closedb (members : list nat) (references : nat -> list nat) : bool :=
  forallb
    (fun member => forallb (fun target => existsb (Nat.eqb target) members) (references member))
    members.

Lemma reference_closedb_true_iff :
  forall members references,
    reference_closedb members references = true <-> reference_closed members references.
Proof.
  intros members references. unfold reference_closedb, reference_closed. split.
  - intros check member member_in target target_in.
    rewrite forallb_forall in check. specialize (check member member_in). cbv beta in check.
    rewrite forallb_forall in check. specialize (check target target_in). cbv beta in check.
    apply existsb_exists in check as [found [found_in found_equal]].
    apply Nat.eqb_eq in found_equal. subst found. exact found_in.
  - intro closed. apply forallb_forall. intros member member_in.
    apply forallb_forall. intros target target_in.
    apply existsb_exists. exists target.
    split; [exact (closed member member_in target target_in) | apply Nat.eqb_refl].
Qed.

(** Objects 10 and 11 are the current and historical state manifests, 1 is
    the anchor (referencing the current manifest and the radix root), and 2 is
    the radix root, a leaf whose key names the historical manifest. *)
Definition example_references (object : nat) : list nat :=
  match object with
  | 1 => [10; 2]
  | 2 => [11]
  | _ => []
  end.

Definition faithful_checkpoint_members : list nat := [10; 11; 1; 2].
Definition anchor_only_checkpoint_members : list nat := [10; 11; 1].

(** Capstone 12 (negative control): inventorying the anchor without its
    reachable radix nodes, as the V1 "+1" rule would, is not reference-closed. *)
Theorem anchor_only_checkpoint_mutant_is_not_reference_closed :
  reference_closed faithful_checkpoint_members example_references /\
  ~ reference_closed anchor_only_checkpoint_members example_references.
Proof.
  split.
  - apply reference_closedb_true_iff. vm_compute. reflexivity.
  - intro closed.
    pose proof (proj2 (reference_closedb_true_iff _ _) closed) as check.
    vm_compute in check. discriminate check.
Qed.

(** * One-time V1 migration *)

Record retained_state := RetainedState {
  state_identity : retention_identity;
  state_claim : nat
}.

Definition retained_state_ok (state : retained_state) : bool :=
  identity_ok (state_identity state) &&
  ChannelImage.fits_fixed_be 8 (state_claim state) &&
  Nat.leb (identity_encoded_len (state_identity state)) (state_claim state).

Definition state_lt (left right : retained_state) : Prop :=
  identity_ltb (state_identity left) (state_identity right) = true.

Record retention_v1 := RetentionV1 {
  v1_current : retained_state;
  v1_historical : list retained_state
}.

(** The V1 manifest parser: valid entries, historical entries strictly
    increasing pairwise in entry order, and no historical state equal to the
    current state. *)
Definition retention_v1_valid (manifest : retention_v1) : Prop :=
  retained_state_ok (v1_current manifest) = true /\
  Forall (fun state => retained_state_ok state = true) (v1_historical manifest) /\
  Sorted state_lt (v1_historical manifest) /\
  Forall (fun state => state_identity state <> state_identity (v1_current manifest))
    (v1_historical manifest).

(** The V2 denotation: the verbatim current entry, the anchor count and the
    history as (key, byte claim) pairs in radix order. *)
Record retention_v2 := RetentionV2 {
  v2_current : retained_state;
  v2_historical_count : nat;
  v2_history : list (list nat * nat)
}.

Definition history_entry (state : retained_state) : list nat * nat :=
  (retention_key (state_identity state), state_claim state).

Definition migrate_v1 (manifest : retention_v1) : retention_v2 :=
  RetentionV2
    (v1_current manifest)
    (length (v1_historical manifest))
    (map history_entry (v1_historical manifest)).

Definition decode_history_entry (entry : list nat * nat) : option retained_state :=
  match decode_retention_key (fst entry) with
  | Some identity => Some (RetainedState identity (snd entry))
  | None => None
  end.

Fixpoint decode_history (entries : list (list nat * nat)) : option (list retained_state) :=
  match entries with
  | [] => Some []
  | entry :: rest =>
      match decode_history_entry entry, decode_history rest with
      | Some state, Some states => Some (state :: states)
      | _, _ => None
      end
  end.

Definition read_v2 (anchor : retention_v2) : option retention_v1 :=
  if Nat.eqb (v2_historical_count anchor) (length (v2_history anchor)) then
    match decode_history (v2_history anchor) with
    | Some historical => Some (RetentionV1 (v2_current anchor) historical)
    | None => None
    end
  else None.

Definition composition_entry
    (manifest_id : list nat -> nat -> nat) (state : retained_state)
    : CheckedDomainComposition.retention_entry :=
  CheckedDomainComposition.RetentionEntry
    (identity_generation (state_identity state))
    (manifest_id (identity_digest (state_identity state))
                 (identity_encoded_len (state_identity state))).

Definition composition_entries
    (manifest_id : list nat -> nat -> nat) (manifest : retention_v1)
    : CheckedDomainComposition.retention_entries :=
  CheckedDomainComposition.RetentionEntries
    (composition_entry manifest_id (v1_current manifest))
    (map (composition_entry manifest_id) (v1_historical manifest)).

Lemma retained_state_ok_identity :
  forall state,
    retained_state_ok state = true -> identity_ok (state_identity state) = true.
Proof.
  intros state ok. unfold retained_state_ok in ok.
  apply andb_prop in ok as [ok _]. apply andb_prop in ok as [ok _]. exact ok.
Qed.

Lemma decode_history_of_states :
  forall states,
    Forall (fun state => retained_state_ok state = true) states ->
    decode_history (map history_entry states) = Some states.
Proof.
  induction states as [|[identity claim] rest IH]; intro all_ok.
  - reflexivity.
  - cbn [map decode_history].
    rewrite (IH (Forall_inv_tail all_ok)).
    unfold decode_history_entry, history_entry.
    cbn [fst snd state_identity state_claim].
    rewrite (decode_retention_key_encode identity
               (retained_state_ok_identity (RetainedState identity claim) (Forall_inv all_ok))).
    reflexivity.
Qed.

(** Capstone 13: reading the migrated anchor returns the V1 manifest exactly
    (entries and aliases), the anchor count is the history length, the history
    keys strictly increase, and the current and retained root counters are
    unchanged for every manifest numbering. *)
Theorem v1_migration_preserves_entries_order_count_and_roots :
  forall manifest,
    retention_v1_valid manifest ->
    read_v2 (migrate_v1 manifest) = Some manifest /\
    v2_historical_count (migrate_v1 manifest) = length (v2_history (migrate_v1 manifest)) /\
    StronglySorted
      (fun left right => ChannelImage.bytes_ltb (fst left) (fst right) = true)
      (v2_history (migrate_v1 manifest)) /\
    (forall restored manifest_id,
        read_v2 (migrate_v1 manifest) = Some restored ->
        CheckedDomainComposition.current_roots (composition_entries manifest_id restored) =
          CheckedDomainComposition.current_roots (composition_entries manifest_id manifest) /\
        CheckedDomainComposition.retained_roots (composition_entries manifest_id restored) =
          CheckedDomainComposition.retained_roots (composition_entries manifest_id manifest)).
Proof.
  intros [current historical] (current_ok & historical_ok & historical_sorted & _).
  cbn [v1_current v1_historical] in *.
  assert (read_back :
    read_v2 (migrate_v1 (RetentionV1 current historical)) =
      Some (RetentionV1 current historical)).
  { unfold read_v2, migrate_v1.
    cbn [v2_historical_count v2_history v2_current v1_current v1_historical].
    rewrite length_map, Nat.eqb_refl. cbv beta iota.
    rewrite (decode_history_of_states historical historical_ok). reflexivity. }
  split; [exact read_back|].
  split.
  { unfold migrate_v1. cbn [v2_historical_count v2_history v1_historical].
    rewrite length_map. reflexivity. }
  split.
  - unfold migrate_v1. cbn [v2_history v1_historical].
    apply Sorted_StronglySorted.
    + intros x y z x_y y_z. exact (bytes_ltb_transitive _ _ _ x_y y_z).
    + apply (sorted_map_on (fun state => retained_state_ok state = true) state_lt);
        [|exact historical_ok|exact historical_sorted].
      intros left right left_ok right_ok before.
      cbv beta in left_ok, right_ok |- *.
      unfold history_entry. cbn [fst].
      rewrite retention_key_order_is_entry_order
        by (apply retained_state_ok_identity; assumption).
      exact before.
  - intros restored manifest_id restored_read.
    rewrite read_back in restored_read.
    injection restored_read as restored_is. subst restored.
    split; reflexivity.
Qed.

(** * Transfer-session binding *)

Inductive retention_format : Type :=
| RetentionFormatV1
| RetentionFormatV2.

Definition appointed_retention_domain (format : retention_format) : list nat :=
  match format with
  | RetentionFormatV1 => retention_manifest_domain
  | RetentionFormatV2 => retention_anchor_domain
  end.

Definition appointed_descriptor_domain (format : retention_format) : list nat :=
  match format with
  | RetentionFormatV1 => checkpoint_v1_domain
  | RetentionFormatV2 => checkpoint_v2_domain
  end.

(** A durable representation of a state: the state version (generation and
    semantic manifest reference), the appointed retention format and the
    reference of the appointed retention object. *)
Record durable_representation := DurableRepresentation {
  represented_state : retention_identity;
  represented_format : retention_format;
  represented_retention_reference : list nat * nat
}.

Record session_binding := SessionBinding {
  bound_state : retention_identity;
  bound_retention_reference : list nat * nat;
  bound_retention_domain : list nat;
  bound_descriptor_domain : list nat
}.

Definition bind_transfer_session (representation : durable_representation) : session_binding :=
  SessionBinding
    (represented_state representation)
    (represented_retention_reference representation)
    (appointed_retention_domain (represented_format representation))
    (appointed_descriptor_domain (represented_format representation)).

Definition resume_admitted (token session : durable_representation) : Prop :=
  bind_transfer_session token = bind_transfer_session session.

(** Capstone 14: a resume token of one format never resumes a session of the
    other format, even for the same state version and retention reference. *)
Theorem retention_format_binding_separates_representations_of_one_state :
  forall state v1_reference v2_reference,
    ~ resume_admitted
        (DurableRepresentation state RetentionFormatV1 v1_reference)
        (DurableRepresentation state RetentionFormatV2 v2_reference) /\
    ~ resume_admitted
        (DurableRepresentation state RetentionFormatV2 v2_reference)
        (DurableRepresentation state RetentionFormatV1 v1_reference).
Proof.
  intros state v1_reference v2_reference.
  split; intro admitted; apply (f_equal bound_retention_domain) in admitted;
    vm_compute in admitted; discriminate admitted.
Qed.

Definition state_version_only_binding (representation : durable_representation)
    : retention_identity :=
  represented_state representation.

(** Capstone 15 (negative control): a binding over the state version alone
    admits a V1 token for a V2 session of the same state. *)
Theorem state_version_only_binding_mutant_resumes_across_formats :
  forall state v1_reference v2_reference,
    state_version_only_binding (DurableRepresentation state RetentionFormatV1 v1_reference) =
    state_version_only_binding (DurableRepresentation state RetentionFormatV2 v2_reference).
Proof. reflexivity. Qed.

(** * Cold reader *)

(** The cold reader admits a V2 history in two phases.  Phase one validates
    the radix and counts its entry occurrences, reading each object at most
    once and memoizing the count of an object at one remaining key width.
    Only after the count equals the anchor's declared count does phase two
    reserve exactly that many entries and enumerate them without further
    reads.  This section proves that the count authorizes the enumeration,
    that the memo must be keyed by the remaining width, and that count and
    inclusion checks authenticate the disjoint union of the retained objects
    and the metadata closure. *)

Lemma count_children_with_ext :
  forall (left right : nat -> option nat) children,
    (forall edge child, In (edge, child) children -> left child = right child) ->
    count_children_with left children = count_children_with right children.
Proof.
  intros left right children.
  induction children as [|[edge child] rest IH]; intro same.
  - reflexivity.
  - cbn [count_children_with].
    rewrite (same edge child (in_eq _ _)).
    rewrite IH; [reflexivity|].
    intros next target member. exact (same next target (in_cons _ _ _ member)).
Qed.

(** Fuel beyond the remaining width never changes a count: every inner step
    strictly shortens the remaining width, so [S remaining] steps reach every
    endpoint the count can reach. *)
Lemma remaining_width_is_sufficient_fuel :
  forall fuel other store remaining object,
    S remaining <= fuel ->
    S remaining <= other ->
    radix_count fuel store remaining object = radix_count other store remaining object.
Proof.
  induction fuel as [|smaller IH]; intros other store remaining object enough other_enough.
  - lia.
  - destruct other as [|other_smaller]; [lia|].
    cbn [radix_count].
    destruct (lookup_radix_node store object) as [[prefix claim children]|];
      [|reflexivity].
    cbn [radix_prefix radix_claim radix_children].
    destruct (Nat.compare_spec (length prefix) remaining) as [at_depth|below|above];
      [reflexivity| |reflexivity].
    destruct claim as [claim|]; [reflexivity|].
    cbv beta iota.
    destruct (edges_strictly_increasing children && (2 <=? length children));
      [|reflexivity].
    apply count_children_with_ext. intros edge child _.
    apply IH; lia.
Qed.

Lemma count_children_with_unfolds :
  forall (count_child : nat -> option nat) (unfold_child : nat -> option (list radix_leaf)),
    (forall object count,
        count_child object = Some count ->
        exists leaves, unfold_child object = Some leaves /\ length leaves = count) ->
    forall children count,
      count_children_with count_child children = Some count ->
      exists leaves,
        unfold_children_with unfold_child children = Some leaves /\ length leaves = count.
Proof.
  intros count_child unfold_child child_unfolds children.
  induction children as [|[edge child] rest IH]; intros count counted.
  - cbn [count_children_with] in counted. injection counted as count_is. subst count.
    exists []. split; reflexivity.
  - cbn [count_children_with] in counted.
    destruct (count_child child) as [here|] eqn:here_counted;
      cbv beta iota in counted; [|discriminate counted].
    destruct (count_children_with count_child rest) as [later|] eqn:later_counted;
      cbv beta iota in counted; [|discriminate counted].
    injection counted as count_is. subst count.
    destruct (child_unfolds child here here_counted) as [here_leaves [here_unfolded here_length]].
    destruct (IH later eq_refl) as [later_leaves [later_unfolded later_length]].
    exists (map (prepend_leaf [edge]) here_leaves ++ later_leaves).
    cbn [unfold_children_with]. rewrite here_unfolded, later_unfolded.
    split; [reflexivity|]. rewrite length_app, length_map. lia.
Qed.

(** Capstone 16: a successful count guarantees that the enumeration succeeds
    with exactly that many entries, so a reader that reserves the declared
    count after the count check never reallocates and never enumerates an
    unauthenticated radix. *)
Theorem radix_count_success_unfolds_with_that_many_leaves :
  forall fuel store remaining object count,
    radix_count fuel store remaining object = Some count ->
    exists leaves,
      radix_unfold fuel store remaining object = Some leaves /\ length leaves = count.
Proof.
  induction fuel as [|smaller IH]; intros store remaining object count counted.
  - discriminate counted.
  - cbn [radix_count] in counted. cbn [radix_unfold].
    destruct (lookup_radix_node store object) as [[prefix claim children]|];
      cbv beta iota in counted |- *; [|discriminate counted].
    cbn [radix_prefix radix_claim radix_children] in counted |- *.
    destruct (Nat.compare (length prefix) remaining); cbv beta iota in counted |- *.
    + destruct claim as [claim|]; destruct children as [|first rest];
        cbv beta iota in counted |- *; try discriminate counted.
      injection counted as count_is. subst count.
      eexists. split; reflexivity.
    + destruct claim as [claim|]; cbv beta iota in counted |- *; [discriminate counted|].
      destruct (edges_strictly_increasing children && (2 <=? length children));
        cbv beta iota in counted |- *; [|discriminate counted].
      destruct (count_children_with_unfolds
                  (radix_count smaller store (remaining - S (length prefix)))
                  (radix_unfold smaller store (remaining - S (length prefix)))
                  (IH store (remaining - S (length prefix)))
                  children count counted)
        as [leaves [unfolded leaves_length]].
      rewrite unfolded. exists (map (prepend_leaf prefix) leaves).
      split; [reflexivity|]. rewrite length_map. exact leaves_length.
    + destruct claim as [claim|]; destruct children as [|first rest];
        cbv beta iota in counted; discriminate counted.
Qed.

(** The shape-free mutant drops the inner-node shape check (strictly
    increasing edges and at least two children), so an empty node below the
    root counts as zero entries instead of refusing. *)
Fixpoint shape_free_count_mutant
    (fuel : nat) (store : list (nat * radix_node)) (remaining object : nat)
    {struct fuel} : option nat :=
  match fuel with
  | 0 => None
  | S smaller =>
      match lookup_radix_node store object with
      | None => None
      | Some node =>
          match Nat.compare (length (radix_prefix node)) remaining,
                radix_claim node, radix_children node with
          | Eq, Some _, [] => Some 1
          | Lt, None, children =>
              count_children_with
                (shape_free_count_mutant smaller store
                   (remaining - S (length (radix_prefix node))))
                children
          | _, _, _ => None
          end
      end
  end.

(** A root whose children are an empty node and a leaf with the 47-byte
    suffix of a full key. *)
Definition empty_child_store : list (nat * radix_node) :=
  [(1, RadixNode [] None [(0, 2); (1, 3)]);
   (2, RadixNode [] None []);
   (3, RadixNode (repeat 0 47) (Some 50) [])].

(** Capstone 17 (negative control): the shape-free count accepts a radix that
    neither the count nor the enumeration accepts, so it would authorize an
    enumeration that fails (it violates capstone 16). *)
Theorem shape_free_count_mutant_counts_an_empty_node_below_the_root :
  radix_count 49 empty_child_store retention_key_len 1 = None /\
  radix_unfold 49 empty_child_store retention_key_len 1 = None /\
  shape_free_count_mutant 49 empty_child_store retention_key_len 1 = Some 1.
Proof. repeat split; vm_compute; reflexivity. Qed.

(** A memo entry ((object, remaining), count) records the count of one object
    at one remaining key width. *)
Definition count_memo : Type := list ((nat * nat) * nat).

Fixpoint memo_lookup (memo : count_memo) (object remaining : nat) : option nat :=
  match memo with
  | [] => None
  | ((candidate, width), count) :: rest =>
      if Nat.eqb candidate object && Nat.eqb width remaining then Some count
      else memo_lookup rest object remaining
  end.

(** [memo_count_children_with] counts the children from left to right and
    threads the memo through them. *)
Fixpoint memo_count_children_with {Memo : Type}
    (count_child : Memo -> nat -> option (nat * Memo))
    (memo : Memo) (children : list (nat * nat)) : option (nat * Memo) :=
  match children with
  | [] => Some (0, memo)
  | (_, child) :: rest =>
      match count_child memo child with
      | None => None
      | Some (here, after_child) =>
          match memo_count_children_with count_child after_child rest with
          | None => None
          | Some (later, after_rest) => Some (here + later, after_rest)
          end
      end
  end.

Definition record_count (object remaining : nat) (computed : option (nat * count_memo))
    : option (nat * count_memo) :=
  match computed with
  | Some (count, after) => Some (count, ((object, remaining), count) :: after)
  | None => None
  end.

(** [memoized_radix_count] is the phase-one count of the cold reader: a memo
    hit at the same object and remaining width returns the recorded count;
    a miss computes the count as [radix_count] does and records it. *)
Fixpoint memoized_radix_count
    (fuel : nat) (store : list (nat * radix_node)) (memo : count_memo)
    (remaining object : nat) {struct fuel} : option (nat * count_memo) :=
  match fuel with
  | 0 => None
  | S smaller =>
      match memo_lookup memo object remaining with
      | Some count => Some (count, memo)
      | None =>
          record_count object remaining
            match lookup_radix_node store object with
            | None => None
            | Some node =>
                match Nat.compare (length (radix_prefix node)) remaining,
                      radix_claim node, radix_children node with
                | Eq, Some _, [] => Some (1, memo)
                | Lt, None, children =>
                    if edges_strictly_increasing children && (2 <=? length children) then
                      memo_count_children_with
                        (fun current child =>
                           memoized_radix_count smaller store current
                             (remaining - S (length (radix_prefix node))) child)
                        memo children
                    else None
                | _, _, _ => None
                end
            end
      end
  end.

(** A memo is sound when every recorded count is the count of its object at
    its remaining width. *)
Definition memo_sound (store : list (nat * radix_node)) (memo : count_memo) : Prop :=
  forall object remaining count,
    memo_lookup memo object remaining = Some count ->
    radix_count (S remaining) store remaining object = Some count.

(** A memoized result agrees with an expected count when both refuse, or both
    succeed with the same count and the returned memo is sound. *)
Definition agrees_with_count
    (store : list (nat * radix_node)) (expected : option nat)
    (result : option (nat * count_memo)) : Prop :=
  match result with
  | Some (count, memo) => expected = Some count /\ memo_sound store memo
  | None => expected = None
  end.

Lemma memo_sound_extend :
  forall store memo object remaining count,
    memo_sound store memo ->
    radix_count (S remaining) store remaining object = Some count ->
    memo_sound store (((object, remaining), count) :: memo).
Proof.
  intros store memo object remaining count sound counted.
  intros probe width found found_lookup. cbn [memo_lookup] in found_lookup.
  destruct (Nat.eqb object probe && Nat.eqb remaining width) eqn:hit.
  - apply andb_prop in hit as [same_object same_width].
    apply Nat.eqb_eq in same_object. apply Nat.eqb_eq in same_width.
    subst probe width. injection found_lookup as found_is. subst found. exact counted.
  - exact (sound probe width found found_lookup).
Qed.

Lemma record_count_agrees :
  forall store object remaining computed,
    agrees_with_count store (radix_count (S remaining) store remaining object) computed ->
    agrees_with_count store (radix_count (S remaining) store remaining object)
      (record_count object remaining computed).
Proof.
  intros store object remaining [[count after]|] agrees;
    cbn [record_count agrees_with_count] in agrees |- *.
  - destruct agrees as [counted after_sound].
    split; [exact counted|]. exact (memo_sound_extend store after object remaining count
                                      after_sound counted).
  - exact agrees.
Qed.

Lemma memo_count_children_with_agrees :
  forall store width (count_child : count_memo -> nat -> option (nat * count_memo)),
    (forall memo child,
        memo_sound store memo ->
        agrees_with_count store (radix_count (S width) store width child)
          (count_child memo child)) ->
    forall children memo,
      memo_sound store memo ->
      agrees_with_count store
        (count_children_with (radix_count (S width) store width) children)
        (memo_count_children_with count_child memo children).
Proof.
  intros store width count_child child_agrees children.
  induction children as [|[edge child] rest IH]; intros memo sound.
  - cbn [memo_count_children_with count_children_with agrees_with_count].
    split; [reflexivity | exact sound].
  - cbn [memo_count_children_with count_children_with].
    pose proof (child_agrees memo child sound) as here_agrees.
    destruct (count_child memo child) as [[here after_child]|];
      cbn [agrees_with_count] in here_agrees.
    + destruct here_agrees as [here_count after_child_sound].
      rewrite here_count. cbv beta iota.
      pose proof (IH after_child after_child_sound) as rest_agrees.
      destruct (memo_count_children_with count_child after_child rest)
        as [[later after_rest]|];
        cbn [agrees_with_count] in rest_agrees |- *.
      * destruct rest_agrees as [later_count after_rest_sound].
        rewrite later_count. split; [reflexivity | exact after_rest_sound].
      * rewrite rest_agrees. reflexivity.
    + rewrite here_agrees. cbn [agrees_with_count]. reflexivity.
Qed.

(** Capstone 18: from a sound memo, the memoized count refuses exactly when
    the count refuses, and otherwise returns the same count with a sound
    memo.  Keying the memo by (object, remaining width) is what makes reuse
    sound, because one object may be valid at one width and invalid at
    another. *)
Theorem memoized_radix_count_agrees_with_radix_count :
  forall fuel store memo remaining object,
    memo_sound store memo ->
    S remaining <= fuel ->
    agrees_with_count store (radix_count (S remaining) store remaining object)
      (memoized_radix_count fuel store memo remaining object).
Proof.
  induction fuel as [|smaller IH]; intros store memo remaining object sound enough.
  - lia.
  - cbn [memoized_radix_count].
    destruct (memo_lookup memo object remaining) as [cached|] eqn:cached_lookup.
    + cbn [agrees_with_count].
      split; [exact (sound object remaining cached cached_lookup) | exact sound].
    + apply record_count_agrees.
      cbn [radix_count].
      destruct (lookup_radix_node store object) as [[prefix claim children]|];
        cbn [agrees_with_count radix_prefix radix_claim radix_children]; [|reflexivity].
      destruct (Nat.compare_spec (length prefix) remaining) as [at_depth|below|above].
      * destruct claim as [claim|]; destruct children as [|first rest];
          cbn [agrees_with_count]; try reflexivity.
        split; [reflexivity | exact sound].
      * destruct claim as [claim|]; cbn [agrees_with_count]; [reflexivity|].
        destruct (edges_strictly_increasing children && (2 <=? length children));
          cbn [agrees_with_count]; [|reflexivity].
        rewrite (count_children_with_ext
                   (radix_count remaining store (remaining - S (length prefix)))
                   (radix_count (S (remaining - S (length prefix))) store
                      (remaining - S (length prefix)))).
        -- apply memo_count_children_with_agrees; [|exact sound].
           intros current child current_sound.
           apply IH; [exact current_sound | lia].
        -- intros edge child _. apply remaining_width_is_sufficient_fuel; lia.
      * destruct claim as [claim|]; destruct children as [|first rest];
          cbn [agrees_with_count]; reflexivity.
Qed.

Fixpoint digest_memo_lookup (memo : list (nat * nat)) (object : nat) : option nat :=
  match memo with
  | [] => None
  | (candidate, count) :: rest =>
      if Nat.eqb candidate object then Some count else digest_memo_lookup rest object
  end.

(** The digest-only mutant keys its memo by the object alone, so it reuses a
    count recorded at one remaining width at another width. *)
Fixpoint digest_only_memo_mutant
    (fuel : nat) (store : list (nat * radix_node)) (memo : list (nat * nat))
    (remaining object : nat) {struct fuel} : option (nat * list (nat * nat)) :=
  match fuel with
  | 0 => None
  | S smaller =>
      match digest_memo_lookup memo object with
      | Some count => Some (count, memo)
      | None =>
          match
            match lookup_radix_node store object with
            | None => None
            | Some node =>
                match Nat.compare (length (radix_prefix node)) remaining,
                      radix_claim node, radix_children node with
                | Eq, Some _, [] => Some (1, memo)
                | Lt, None, children =>
                    if edges_strictly_increasing children && (2 <=? length children) then
                      memo_count_children_with
                        (fun current child =>
                           digest_only_memo_mutant smaller store current
                             (remaining - S (length (radix_prefix node))) child)
                        memo children
                    else None
                | _, _, _ => None
                end
            end
          with
          | Some (count, after) => Some (count, (object, count) :: after)
          | None => None
          end
      end
  end.

(** Leaf object 2 holds a 47-byte suffix, so it is a valid endpoint below the
    root but not below node 3, where only 46 key bytes remain. *)
Definition misplaced_shared_leaf_store : list (nat * radix_node) :=
  [(1, RadixNode [] None [(0, 2); (1, 3)]);
   (2, RadixNode (repeat 0 47) (Some 50) []);
   (3, RadixNode [] None [(0, 2); (1, 4)]);
   (4, RadixNode (repeat 0 46) (Some 50) [])].

(** Capstone 19 (negative control): a memo keyed by the object alone accepts
    a radix whose shared leaf is misplaced, and declares three entries where
    the count and the width-keyed memo refuse (it violates capstone 18). *)
Theorem digest_only_memo_mutant_accepts_a_misplaced_shared_leaf :
  radix_count 49 misplaced_shared_leaf_store retention_key_len 1 = None /\
  option_map fst (memoized_radix_count 49 misplaced_shared_leaf_store [] retention_key_len 1) =
    None /\
  option_map fst (digest_only_memo_mutant 49 misplaced_shared_leaf_store [] retention_key_len 1) =
    Some 3.
Proof. repeat split; vm_compute; reflexivity. Qed.

(** Capstone 20: when the checkpoint data, the retained union and the
    metadata closure are each duplicate-free, the retained union and the
    metadata closure are disjoint and both included in the data, and the
    data count is the sum of their counts, then the data is exactly their
    disjoint union and its canonical bytes are the sum of theirs.  The cold
    reader therefore authenticates the metadata closure separately from the
    retained union with an inclusion check per object and one count check. *)
Theorem count_and_inclusion_authenticate_the_disjoint_union :
  forall data retained metadata : list inventory_object,
    NoDup data -> NoDup retained -> NoDup metadata ->
    (forall object, In object retained -> ~ In object metadata) ->
    incl retained data -> incl metadata data ->
    length data = length retained + length metadata ->
    Permutation data (retained ++ metadata) /\
    canonical_byte_sum snd data =
      canonical_byte_sum snd retained + canonical_byte_sum snd metadata.
Proof.
  intros data retained metadata data_distinct retained_distinct metadata_distinct
    disjoint retained_included metadata_included counted.
  assert (union_distinct : NoDup (retained ++ metadata)).
  { apply NoDup_app; [exact retained_distinct | exact metadata_distinct | exact disjoint]. }
  assert (union_included : incl (retained ++ metadata) data).
  { apply incl_app; [exact retained_included | exact metadata_included]. }
  assert (data_included : incl data (retained ++ metadata)).
  { apply NoDup_length_incl; [exact union_distinct | | exact union_included].
    rewrite length_app. lia. }
  assert (permutation : Permutation data (retained ++ metadata)).
  { apply NoDup_Permutation; [exact data_distinct | exact union_distinct |].
    intro object. split; [apply data_included | apply union_included]. }
  split; [exact permutation|].
  rewrite (canonical_byte_sum_permutation snd permutation).
  apply canonical_byte_sum_app.
Qed.

(** Object 2 is both retained and metadata; object 4 is neither.  The
    metadata closure is the V2 minimum: one radix node of 50 bytes and the
    136-byte anchor. *)
Definition overlapping_retained : list inventory_object := [(1, 50); (2, 50)].
Definition overlapping_metadata : list inventory_object := [(2, 50); (3, 136)].
Definition overlapping_data : list inventory_object := [(1, 50); (2, 50); (3, 136); (4, 50)].

(** Capstone 21 (negative control): without disjointness, the inclusion,
    count and byte checks all pass while the data holds an object that is in
    neither the retained union nor the metadata closure (it violates
    capstone 20). *)
Theorem overlapping_count_mutant_accepts_an_unauthenticated_object :
  NoDup overlapping_data /\ NoDup overlapping_retained /\ NoDup overlapping_metadata /\
  incl overlapping_retained overlapping_data /\ incl overlapping_metadata overlapping_data /\
  length overlapping_data = length overlapping_retained + length overlapping_metadata /\
  canonical_byte_sum snd overlapping_data =
    canonical_byte_sum snd overlapping_retained + canonical_byte_sum snd overlapping_metadata /\
  In (4, 50) overlapping_data /\
  ~ In (4, 50) (overlapping_retained ++ overlapping_metadata).
Proof.
  unfold overlapping_data, overlapping_retained, overlapping_metadata.
  split; [repeat constructor; cbn; intuition discriminate|].
  split; [repeat constructor; cbn; intuition discriminate|].
  split; [repeat constructor; cbn; intuition discriminate|].
  split; [intros object member; cbn in member |- *; intuition|].
  split; [intros object member; cbn in member |- *; intuition|].
  split; [reflexivity|].
  split; [reflexivity|].
  split; [cbn; intuition|].
  cbn. intuition discriminate.
Qed.

End RetentionV2Format.

Print Assumptions RetentionV2Format.retention_key_is_an_exact_48_byte_bijection.
Print Assumptions RetentionV2Format.retention_key_order_is_entry_order.
Print Assumptions RetentionV2Format.little_endian_key_mutant_inverts_generation_order.
Print Assumptions RetentionV2Format.lossy_key_mutants_collapse_v1_aliases.
Print Assumptions RetentionV2Format.retention_domains_are_pairwise_distinct.
Print Assumptions RetentionV2Format.domain_separation_excludes_cross_universe_digest_aliases.
Print Assumptions RetentionV2Format.shared_node_domain_mutant_aliases_empty_metadata_and_semantic_roots.
Print Assumptions RetentionV2Format.radix_count_denotes_entry_occurrences.
Print Assumptions RetentionV2Format.distinct_leaf_count_mutant_undercounts_shared_entries.
Print Assumptions RetentionV2Format.v2_checkpoint_inventory_is_retained_plus_metadata.
Print Assumptions RetentionV2Format.plus_one_rule_mutant_rejects_every_v2_inventory.
Print Assumptions RetentionV2Format.anchor_only_checkpoint_mutant_is_not_reference_closed.
Print Assumptions RetentionV2Format.v1_migration_preserves_entries_order_count_and_roots.
Print Assumptions RetentionV2Format.retention_format_binding_separates_representations_of_one_state.
Print Assumptions RetentionV2Format.state_version_only_binding_mutant_resumes_across_formats.
Print Assumptions RetentionV2Format.radix_count_success_unfolds_with_that_many_leaves.
Print Assumptions RetentionV2Format.shape_free_count_mutant_counts_an_empty_node_below_the_root.
Print Assumptions RetentionV2Format.memoized_radix_count_agrees_with_radix_count.
Print Assumptions RetentionV2Format.digest_only_memo_mutant_accepts_a_misplaced_shared_leaf.
Print Assumptions RetentionV2Format.count_and_inclusion_authenticate_the_disjoint_union.
Print Assumptions RetentionV2Format.overlapping_count_mutant_accepts_an_unauthenticated_object.
