From Stdlib Require Import Bool Arith PeanoNat Lists.List.
From StackSafePDA Require Import RSpaceCallerAuthority RSpaceConsensusIdentity
  RSpaceDirectManifestIdentity.
Import ListNotations RSpaceDirectManifestIdentity.RSpaceDirectManifestIdentity.

(**
  Nonvacuity witnesses and negative controls for the direct manifest identity
  of [RSpaceDirectManifestIdentity.v].

  The witness is one durable store of profile 1.  It holds five manifests of
  schema epoch V1: its current state, an earlier state, an imported state whose
  manifest carries the legacy history root 40, a state of the foreign profile
  2, and a state that its ledger no longer retains.  The ledger counts the
  first four as current or retained; it counts a retention anchor, a digest of
  a third domain under which the store holds an object that is not a
  manifest, in its metadata domain only.  A rebuilt store holds the same
  current state with no history and another catalog generation.  The preserved
  session of the witness is a checked coupled binding whose certified root is
  the legacy root of the frozen history [1].

  Digests are the injective codes of the theory: no proof below computes one,
  and every inequality between two digests follows from injectivity.

  Witnesses:
  - [witness_store_is_coherent]: the witness store is coherent;
  - [retained_states_resolve_by_their_own_reason]: the current and the earlier
    state resolve to their own roots, the imported state is refused for its
    legacy provenance and the foreign state for its profile, each after one
    read;
  - [other_digests_are_refused_without_a_read]: the unretained manifest and the
    anchor are stored, the ledger counts the anchor as metadata, and both are
    refused as unknown without a read;
  - [one_content_one_root_across_histories]: two stores with different
    histories and catalog generations meet the premises of
    [direct_root_is_function_of_semantic_roots] and publish one root;
  - [preserved_session_resolves_its_legacy_root],
    [preserved_root_is_refused_by_the_coherent_store],
    [durable_root_is_resolved_by_no_preserved_session] and
    [direct_binding_is_never_checked] instantiate each part of
    [direct_and_preserved_sessions_cannot_resolve_each_other] with premises
    that hold.

  Negative controls:
  - [history_chained_root_is_not_a_function_of_semantic_roots] (the red
    control of the theory): a root that also digests the retained history is
    not a function of the semantic roots, because the witness store and the
    rebuilt store have equal semantic roots and different such roots;
  - [presence_resolution_answers_a_state_the_store_no_longer_retains]: a
    resolution that decides by object-store presence instead of the ledger
    answers the unretained state, so it does not refuse every root that no
    current or retained state has, as [direct_resolution_is_exact] requires.
*)

Module RSpaceDirectManifestIdentityMutants.

(** * The witness store *)

(** [witness_profile] is the semantic profile of the witness stores. *)
Definition witness_profile : nat := 1.

(** The five manifests of the witness, named by their role. *)
Definition current_state : state_manifest :=
  StateManifest semantic_schema_epoch_v1 witness_profile (SemanticRoots 11 12 13) None.
Definition earlier_state : state_manifest :=
  StateManifest semantic_schema_epoch_v1 witness_profile (SemanticRoots 21 22 23) None.
Definition imported_state : state_manifest :=
  StateManifest semantic_schema_epoch_v1 witness_profile (SemanticRoots 31 32 33) (Some 40).
Definition foreign_state : state_manifest :=
  StateManifest semantic_schema_epoch_v1 2 (SemanticRoots 41 42 43) None.
Definition unretained_state : state_manifest :=
  StateManifest semantic_schema_epoch_v1 witness_profile (SemanticRoots 51 52 53) None.

(** [retention_anchor] is the digest of a retention anchor: a code in a third
    digest domain, 2, besides the manifest and legacy history domains. *)
Definition retention_anchor : nat := pair_code (2, 7).

(** [ledger_for current retained metadata] is a closure ledger that counts
    [current] in its current domain, each manifest of [retained] in its
    retained domain and each digest of [metadata] in its metadata domain
    only. *)
Definition ledger_for (current : state_manifest) (retained : list state_manifest)
    (metadata : list nat) (root : nat) : option root_counts :=
  if Nat.eqb (manifest_digest current) root then Some (RootCounts 1 0 0)
  else if existsb (fun manifest => Nat.eqb (manifest_digest manifest) root) retained
  then Some (RootCounts 0 1 0)
  else if existsb (Nat.eqb root) metadata then Some (RootCounts 0 0 1)
  else None.

(** [objects_for stored fallback] is an object store that holds the canonical
    bytes of each manifest of [stored] under its digest and answers [fallback]
    for every other digest. *)
Fixpoint objects_for (stored : list state_manifest) (fallback : nat -> store_read)
    (root : nat) : store_read :=
  match stored with
  | [] => fallback root
  | manifest :: rest =>
      if Nat.eqb (manifest_digest manifest) root then Present (ManifestBytes manifest)
      else objects_for rest fallback root
  end.

(** [anchor_object] holds the bytes of the retention anchor, which do not parse
    as a manifest, under its digest and nothing else. *)
Definition anchor_object (root : nat) : store_read :=
  if Nat.eqb root retention_anchor then Present (OtherBytes 7) else Absent.

(** The retained earlier states, all current and retained states, and all
    stored manifests of the witness store. *)
Definition witness_retained : list state_manifest := [earlier_state; imported_state; foreign_state].
Definition witness_states : list state_manifest := current_state :: witness_retained.
Definition witness_stored_manifests : list state_manifest := witness_states ++ [unretained_state].

(** [witness_store] is the witness: catalog generation 2, the four current and
    retained states, and the unretained manifest and the anchor stored
    besides. *)
Definition witness_store : durable_store :=
  DurableStore witness_profile current_state witness_retained 2
    (ledger_for current_state witness_retained [retention_anchor])
    (objects_for witness_stored_manifests anchor_object).

(** [rebuilt_store] holds the current state of [witness_store] with no
    retained history, another catalog generation and its own ledger and
    objects, as a store rebuilt from that state alone would. *)
Definition rebuilt_store : durable_store :=
  DurableStore witness_profile current_state [] 3
    (ledger_for current_state [] []) (objects_for [current_state] (fun _ => Absent)).

Lemma ledger_for_counts_exactly : forall current retained metadata root,
  counts_semantic_root (ledger_for current retained metadata) root = true <->
  exists manifest, In manifest (current :: retained) /\ manifest_digest manifest = root.
Proof.
  intros current retained metadata root.
  unfold counts_semantic_root, ledger_for.
  destruct (Nat.eqb_spec (manifest_digest current) root) as [current_root|not_current];
    cbv beta iota.
  - split.
    + intros _; exists current; split; [left; reflexivity|exact current_root].
    + intros _; reflexivity.
  - destruct (existsb (fun manifest => Nat.eqb (manifest_digest manifest) root) retained)
      eqn:retained_root; cbv beta iota.
    + split; [intros _|intros _; reflexivity].
      apply existsb_exists in retained_root as [manifest [retained_manifest same]].
      cbv beta in same; apply Nat.eqb_eq in same.
      exists manifest; split; [right; exact retained_manifest|exact same].
    + split.
      * destruct (existsb (Nat.eqb root) metadata); cbv beta iota;
          intros absurd; discriminate absurd.
      * intros [manifest [[is_current|retained_manifest] same]].
        -- subst manifest; contradiction.
        -- assert (existsb (fun manifest => Nat.eqb (manifest_digest manifest) root) retained
             = true) as counted.
           { apply existsb_exists; exists manifest; split; [exact retained_manifest|].
             cbv beta; apply Nat.eqb_eq; exact same. }
           rewrite retained_root in counted; discriminate counted.
Qed.

Lemma ledger_for_metadata_only : forall current retained metadata root,
  manifest_digest current <> root ->
  (forall manifest, In manifest retained -> manifest_digest manifest <> root) ->
  In root metadata ->
  ledger_for current retained metadata root = Some (RootCounts 0 0 1).
Proof.
  intros current retained metadata root not_current not_retained metadata_root.
  unfold ledger_for.
  destruct (Nat.eqb_spec (manifest_digest current) root) as [same|_]; [contradiction|].
  destruct (existsb (fun manifest => Nat.eqb (manifest_digest manifest) root) retained)
    eqn:retained_root.
  - apply existsb_exists in retained_root as [manifest [retained_manifest same]].
    cbv beta in same; apply Nat.eqb_eq in same.
    exfalso; exact (not_retained manifest retained_manifest same).
  - assert (existsb (Nat.eqb root) metadata = true) as counted.
    { apply existsb_exists; exists root; split; [exact metadata_root|apply Nat.eqb_refl]. }
    rewrite counted; reflexivity.
Qed.

Lemma objects_for_stores_each_manifest : forall stored fallback manifest,
  In manifest stored ->
  objects_for stored fallback (manifest_digest manifest) = Present (ManifestBytes manifest).
Proof.
  induction stored as [|head rest stores_rest]; intros fallback manifest stored_manifest.
  - destruct stored_manifest.
  - cbn [objects_for].
    destruct (Nat.eqb_spec (manifest_digest head) (manifest_digest manifest))
      as [same|different].
    + apply manifest_digest_injective in same; subst head; reflexivity.
    + destruct stored_manifest as [is_head|in_rest].
      * subst head; contradiction.
      * apply stores_rest; exact in_rest.
Qed.

Lemma objects_for_falls_back : forall stored fallback root,
  (forall manifest, In manifest stored -> manifest_digest manifest <> root) ->
  objects_for stored fallback root = fallback root.
Proof.
  induction stored as [|head rest falls_back_rest]; intros fallback root named_by_none.
  - reflexivity.
  - cbn [objects_for].
    destruct (Nat.eqb_spec (manifest_digest head) root) as [same|_].
    + exfalso; exact (named_by_none head (or_introl eq_refl) same).
    + apply falls_back_rest; intros manifest stored_manifest.
      exact (named_by_none manifest (or_intror stored_manifest)).
Qed.

Lemma manifest_digest_is_not_the_anchor : forall manifest,
  manifest_digest manifest <> retention_anchor.
Proof.
  intros manifest same.
  unfold manifest_digest, retention_anchor in same.
  apply pair_code_injective, pair_equal_spec in same as [different_domains _].
  unfold manifest_domain in different_domains.
  discriminate different_domains.
Qed.

Lemma unretained_state_is_stored :
  objects witness_store (manifest_digest unretained_state) =
    Present (ManifestBytes unretained_state).
Proof.
  assert (In unretained_state witness_stored_manifests) as stored.
  { unfold witness_stored_manifests; apply in_or_app; right; left; reflexivity. }
  exact (objects_for_stores_each_manifest witness_stored_manifests anchor_object
    unretained_state stored).
Qed.

Lemma unretained_state_is_not_retained : forall manifest,
  In manifest (retained_states witness_store) ->
  manifest_digest manifest <> manifest_digest unretained_state.
Proof.
  intros manifest retained same.
  apply manifest_digest_injective in same; subst manifest.
  destruct retained as [same|[same|[same|[same|[]]]]]; discriminate same.
Qed.

Example witness_store_is_coherent : coherent witness_store.
Proof.
  split.
  - intros root.
    exact (ledger_for_counts_exactly current_state witness_retained [retention_anchor] root).
  - intros manifest retained; split.
    + destruct retained as [<-|[<-|[<-|[<-|[]]]]]; reflexivity.
    + assert (In manifest witness_stored_manifests) as stored.
      { unfold witness_stored_manifests; apply in_or_app; left; exact retained. }
      exact (objects_for_stores_each_manifest witness_stored_manifests anchor_object
        manifest stored).
Qed.

Example retained_states_resolve_by_their_own_reason :
  resolve_direct witness_store (manifest_digest current_state) =
    Resolution [manifest_digest current_state]
      (Resolved (manifest_digest current_state) (roots current_state)) /\
  resolve_direct witness_store (manifest_digest earlier_state) =
    Resolution [manifest_digest earlier_state]
      (Resolved (manifest_digest earlier_state) (roots earlier_state)) /\
  resolve_direct witness_store (manifest_digest imported_state) =
    Resolution [manifest_digest imported_state] (Refused (UnknownRoot LegacyProvenance)) /\
  resolve_direct witness_store (manifest_digest foreign_state) =
    Resolution [manifest_digest foreign_state] (Refused (UnknownRoot ForeignProfile)).
Proof.
  split; [|split; [|split]].
  - rewrite (coherent_store_answers_each_retained_state witness_store current_state
      witness_store_is_coherent); [reflexivity|left; reflexivity].
  - rewrite (coherent_store_answers_each_retained_state witness_store earlier_state
      witness_store_is_coherent); [reflexivity|right; left; reflexivity].
  - rewrite (coherent_store_answers_each_retained_state witness_store imported_state
      witness_store_is_coherent); [reflexivity|right; right; left; reflexivity].
  - rewrite (coherent_store_answers_each_retained_state witness_store foreign_state
      witness_store_is_coherent); [reflexivity|right; right; right; left; reflexivity].
Qed.

Example other_digests_are_refused_without_a_read :
  objects witness_store (manifest_digest unretained_state) =
    Present (ManifestBytes unretained_state) /\
  resolve_direct witness_store (manifest_digest unretained_state) =
    Resolution [] (Refused (UnknownRoot NotCurrentOrRetained)) /\
  objects witness_store retention_anchor = Present (OtherBytes 7) /\
  ledger witness_store retention_anchor = Some (RootCounts 0 0 1) /\
  resolve_direct witness_store retention_anchor =
    Resolution [] (Refused (UnknownRoot NotCurrentOrRetained)).
Proof.
  split; [exact unretained_state_is_stored|].
  split.
  { apply coherent_store_refuses_every_other_root;
      [exact witness_store_is_coherent|exact unretained_state_is_not_retained]. }
  split.
  { transitivity (anchor_object retention_anchor).
    - apply objects_for_falls_back; intros manifest _; apply manifest_digest_is_not_the_anchor.
    - unfold anchor_object; rewrite Nat.eqb_refl; reflexivity. }
  split.
  { apply ledger_for_metadata_only.
    - apply manifest_digest_is_not_the_anchor.
    - intros manifest _; apply manifest_digest_is_not_the_anchor.
    - left; reflexivity. }
  apply coherent_store_refuses_every_other_root; [exact witness_store_is_coherent|].
  intros manifest _; apply manifest_digest_is_not_the_anchor.
Qed.

Example one_content_one_root_across_histories :
  store_profile witness_store = store_profile rebuilt_store /\
  durable_manifest (store_profile witness_store) (current_manifest witness_store) /\
  durable_manifest (store_profile rebuilt_store) (current_manifest rebuilt_store) /\
  retained_manifests witness_store <> retained_manifests rebuilt_store /\
  catalog_generation witness_store <> catalog_generation rebuilt_store /\
  published_root witness_store = published_root rebuilt_store.
Proof.
  assert (store_profile witness_store = store_profile rebuilt_store) as same_profile
    by reflexivity.
  assert (durable_manifest (store_profile witness_store) (current_manifest witness_store))
    as witness_durable by (repeat split).
  assert (durable_manifest (store_profile rebuilt_store) (current_manifest rebuilt_store))
    as rebuilt_durable by (repeat split).
  split; [exact same_profile|].
  split; [exact witness_durable|].
  split; [exact rebuilt_durable|].
  split; [discriminate|].
  split; [discriminate|].
  apply (proj2 (direct_root_is_function_of_semantic_roots witness_store rebuilt_store
    same_profile witness_durable rebuilt_durable)).
  reflexivity.
Qed.

(** * The red control: a root chained over the retained history *)

(** [history_chained_root] is the mutant root: the code of the published root
    together with the digests of the retained manifests, as a root chained over
    the retained history would be. *)
Definition history_chained_root (store : durable_store) : nat :=
  pair_code (published_root store, list_code (map manifest_digest (retained_manifests store))).

Example history_chained_root_is_not_a_function_of_semantic_roots :
  ~ root_is_function_of_semantic_roots history_chained_root.
Proof.
  intros claimed.
  assert (store_profile witness_store = store_profile rebuilt_store) as same_profile
    by reflexivity.
  assert (durable_manifest (store_profile witness_store) (current_manifest witness_store))
    as witness_durable by (repeat split).
  assert (durable_manifest (store_profile rebuilt_store) (current_manifest rebuilt_store))
    as rebuilt_durable by (repeat split).
  pose proof (proj2 (claimed witness_store rebuilt_store same_profile witness_durable
    rebuilt_durable) eq_refl) as same_chained_root.
  unfold history_chained_root in same_chained_root.
  apply pair_code_injective, pair_equal_spec in same_chained_root as [_ same_history_code].
  apply list_code_injective in same_history_code.
  discriminate same_history_code.
Qed.

(** * The second control: resolution by object-store presence *)

(** [resolve_by_presence] is the mutant resolution: it skips the ledger probe,
    always reads the object stored under the root and decides by what it
    finds there. *)
Definition resolve_by_presence (store : durable_store) (root : nat) : resolution :=
  Resolution [root]
    (match objects store root with
     | ReadFailure => Refused StoreFailure
     | Absent => Refused (UnknownRoot NotCurrentOrRetained)
     | Present object =>
         match parse_manifest object with
         | None => Refused CorruptRootCatalog
         | Some manifest =>
             if Nat.eqb (manifest_digest manifest) root
             then manifest_outcome (store_profile store) manifest
             else Refused CorruptRootCatalog
         end
     end).

Lemma resolve_by_presence_answers_a_stored_manifest : forall store manifest,
  objects store (manifest_digest manifest) = Present (ManifestBytes manifest) ->
  schema_epoch manifest = semantic_schema_epoch_v1 ->
  result (resolve_by_presence store (manifest_digest manifest)) =
    manifest_outcome (store_profile store) manifest.
Proof.
  intros store manifest stored v1.
  unfold resolve_by_presence, parse_manifest; cbv beta iota delta [result].
  rewrite stored; cbv beta iota.
  rewrite v1, Nat.eqb_refl; cbv beta iota.
  rewrite Nat.eqb_refl; reflexivity.
Qed.

Example presence_resolution_answers_a_state_the_store_no_longer_retains :
  result (resolve_by_presence witness_store (manifest_digest unretained_state)) =
    Resolved (manifest_digest unretained_state) (roots unretained_state) /\
  ~ (forall store root, coherent store ->
       (forall manifest, In manifest (retained_states store) ->
          manifest_digest manifest <> root) ->
       resolve_by_presence store root =
         Resolution [] (Refused (UnknownRoot NotCurrentOrRetained))).
Proof.
  split.
  - rewrite (resolve_by_presence_answers_a_stored_manifest witness_store unretained_state
      unretained_state_is_stored eq_refl).
    reflexivity.
  - intros claimed.
    assert (resolve_by_presence witness_store (manifest_digest unretained_state) =
      Resolution [] (Refused (UnknownRoot NotCurrentOrRetained))) as refused.
    { apply claimed; [exact witness_store_is_coherent|exact unretained_state_is_not_retained]. }
    discriminate refused.
Qed.

(** * Direct and preserved sessions *)

(** [preserved_root] is the legacy root of the frozen history [1]. *)
Definition preserved_root : nat := legacy_root_digest [1].

(** [preserved_binding] is a coupled binding of the preserved legacy identity
    whose compatibility facet freezes the history [1] and certifies
    [preserved_root]. *)
Definition preserved_binding : Consensus.bridge_binding :=
  Consensus.BridgeBinding 20 Consensus.witness_domain Consensus.PreservedLegacyV1
    Consensus.witness_semantic (Consensus.CompatibilityFacet 31 [1]) preserved_root.

(** [preserved_owner] retains the binding reference 20, and its authenticated
    alias maps [preserved_root] to it. *)
Definition preserved_owner : Authority.owner :=
  Authority.Owner Consensus.witness_domain 4 20 7 true true [20] [(preserved_root, 20)].

(** [preserved_lookup] authenticates [preserved_binding] under reference 20,
    and [refined] accepts every binding as a legacy refinement. *)
Definition preserved_lookup (_ : Authority.owner) (reference : nat)
    : option Consensus.bridge_binding :=
  if Nat.eqb reference 20 then Some preserved_binding else None.
Definition refined (_ : Consensus.bridge_binding) : Prop := True.

(** [direct_binding] differs from [preserved_binding] only in its version,
    [DirectManifestV1], and [direct_lookup] authenticates it under the same
    reference. *)
Definition direct_binding : Consensus.bridge_binding :=
  Consensus.BridgeBinding 20 Consensus.witness_domain Consensus.DirectManifestV1
    Consensus.witness_semantic (Consensus.CompatibilityFacet 31 [1]) preserved_root.
Definition direct_lookup (_ : Authority.owner) (reference : nat)
    : option Consensus.bridge_binding :=
  if Nat.eqb reference 20 then Some direct_binding else None.

Example preserved_session_resolves_its_legacy_root :
  Consensus.resolve preserved_lookup refined legacy_root_digest preserved_owner
    Consensus.PreservedLegacyV1 preserved_root preserved_binding.
Proof.
  constructor.
  - reflexivity.
  - repeat split.
  - reflexivity.
  - unfold Authority.resolves_alias, Authority.alias_targets.
    split; [reflexivity|split; [reflexivity|]].
    simpl; rewrite Nat.eqb_refl; reflexivity.
  - left; reflexivity.
Qed.

Example preserved_root_is_refused_by_the_coherent_store :
  resolve_direct witness_store preserved_root =
    Resolution [] (Refused (UnknownRoot NotCurrentOrRetained)).
Proof.
  destruct (direct_and_preserved_sessions_cannot_resolve_each_other preserved_lookup refined
    preserved_owner Consensus.PreservedLegacyV1 witness_store preserved_root preserved_binding)
    as [_ [_ [refused _]]].
  exact (refused preserved_session_resolves_its_legacy_root witness_store_is_coherent).
Qed.

Example durable_root_is_resolved_by_no_preserved_session :
  result (resolve_direct witness_store (published_root witness_store)) =
    Resolved (published_root witness_store) (roots current_state) /\
  forall session binding,
    ~ Consensus.resolve preserved_lookup refined legacy_root_digest preserved_owner session
        (published_root witness_store) binding.
Proof.
  assert (result (resolve_direct witness_store (published_root witness_store)) =
    Resolved (published_root witness_store) (roots current_state)) as resolved.
  { destruct retained_states_resolve_by_their_own_reason as [current_resolves _].
    exact (f_equal result current_resolves). }
  split; [exact resolved|].
  intros session binding.
  destruct (direct_and_preserved_sessions_cannot_resolve_each_other preserved_lookup refined
    preserved_owner session witness_store (published_root witness_store) binding)
    as [_ [_ [_ [not_preserved _]]]].
  exact (not_preserved _ _ resolved).
Qed.

Example direct_binding_is_never_checked :
  Consensus.authenticated_binding direct_lookup preserved_owner direct_binding /\
  ~ Consensus.checked direct_lookup refined legacy_root_digest preserved_owner direct_binding /\
  ~ Consensus.resolve direct_lookup refined legacy_root_digest preserved_owner
      Consensus.DirectManifestV1 preserved_root direct_binding.
Proof.
  split; [unfold Consensus.authenticated_binding; reflexivity|].
  destruct (direct_and_preserved_sessions_cannot_resolve_each_other direct_lookup refined
    preserved_owner Consensus.DirectManifestV1 witness_store preserved_root direct_binding)
    as [no_direct_session [never_checked _]].
  split; [exact (never_checked eq_refl)|exact no_direct_session].
Qed.

Print Assumptions witness_store_is_coherent.
Print Assumptions retained_states_resolve_by_their_own_reason.
Print Assumptions other_digests_are_refused_without_a_read.
Print Assumptions one_content_one_root_across_histories.
Print Assumptions history_chained_root_is_not_a_function_of_semantic_roots.
Print Assumptions presence_resolution_answers_a_state_the_store_no_longer_retains.
Print Assumptions preserved_session_resolves_its_legacy_root.
Print Assumptions preserved_root_is_refused_by_the_coherent_store.
Print Assumptions durable_root_is_resolved_by_no_preserved_session.
Print Assumptions direct_binding_is_never_checked.

End RSpaceDirectManifestIdentityMutants.
