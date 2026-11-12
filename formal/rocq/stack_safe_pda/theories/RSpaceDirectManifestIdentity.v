From Stdlib Require Import Bool Arith PeanoNat Lists.List.
From Stdlib Require Cantor.
From StackSafePDA Require Import RSpaceCallerAuthority RSpaceConsensusIdentity.
Import ListNotations.

(**
  The direct manifest identity of the durable RSpace store.

  In durable mode the consensus state root of an RSpace state is the store's
  own identity: the 32-byte Blake2b-256 digest of the state's canonical
  semantic manifest ([DurableStateRootV1] in [state_root.rs]).  No LMDB root
  is reproduced and no table maps one kind of root to the other.  LMDB mode
  keeps the preserved legacy identity of [RSpaceConsensusIdentity.v], whose
  sessions speak [PreservedLegacyV1]; the durable identity is the version
  [DirectManifestV1].  This theory states what the durable store's root is and
  how [resolve_retained_state] resolves a root, step by step:

  1. probe the closure ledger once; a digest whose current and retained root
     counts are both zero is refused as unknown, and no object is read;
  2. read the object stored under the digest once;
  3. parse it as a semantic manifest of schema epoch V1 and require its digest
     to be the root;
  4. refuse a manifest that carries a legacy history root;
  5. refuse a manifest of another semantic profile;
  6. answer the manifest's reference and its three namespace roots.

  Abstraction.  A digest is modelled as an injective code.  [pair_code] is an
  injective map from pairs of naturals to naturals; it is built from the
  Cantor pairing of the standard library and closed with [Qed], so a proof
  can use only its injectivity and never computes a digest.  The digest of a
  manifest and a legacy history root are codes in two different domains, as
  the domain tag of the canonical envelope separates the preimages of
  different object kinds.  Two equal digests therefore have equal preimages of
  the same kind: the collision resistance of Blake2b-256 is part of this
  abstraction and is not proved.  The three namespace roots are opaque
  naturals; that the root of a canonical radix DAG is a function of the set of
  endpoints it holds is an executable obligation outside this model.

  Theorems.
  - [direct_root_is_function_of_semantic_roots]: for two stores of one profile
    whose current manifests are durable (schema epoch V1, that profile and no
    legacy history root), the published roots are equal exactly when the
    semantic roots are equal, whatever the retained histories, catalog
    generations, ledgers and objects of the two stores.
  - [direct_resolution_is_exact]: a resolution reads at most one object, the
    one stored under the root, and only when the ledger counts the root; it
    answers a state exactly when that object is a durable manifest whose
    digest is the root, and then answers that manifest's roots; on a coherent
    store every root of a current or retained state gets that state's own
    outcome, and every other root is refused as unknown without a read.
  - [direct_and_preserved_sessions_cannot_resolve_each_other]: the coupled
    bridge refuses every direct session and never checks a binding of the
    direct version; a root that a preserved session resolves is refused by
    every coherent durable store without a read; a root that a durable store
    resolves is resolved by no preserved session; and no manifest that carries
    a legacy history root resolves in any durable store.

  The companion [RSpaceDirectManifestIdentityMutants.v] shows that the
  premises of the three theorems are satisfiable and refutes two mutants: a
  root chained over the retained history, and a resolution that decides by
  object-store presence instead of the ledger.
*)

Module RSpaceDirectManifestIdentity.
Module Authority := RSpaceCallerAuthority.RSpaceCallerAuthority.
Module Consensus := RSpaceConsensusIdentity.RSpaceConsensusIdentity.

(** * Digests *)

(** An injective map from pairs of naturals to naturals exists: the Cantor
    pairing of the standard library is one.  The proof is closed with [Qed],
    so the map is opaque. *)
Lemma injective_pairing_exists :
  { code : nat * nat -> nat | forall left right, code left = code right -> left = right }.
Proof.
  exists Cantor.to_nat.
  intros left right same.
  apply (f_equal Cantor.of_nat) in same.
  rewrite !Cantor.cancel_of_to in same.
  exact same.
Qed.

(** [pair_code] is the code of a pair of naturals, the model of a digest over
    two fields. *)
Definition pair_code : nat * nat -> nat := proj1_sig injective_pairing_exists.

Arguments pair_code : simpl never.

Lemma pair_code_injective : forall left right,
  pair_code left = pair_code right -> left = right.
Proof. exact (proj2_sig injective_pairing_exists). Qed.

(** [option_code] codes an optional natural: [None] is 0 and [Some value] is
    the successor of [value]. *)
Definition option_code (value : option nat) : nat :=
  match value with
  | None => 0
  | Some present => S present
  end.

(** [list_code] codes a list of naturals: the empty list is 0 and a nonempty
    list is the successor of the code of its head and the code of its tail. *)
Fixpoint list_code (values : list nat) : nat :=
  match values with
  | [] => 0
  | value :: rest => S (pair_code (value, list_code rest))
  end.

Lemma option_code_injective : forall left right,
  option_code left = option_code right -> left = right.
Proof.
  intros [left|] [right|]; simpl; intros same; try discriminate same; congruence.
Qed.

Lemma list_code_injective : forall left right,
  list_code left = list_code right -> left = right.
Proof.
  induction left as [|value rest same_rest_code]; intros [|other others]; simpl; intros same.
  - reflexivity.
  - discriminate same.
  - discriminate same.
  - injection same as same_pair.
    apply pair_code_injective, pair_equal_spec in same_pair as [same_value same_codes].
    apply same_rest_code in same_codes.
    subst; reflexivity.
Qed.

(** * Semantic manifests *)

(** [semantic_roots] are the three namespace roots of a state
    ([SemanticRoots]): the data bag, the continuation map and the join bag. *)
Record semantic_roots := SemanticRoots {
  data_bag_root : nat;
  continuation_map_root : nat;
  join_bag_root : nat
}.

(** [state_manifest] holds exactly the fields that the canonical bytes of a
    [SemanticStateManifestV1] encode: the schema epoch, the semantic profile,
    the three namespace roots and the optional legacy history root, which
    records where an imported state came from (provenance, not content). *)
Record state_manifest := StateManifest {
  schema_epoch : nat;
  profile_id : nat;
  roots : semantic_roots;
  legacy_history_root : option nat
}.

(** [semantic_schema_epoch_v1] is [SEMANTIC_SCHEMA_EPOCH_V1], the only schema
    epoch with which a manifest is built or parsed. *)
Definition semantic_schema_epoch_v1 : nat := 1.

(** [durable_manifest profile manifest] holds when a store of [profile] can
    resolve [manifest]: schema epoch V1, that profile and no legacy history
    root. *)
Definition durable_manifest (profile : nat) (manifest : state_manifest) : Prop :=
  schema_epoch manifest = semantic_schema_epoch_v1 /\ profile_id manifest = profile /\
  legacy_history_root manifest = None.

(** [roots_code] and [manifest_code] code the canonical fields in their
    canonical order. *)
Definition roots_code (namespace_roots : semantic_roots) : nat :=
  pair_code (data_bag_root namespace_roots,
    pair_code (continuation_map_root namespace_roots, join_bag_root namespace_roots)).

Definition manifest_code (manifest : state_manifest) : nat :=
  pair_code (schema_epoch manifest,
    pair_code (profile_id manifest,
      pair_code (roots_code (roots manifest), option_code (legacy_history_root manifest)))).

(** The digest domains of semantic manifests and of legacy history roots. *)
Definition manifest_domain : nat := 0.
Definition legacy_history_domain : nat := 1.

(** [manifest_digest] is the direct manifest identity of a manifest, the digest
    of its canonical bytes ([DurableStateRootV1::of_manifest]). *)
Definition manifest_digest (manifest : state_manifest) : nat :=
  pair_code (manifest_domain, manifest_code manifest).

(** [legacy_root_digest] is a legacy history root, the digest of frozen legacy
    history bytes.  It is the frozen root with which this theory instantiates
    the coupled bridge of [RSpaceConsensusIdentity.v]. *)
Definition legacy_root_digest (history : list nat) : nat :=
  pair_code (legacy_history_domain, list_code history).

Lemma roots_code_injective : forall left right,
  roots_code left = roots_code right -> left = right.
Proof.
  intros [data_left continuation_left join_left] [data_right continuation_right join_right] same.
  unfold roots_code in same; simpl in same.
  apply pair_code_injective, pair_equal_spec in same as [same_data same_tail].
  apply pair_code_injective, pair_equal_spec in same_tail as [same_continuation same_join].
  subst; reflexivity.
Qed.

Lemma manifest_digest_injective : forall left right,
  manifest_digest left = manifest_digest right -> left = right.
Proof.
  intros [schema_left profile_left roots_left legacy_left]
    [schema_right profile_right roots_right legacy_right] same.
  unfold manifest_digest, manifest_code in same; simpl in same.
  apply pair_code_injective, pair_equal_spec in same as [_ same_code].
  apply pair_code_injective, pair_equal_spec in same_code as [same_schema same_after_schema].
  apply pair_code_injective, pair_equal_spec in same_after_schema
    as [same_profile same_after_profile].
  apply pair_code_injective, pair_equal_spec in same_after_profile as [same_roots same_legacy].
  apply roots_code_injective in same_roots.
  apply option_code_injective in same_legacy.
  subst; reflexivity.
Qed.

Lemma manifest_digest_is_not_a_legacy_root : forall manifest history,
  manifest_digest manifest <> legacy_root_digest history.
Proof.
  intros manifest history same.
  unfold manifest_digest, legacy_root_digest in same.
  apply pair_code_injective, pair_equal_spec in same as [different_domains _].
  unfold manifest_domain, legacy_history_domain in different_domains.
  discriminate different_domains.
Qed.

(** * The durable store *)

(** [root_counts] are the root counts that the closure ledger keeps for one
    digest in its current, retained and metadata domains. *)
Record root_counts := RootCounts {
  current_count : nat;
  retained_count : nat;
  metadata_count : nat
}.

(** [counts_semantic_root] is [StructuralClosureLedgerV1::counts_semantic_root]:
    a digest names a state when its current or its retained root count is
    positive.  A digest that only the metadata domain counts, such as a
    retention anchor, names no state. *)
Definition counts_semantic_root (ledger : nat -> option root_counts) (root : nat) : bool :=
  match ledger root with
  | None => false
  | Some (RootCounts 0 0 _) => false
  | Some _ => true
  end.

(** [stored_object] is what the object store holds under a digest: the
    canonical bytes of a manifest, or bytes that do not parse as a semantic
    manifest, such as a trie node, a retention manifest or a retention
    anchor. *)
Inductive stored_object :=
| ManifestBytes (manifest : state_manifest)
| OtherBytes (tag : nat).

(** [store_read] is the outcome of one bounded object read: a failure of the
    store, no object, or the object. *)
Inductive store_read :=
| ReadFailure
| Absent
| Present (object : stored_object).

(** [durable_store] is an admitted durable authority core: the store's
    semantic profile, its current manifest, the manifests of the earlier
    states it retains, its catalog generation (a physical coordinate), its
    closure ledger and its object store. *)
Record durable_store := DurableStore {
  store_profile : nat;
  current_manifest : state_manifest;
  retained_manifests : list state_manifest;
  catalog_generation : nat;
  ledger : nat -> option root_counts;
  objects : nat -> store_read
}.

(** [retained_states] are the current state and the retained earlier
    states. *)
Definition retained_states (store : durable_store) : list state_manifest :=
  current_manifest store :: retained_manifests store.

(** [published_root] is the consensus state root of the store in durable
    mode: the direct manifest identity of its current manifest. *)
Definition published_root (store : durable_store) : nat :=
  manifest_digest (current_manifest store).

(** [coherent] is what cold admission and the closure ledger establish: the
    ledger counts exactly the digests of the current and retained manifests,
    and each of them is a manifest of schema epoch V1 stored under its own
    digest.  The object store may hold other objects, among them manifests that
    the ledger no longer retains. *)
Definition coherent (store : durable_store) : Prop :=
  (forall root, counts_semantic_root (ledger store) root = true <->
     exists manifest, In manifest (retained_states store) /\ manifest_digest manifest = root) /\
  (forall manifest, In manifest (retained_states store) ->
     schema_epoch manifest = semantic_schema_epoch_v1 /\
     objects store (manifest_digest manifest) = Present (ManifestBytes manifest)).

(** * Resolution *)

(** [unknown_reason] is why a root is unknown ([RootError::UnknownRootError]):
    the ledger does not count it, its manifest carries a legacy history root,
    or its manifest is of another semantic profile. *)
Inductive unknown_reason :=
| NotCurrentOrRetained
| LegacyProvenance
| ForeignProfile.

(** [refusal] is a refused resolution: an unknown root, a ledger and an
    object store that disagree ([RootError::CorruptRootCatalog]), or a failed
    read ([RootError::KvStoreError]). *)
Inductive refusal :=
| UnknownRoot (reason : unknown_reason)
| CorruptRootCatalog
| StoreFailure.

(** [outcome] is a resolved state, given by its reference and its namespace
    roots, or a refusal. *)
Inductive outcome :=
| Resolved (reference : nat) (resolved_roots : semantic_roots)
| Refused (why : refusal).

(** [resolution] is an outcome together with the digests whose objects were
    read, in order. *)
Record resolution := Resolution {
  objects_read : list nat;
  result : outcome
}.

(** [parse_manifest] is [SemanticStateManifestView::parse]: it accepts the
    canonical bytes of a manifest of schema epoch V1 and nothing else. *)
Definition parse_manifest (object : stored_object) : option state_manifest :=
  match object with
  | ManifestBytes manifest =>
      if Nat.eqb (schema_epoch manifest) semantic_schema_epoch_v1 then Some manifest else None
  | OtherBytes _ => None
  end.

(** [manifest_outcome] is steps 4 to 6 for a parsed manifest whose digest is
    the root: a legacy history root is refused first, then another profile,
    and otherwise the answer is the manifest's reference and roots. *)
Definition manifest_outcome (profile : nat) (manifest : state_manifest) : outcome :=
  match legacy_history_root manifest with
  | Some _ => Refused (UnknownRoot LegacyProvenance)
  | None =>
      if Nat.eqb (profile_id manifest) profile
      then Resolved (manifest_digest manifest) (roots manifest)
      else Refused (UnknownRoot ForeignProfile)
  end.

(** [resolve_direct] is [resolve_retained_state]: one ledger probe, then one
    read of the object stored under the root, the parse and the digest check,
    and [manifest_outcome]. *)
Definition resolve_direct (store : durable_store) (root : nat) : resolution :=
  if counts_semantic_root (ledger store) root then
    Resolution [root]
      (match objects store root with
       | ReadFailure => Refused StoreFailure
       | Absent => Refused CorruptRootCatalog
       | Present object =>
           match parse_manifest object with
           | None => Refused CorruptRootCatalog
           | Some manifest =>
               if Nat.eqb (manifest_digest manifest) root
               then manifest_outcome (store_profile store) manifest
               else Refused CorruptRootCatalog
           end
       end)
  else Resolution [] (Refused (UnknownRoot NotCurrentOrRetained)).

(** * The direct root is a function of the semantic roots *)

(** [root_is_function_of_semantic_roots root_of] says that [root_of] is a
    function of the semantic roots, and an injective one, on the stores of one
    profile whose current manifests are durable: two such stores have equal
    roots exactly when their current semantic roots are equal, whatever their
    retained histories, catalog generations, ledgers and objects. *)
Definition root_is_function_of_semantic_roots (root_of : durable_store -> nat) : Prop :=
  forall left right,
    store_profile left = store_profile right ->
    durable_manifest (store_profile left) (current_manifest left) ->
    durable_manifest (store_profile right) (current_manifest right) ->
    (root_of left = root_of right <->
     roots (current_manifest left) = roots (current_manifest right)).

Lemma durable_manifests_with_equal_roots_are_equal : forall profile left right,
  durable_manifest profile left -> durable_manifest profile right ->
  roots left = roots right -> left = right.
Proof.
  intros profile [schema_left profile_left roots_left legacy_left]
    [schema_right profile_right roots_right legacy_right]
    [left_schema [left_profile left_legacy]] [right_schema [right_profile right_legacy]]
    same_roots.
  simpl in *; subst; reflexivity.
Qed.

Theorem direct_root_is_function_of_semantic_roots :
  root_is_function_of_semantic_roots published_root.
Proof.
  intros left right same_profile left_durable right_durable; unfold published_root; split.
  - intros same_root; apply manifest_digest_injective in same_root; now rewrite same_root.
  - intros same_roots; rewrite same_profile in left_durable.
    now rewrite (durable_manifests_with_equal_roots_are_equal _ _ _
      left_durable right_durable same_roots).
Qed.

(** * Resolution is exact *)

Lemma resolve_direct_reads_at_most_the_root : forall store root,
  objects_read (resolve_direct store root) =
    (if counts_semantic_root (ledger store) root then [root] else []).
Proof.
  intros store root; unfold resolve_direct.
  destruct (counts_semantic_root (ledger store) root); reflexivity.
Qed.

Lemma resolve_direct_answers_exactly_the_stored_manifest :
  forall store root reference resolved,
  result (resolve_direct store root) = Resolved reference resolved <->
  counts_semantic_root (ledger store) root = true /\
  exists manifest, objects store root = Present (ManifestBytes manifest) /\
    manifest_digest manifest = root /\ durable_manifest (store_profile store) manifest /\
    reference = root /\ resolved = roots manifest.
Proof.
  intros store root reference resolved.
  unfold resolve_direct, parse_manifest, manifest_outcome.
  destruct (counts_semantic_root (ledger store) root) eqn:counted.
  2: { cbv beta iota delta [result]; split.
       - intros absurd; discriminate absurd.
       - intros [absurd _]; discriminate absurd. }
  destruct (objects store root) as [| |[manifest|tag]] eqn:stored;
    cbv beta iota delta [result].
  - split.
    + intros absurd; discriminate absurd.
    + intros [_ [other [present _]]]; discriminate present.
  - split.
    + intros absurd; discriminate absurd.
    + intros [_ [other [present _]]]; discriminate present.
  - destruct (Nat.eqb_spec (schema_epoch manifest) semantic_schema_epoch_v1) as [v1|not_v1];
      cbv beta iota.
    + destruct (Nat.eqb_spec (manifest_digest manifest) root) as [digest_is_root|digest_is_not_root];
        cbv beta iota.
      * destruct (legacy_history_root manifest) as [history|] eqn:legacy.
        -- split.
           ++ intros absurd; discriminate absurd.
           ++ intros [_ [other [present [_ [[_ [_ no_legacy]] _]]]]].
              injection present as <-; congruence.
        -- destruct (Nat.eqb_spec (profile_id manifest) (store_profile store))
             as [same_profile|other_profile]; cbv beta iota.
           ++ split.
              ** intros answered; injection answered as <- <-.
                 split; [reflexivity|].
                 exists manifest; repeat split; assumption.
              ** intros [_ [other [present [_ [_ [-> ->]]]]]].
                 injection present as <-; now rewrite digest_is_root.
           ++ split.
              ** intros absurd; discriminate absurd.
              ** intros [_ [other [present [_ [[_ [profile_matches _]] _]]]]].
                 injection present as <-; contradiction.
      * split.
        -- intros absurd; discriminate absurd.
        -- intros [_ [other [present [digest_matches _]]]].
           injection present as <-; contradiction.
    + split.
      * intros absurd; discriminate absurd.
      * intros [_ [other [present [_ [[schema_matches _] _]]]]].
        injection present as <-; contradiction.
  - split.
    + intros absurd; discriminate absurd.
    + intros [_ [other [present _]]]; discriminate present.
Qed.

Lemma coherent_store_answers_each_retained_state : forall store manifest,
  coherent store -> In manifest (retained_states store) ->
  resolve_direct store (manifest_digest manifest) =
    Resolution [manifest_digest manifest] (manifest_outcome (store_profile store) manifest).
Proof.
  intros store manifest [counted_exactly stored_exactly] retained.
  destruct (stored_exactly manifest retained) as [v1 stored].
  assert (counts_semantic_root (ledger store) (manifest_digest manifest) = true) as counted.
  { apply counted_exactly; exists manifest; split; [exact retained|reflexivity]. }
  unfold resolve_direct, parse_manifest.
  rewrite counted, stored; cbv beta iota.
  rewrite v1, Nat.eqb_refl; cbv beta iota.
  rewrite Nat.eqb_refl; reflexivity.
Qed.

Lemma coherent_store_refuses_every_other_root : forall store root,
  coherent store ->
  (forall manifest, In manifest (retained_states store) -> manifest_digest manifest <> root) ->
  resolve_direct store root = Resolution [] (Refused (UnknownRoot NotCurrentOrRetained)).
Proof.
  intros store root [counted_exactly _] named_by_none.
  unfold resolve_direct.
  destruct (counts_semantic_root (ledger store) root) eqn:counted.
  - apply counted_exactly in counted as [manifest [retained same]].
    exfalso; exact (named_by_none manifest retained same).
  - reflexivity.
Qed.

Theorem direct_resolution_is_exact : forall store root,
  objects_read (resolve_direct store root) =
    (if counts_semantic_root (ledger store) root then [root] else []) /\
  (forall reference resolved,
     result (resolve_direct store root) = Resolved reference resolved <->
     counts_semantic_root (ledger store) root = true /\
     exists manifest, objects store root = Present (ManifestBytes manifest) /\
       manifest_digest manifest = root /\ durable_manifest (store_profile store) manifest /\
       reference = root /\ resolved = roots manifest) /\
  (coherent store ->
     (forall manifest, In manifest (retained_states store) -> manifest_digest manifest = root ->
        resolve_direct store root =
          Resolution [root] (manifest_outcome (store_profile store) manifest)) /\
     ((forall manifest, In manifest (retained_states store) -> manifest_digest manifest <> root) ->
        resolve_direct store root = Resolution [] (Refused (UnknownRoot NotCurrentOrRetained)))).
Proof.
  intros store root.
  split; [apply resolve_direct_reads_at_most_the_root|].
  split; [intros reference resolved; apply resolve_direct_answers_exactly_the_stored_manifest|].
  intros coherent_store; split.
  - intros manifest retained named; subst root.
    exact (coherent_store_answers_each_retained_state store manifest coherent_store retained).
  - intros named_by_none.
    exact (coherent_store_refuses_every_other_root store root coherent_store named_by_none).
Qed.

(** * Direct and preserved sessions *)

Theorem direct_and_preserved_sessions_cannot_resolve_each_other :
  forall (lookup : Authority.owner -> nat -> option Consensus.bridge_binding)
    (refinement : Consensus.bridge_binding -> Prop) (owner : Authority.owner)
    (session : Consensus.identity_version) (store : durable_store) (root : nat)
    (binding : Consensus.bridge_binding),
  ~ Consensus.resolve lookup refinement legacy_root_digest owner Consensus.DirectManifestV1
      root binding /\
  (Consensus.binding_version binding = Consensus.DirectManifestV1 ->
     ~ Consensus.checked lookup refinement legacy_root_digest owner binding) /\
  (Consensus.resolve lookup refinement legacy_root_digest owner session root binding ->
     coherent store ->
     resolve_direct store root = Resolution [] (Refused (UnknownRoot NotCurrentOrRetained))) /\
  (forall reference resolved,
     result (resolve_direct store root) = Resolved reference resolved ->
     ~ Consensus.resolve lookup refinement legacy_root_digest owner session root binding) /\
  (forall manifest history reference resolved,
     legacy_history_root manifest = Some history ->
     result (resolve_direct store (manifest_digest manifest)) <> Resolved reference resolved).
Proof.
  intros lookup refinement owner session store root binding.
  assert (preserved_root_is_legacy : forall any_session,
    Consensus.resolve lookup refinement legacy_root_digest owner any_session root binding ->
    root = legacy_root_digest (Consensus.frozen_history_bytes (Consensus.compatibility binding))).
  { intros any_session [_ checked_binding certified _ _].
    rewrite <- certified.
    exact (Consensus.checked_root_comes_from_frozen_compatibility _ _ _ _ _ checked_binding). }
  split; [|split; [|split; [|split]]].
  - apply Consensus.mixed_session_cannot_resolve; discriminate.
  - intros direct_version checked_binding.
    destruct checked_binding as [_ [_ [preserved_version _]]]; congruence.
  - intros resolution coherent_store.
    apply coherent_store_refuses_every_other_root; [exact coherent_store|].
    intros manifest _ same.
    apply (manifest_digest_is_not_a_legacy_root manifest
      (Consensus.frozen_history_bytes (Consensus.compatibility binding))).
    rewrite same; exact (preserved_root_is_legacy session resolution).
  - intros reference resolved answered resolution.
    apply resolve_direct_answers_exactly_the_stored_manifest in answered
      as [_ [manifest [_ [digest_is_root _]]]].
    apply (manifest_digest_is_not_a_legacy_root manifest
      (Consensus.frozen_history_bytes (Consensus.compatibility binding))).
    rewrite digest_is_root; exact (preserved_root_is_legacy session resolution).
  - intros manifest history reference resolved legacy answered.
    apply resolve_direct_answers_exactly_the_stored_manifest in answered
      as [_ [stored_manifest [_ [same_digest [[_ [_ no_legacy]] _]]]]].
    apply manifest_digest_injective in same_digest; subst stored_manifest.
    congruence.
Qed.

End RSpaceDirectManifestIdentity.

Print Assumptions RSpaceDirectManifestIdentity.direct_root_is_function_of_semantic_roots.
Print Assumptions RSpaceDirectManifestIdentity.direct_resolution_is_exact.
Print Assumptions RSpaceDirectManifestIdentity.direct_and_preserved_sessions_cannot_resolve_each_other.
