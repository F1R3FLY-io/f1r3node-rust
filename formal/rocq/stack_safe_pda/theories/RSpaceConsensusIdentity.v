From Stdlib Require Import Bool Arith PeanoNat Lia Lists.List.
From StackSafePDA Require Import RSpaceCallerAuthority.
Import ListNotations.

Module RSpaceConsensusIdentity.
Module Authority := RSpaceCallerAuthority.RSpaceCallerAuthority.

Definition ordered_update (state : nat * nat) (operation : bool) : nat * nat :=
  if operation then (fst state, 2) else (1, snd state).

Definition ordered_result (operations : list bool) : nat * nat :=
  fold_left ordered_update operations (0, 0).

Definition legacy_update (_ : nat) (operation : bool) : nat :=
  if operation then 2 else 1.

Definition legacy_result (operations : list bool) : nat :=
  fold_left legacy_update operations 0.

Theorem ordered_insertions_commute :
  ordered_result [false; true] = ordered_result [true; false].
Proof. reflexivity. Qed.

Theorem legacy_alias_insertions_do_not_commute :
  legacy_result [false; true] <> legacy_result [true; false].
Proof. discriminate. Qed.

Theorem no_unconditional_ordered_state_projection :
  ~ exists project : nat * nat -> nat,
    forall operations, project (ordered_result operations) = legacy_result operations.
Proof.
  intros [project law].
  pose proof (law [false; true]) as first.
  pose proof (law [true; false]) as second.
  cbn in first, second; congruence.
Qed.

(* DirectManifestV1 is the durable store's own identity, the digest of its
   semantic manifest (RSpaceDirectManifestIdentity.v); the coupled bridge below
   never checks or resolves it. *)
Inductive identity_version :=
| PreservedLegacyV1
| DirectManifestV1
| DirectManifestEpoch (epoch : nat)
| UnknownVersion (tag : nat).

Record semantic_identity := SemanticIdentity {
  manifest_reference : nat;
  schema_version : nat;
  profile : nat;
  data_root : nat;
  continuation_root : nat;
  join_root : nat;
  inherited_provenance : option nat
}.

Definition same_semantics (left right : semantic_identity) : Prop :=
  schema_version left = schema_version right /\ profile left = profile right /\
  data_root left = data_root right /\ continuation_root left = continuation_root right /\
  join_root left = join_root right.

Record compatibility_facet := CompatibilityFacet {
  facet_reference : nat;
  frozen_history_bytes : list nat
}.

Record bridge_binding := BridgeBinding {
  binding_reference : nat;
  binding_domain : Authority.domain;
  binding_version : identity_version;
  semantic : semantic_identity;
  compatibility : compatibility_facet;
  certified_legacy_root : nat
}.

Definition coupled_equivalent (left right : bridge_binding) : Prop :=
  binding_domain left = binding_domain right /\
  binding_version left = binding_version right /\
  same_semantics (semantic left) (semantic right) /\
  frozen_history_bytes (compatibility left) = frozen_history_bytes (compatibility right).

Section CheckedBridge.

Variable binding_lookup : Authority.owner -> nat -> option bridge_binding.
Variable legacy_refinement : bridge_binding -> Prop.
Variable frozen_root : list nat -> nat.
Variable frozen_read : list nat -> nat -> list nat.

Definition authenticated_binding (o : Authority.owner) (b : bridge_binding) : Prop :=
  binding_lookup o (binding_reference b) = Some b.

Definition owner_aliases (o : Authority.owner) : Authority.alias_catalog :=
  Authority.AliasCatalog (Authority.owner_domain o) (Authority.authenticated_aliases o).

Definition checked (o : Authority.owner) (b : bridge_binding) : Prop :=
  authenticated_binding o b /\ binding_domain b = Authority.owner_domain o /\
  binding_version b = PreservedLegacyV1 /\ schema_version (semantic b) = 1 /\
  profile (semantic b) = Authority.profile_identity (Authority.owner_domain o) /\
  certified_legacy_root b = frozen_root (frozen_history_bytes (compatibility b)) /\
  legacy_refinement b.

Definition selected (o : Authority.owner) (b : bridge_binding) : Prop :=
  checked o b /\ Authority.current_root o = binding_reference b.

Inductive resolve (o : Authority.owner) (session : identity_version) (root : nat)
    (b : bridge_binding) : Prop :=
| Resolve : session = PreservedLegacyV1 -> checked o b ->
    certified_legacy_root b = root ->
    Authority.resolves_alias o (owner_aliases o) root (binding_reference b) ->
    In (binding_reference b) (Authority.retained_roots o) -> resolve o session root b.

Theorem checked_requires_exact_authentication_and_refinement : forall o b,
  checked o b -> authenticated_binding o b /\ legacy_refinement b.
Proof. unfold checked; intuition. Qed.

Theorem authenticated_reference_identifies_exact_binding : forall o left right,
  authenticated_binding o left -> authenticated_binding o right ->
  binding_reference left = binding_reference right -> left = right.
Proof. unfold authenticated_binding; intros; congruence. Qed.

Theorem checked_preserves_domain_profile_schema_and_version : forall o b,
  checked o b -> binding_domain b = Authority.owner_domain o /\
  profile (semantic b) = Authority.profile_identity (Authority.owner_domain o) /\
  schema_version (semantic b) = 1 /\ binding_version b = PreservedLegacyV1.
Proof. unfold checked; intuition. Qed.

Theorem checked_root_comes_from_frozen_compatibility : forall o b,
  checked o b ->
  certified_legacy_root b = frozen_root (frozen_history_bytes (compatibility b)).
Proof. unfold checked; intuition. Qed.

Theorem absent_exact_authentication_refuses : forall o b,
  ~ authenticated_binding o b -> ~ checked o b.
Proof. unfold checked; intuition. Qed.

Theorem absent_refinement_refuses : forall o b,
  ~ legacy_refinement b -> ~ checked o b.
Proof. unfold checked; intuition. Qed.

Theorem direct_manifest_identity_is_not_activated : forall o b epoch,
  binding_version b = DirectManifestEpoch epoch -> ~ checked o b.
Proof. unfold checked; intros; intuition congruence. Qed.

Theorem unknown_binding_version_is_not_activated : forall o b tag,
  binding_version b = UnknownVersion tag -> ~ checked o b.
Proof. unfold checked; intros; intuition congruence. Qed.

Theorem mixed_session_cannot_resolve : forall o session root b,
  session <> PreservedLegacyV1 -> ~ resolve o session root b.
Proof. intros o session root b mismatch resolution; inversion resolution; contradiction. Qed.

Theorem alias_requires_appointed_retained_coupled_target : forall o session root b,
  resolve o session root b -> checked o b /\ certified_legacy_root b = root /\
  Authority.resolves_alias o (owner_aliases o) root (binding_reference b) /\
  In (binding_reference b) (Authority.retained_roots o).
Proof. intros; inversion H; subst; auto. Qed.

Theorem unappointed_provenance_cannot_resolve : forall o root b,
  inherited_provenance (semantic b) = Some root ->
  ~ Authority.resolves_alias o (owner_aliases o) root (binding_reference b) ->
  ~ resolve o PreservedLegacyV1 root b.
Proof. intros o root b provenance missing resolution; inversion resolution; contradiction. Qed.

Theorem alias_resolution_identifies_one_exact_binding : forall o session root left right,
  resolve o session root left -> resolve o session root right -> left = right.
Proof.
  intros o session root left right left_ok right_ok.
  inversion left_ok as [session_left checked_left root_left alias_left retained_left].
  inversion right_ok as [session_right checked_right root_right alias_right retained_right].
  apply (authenticated_reference_identifies_exact_binding o).
  - exact (proj1 checked_left).
  - exact (proj1 checked_right).
  - eapply Authority.alias_resolution_is_exact_and_unambiguous; eassumption.
Qed.

Theorem unretained_binding_cannot_resolve : forall o session root b,
  ~ In (binding_reference b) (Authority.retained_roots o) -> ~ resolve o session root b.
Proof. intros o session root b missing resolution; inversion resolution; contradiction. Qed.

Theorem equivalent_checked_bindings_preserve_root_and_reads : forall o left right,
  checked o left -> checked o right -> coupled_equivalent left right ->
  certified_legacy_root left = certified_legacy_root right /\
  forall query, frozen_read (frozen_history_bytes (compatibility left)) query =
    frozen_read (frozen_history_bytes (compatibility right)) query.
Proof.
  intros o left right left_ok right_ok [_ [_ [_ same]]].
  pose proof (checked_root_comes_from_frozen_compatibility _ _ left_ok) as left_root.
  pose proof (checked_root_comes_from_frozen_compatibility _ _ right_ok) as right_root.
  rewrite same in left_root; split; [congruence|intros; now rewrite same].
Qed.

Definition immutable_pin := bridge_binding.

Inductive replace_representative (o : Authority.owner) (old new : bridge_binding)
    (pin : immutable_pin) : bridge_binding -> immutable_pin -> Prop :=
| ReplaceRepresentative : checked o old -> checked o new ->
    coupled_equivalent old new ->
    In (binding_reference new) (Authority.retained_roots o) ->
    replace_representative o old new pin new pin.

Theorem replacement_preserves_coupled_observations_and_exact_pin :
  forall o old new pin representative next_pin,
  replace_representative o old new pin representative next_pin ->
  representative = new /\ next_pin = pin /\
  certified_legacy_root old = certified_legacy_root representative /\
  forall query, frozen_read (frozen_history_bytes (compatibility old)) query =
    frozen_read (frozen_history_bytes (compatibility representative)) query.
Proof.
  intros o old new pin representative next_pin step; inversion step; subst.
  repeat split; auto.
  - exact (proj1 (equivalent_checked_bindings_preserve_root_and_reads _ _ _ H H0 H1)).
  - exact (proj2 (equivalent_checked_bindings_preserve_root_and_reads _ _ _ H H0 H1)).
Qed.

Variable successful_refined_transition : bridge_binding -> bridge_binding -> Prop.

Definition installed_catalog (o : Authority.owner) (target : bridge_binding)
    (retained : list nat) (aliases : list (nat * nat)) : Authority.owner :=
  Authority.Owner (Authority.owner_domain o) (Authority.incarnation o)
    (binding_reference target) (S (Authority.publication_generation o))
    (Authority.live o) (Authority.ready o) retained aliases.

Inductive publish (o : Authority.owner) (permit : Authority.mutation_permit)
    (old target : bridge_binding) : Authority.owner -> nat -> Prop :=
| Publish : forall retained aliases representative,
    Authority.authorizes o permit -> selected o old ->
    successful_refined_transition old target ->
    checked (installed_catalog o target retained aliases) target ->
    In (binding_reference target) retained ->
    resolve (installed_catalog o target retained aliases) PreservedLegacyV1
      (certified_legacy_root target) representative ->
    coupled_equivalent target representative ->
    publish o permit old target (installed_catalog o target retained aliases)
      (certified_legacy_root target).

Theorem publication_selects_one_binding_and_exposes_legacy_root :
  forall o permit old target next exposed,
  publish o permit old target next exposed ->
  Authority.current_root next = binding_reference target /\
  exposed = frozen_root (frozen_history_bytes (compatibility target)) /\
  Authority.owner_domain next = Authority.owner_domain o /\
  Authority.publication_generation next = S (Authority.publication_generation o).
Proof.
  intros o permit old target next exposed step; inversion step; subst; simpl.
  repeat split; auto; eapply checked_root_comes_from_frozen_compatibility; eassumption.
Qed.

Theorem completed_publication_is_closed_under_selection :
  forall o permit old target next exposed,
  publish o permit old target next exposed -> selected next target.
Proof. intros; inversion H; subst; split; [assumption|reflexivity]. Qed.

Theorem completed_publication_has_a_usable_equivalent_alias :
  forall o permit old target next exposed,
  publish o permit old target next exposed -> exists representative,
  resolve next PreservedLegacyV1 exposed representative /\
  coupled_equivalent target representative /\ In (binding_reference target) (Authority.retained_roots next).
Proof. intros; inversion H; subst; simpl; eauto. Qed.

Theorem publication_requires_refined_transition_not_just_semantic_admission :
  forall o permit old target next exposed,
  publish o permit old target next exposed ->
  selected o old /\ checked next target /\ successful_refined_transition old target.
Proof. intros; inversion H; subst; auto. Qed.

Theorem publication_invalidates_old_permit : forall o permit old target next exposed,
  publish o permit old target next exposed -> ~ Authority.authorizes next permit.
Proof.
  intros o permit old target next exposed step; inversion step; subst.
  unfold Authority.authorizes, installed_catalog in *; simpl in *; intuition lia.
Qed.

Inductive recover (durable : Authority.owner) (binding : bridge_binding)
    (observable : list nat) (fresh : nat) : Authority.owner -> bridge_binding -> Prop :=
| Recover : forall reopened representative,
    selected durable binding ->
    Authority.reopen_transition (Authority.close durable) observable fresh reopened ->
    checked reopened binding ->
    resolve reopened PreservedLegacyV1 (certified_legacy_root binding) representative ->
    coupled_equivalent binding representative ->
    recover durable binding observable fresh reopened binding.

Theorem recovery_preserves_exact_selected_binding_and_legacy_observations :
  forall durable binding observable fresh reopened recovered,
  recover durable binding observable fresh reopened recovered ->
  recovered = binding /\ selected reopened recovered /\
  certified_legacy_root recovered = certified_legacy_root binding /\
  forall query, frozen_read (frozen_history_bytes (compatibility recovered)) query =
    frozen_read (frozen_history_bytes (compatibility binding)) query.
Proof.
  intros durable binding observable fresh reopened recovered step; inversion step; subst.
  match goal with transition : Authority.reopen_transition _ _ _ _ |- _ =>
    inversion transition; subst end.
  unfold selected in *; simpl in *; intuition.
Qed.

Theorem missing_recovery_binding_cannot_fall_back : forall durable binding observable fresh,
  ~ checked (Authority.reopen durable fresh) binding ->
  ~ exists reopened recovered, recover durable binding observable fresh reopened recovered.
Proof.
  intros durable binding observable fresh missing [reopened [recovered step]].
  inversion step; subst.
  match goal with transition : Authority.reopen_transition _ _ _ _ |- _ =>
    inversion transition; subst end.
  apply missing; assumption.
Qed.

Theorem recovered_owner_rejects_every_observable_permit :
  forall durable binding observable fresh reopened recovered permit,
  recover durable binding observable fresh reopened recovered ->
  In (Authority.permit_incarnation permit) observable -> ~ Authority.authorizes reopened permit.
Proof.
  intros durable binding observable fresh reopened recovered permit step present.
  inversion step; subst; eapply Authority.reopen_rejects_every_observable_incarnation; eassumption.
Qed.

Theorem recovered_owner_has_a_usable_equivalent_alias :
  forall durable binding observable fresh reopened recovered,
  recover durable binding observable fresh reopened recovered -> exists representative,
  resolve reopened PreservedLegacyV1 (certified_legacy_root recovered) representative /\
  coupled_equivalent recovered representative.
Proof. intros; inversion H; subst; eauto. Qed.

End CheckedBridge.

Definition witness_domain := Authority.Domain Authority.PathMapDag 1 1 1.
Definition witness_semantic := SemanticIdentity 10 1 1 11 12 13 None.
Definition witness_binding (facet root : nat) :=
  BridgeBinding 20 witness_domain PreservedLegacyV1 witness_semantic
    (CompatibilityFacet facet [root]) root.

Theorem same_semantic_manifest_does_not_imply_coupled_equivalence :
  semantic (witness_binding 31 1) = semantic (witness_binding 32 2) /\
  ~ coupled_equivalent (witness_binding 31 1) (witness_binding 32 2).
Proof. split; [reflexivity|unfold coupled_equivalent; simpl; intuition discriminate]. Qed.

Print Assumptions ordered_insertions_commute.
Print Assumptions legacy_alias_insertions_do_not_commute.
Print Assumptions no_unconditional_ordered_state_projection.
Print Assumptions checked_requires_exact_authentication_and_refinement.
Print Assumptions authenticated_reference_identifies_exact_binding.
Print Assumptions checked_preserves_domain_profile_schema_and_version.
Print Assumptions checked_root_comes_from_frozen_compatibility.
Print Assumptions absent_exact_authentication_refuses.
Print Assumptions absent_refinement_refuses.
Print Assumptions direct_manifest_identity_is_not_activated.
Print Assumptions unknown_binding_version_is_not_activated.
Print Assumptions mixed_session_cannot_resolve.
Print Assumptions alias_requires_appointed_retained_coupled_target.
Print Assumptions unappointed_provenance_cannot_resolve.
Print Assumptions alias_resolution_identifies_one_exact_binding.
Print Assumptions unretained_binding_cannot_resolve.
Print Assumptions equivalent_checked_bindings_preserve_root_and_reads.
Print Assumptions replacement_preserves_coupled_observations_and_exact_pin.
Print Assumptions publication_selects_one_binding_and_exposes_legacy_root.
Print Assumptions completed_publication_is_closed_under_selection.
Print Assumptions completed_publication_has_a_usable_equivalent_alias.
Print Assumptions publication_requires_refined_transition_not_just_semantic_admission.
Print Assumptions publication_invalidates_old_permit.
Print Assumptions recovery_preserves_exact_selected_binding_and_legacy_observations.
Print Assumptions missing_recovery_binding_cannot_fall_back.
Print Assumptions recovered_owner_rejects_every_observable_permit.
Print Assumptions recovered_owner_has_a_usable_equivalent_alias.
Print Assumptions same_semantic_manifest_does_not_imply_coupled_equivalence.

End RSpaceConsensusIdentity.
