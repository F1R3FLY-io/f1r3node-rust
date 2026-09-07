From Stdlib Require Import Bool Arith PeanoNat Lia Lists.List.
Import ListNotations.

Module RSpaceCallerAuthority.

Inductive backend := Memory | Lmdb | PathMapDag.

Record domain := Domain {
  selected_backend : backend;
  store_identity : nat;
  profile_identity : nat;
  semantic_epoch : nat
}.

Record owner := Owner {
  owner_domain : domain;
  incarnation : nat;
  current_root : nat;
  publication_generation : nat;
  live : bool;
  ready : bool;
  retained_roots : list nat;
  authenticated_aliases : list (nat * nat)
}.

Record mutation_permit := MutationPermit {
  permit_domain : domain;
  permit_incarnation : nat;
  expected_root : nat;
  expected_generation : nat
}.

Definition authorizes (o : owner) (p : mutation_permit) : Prop :=
  live o = true /\ ready o = true /\ permit_domain p = owner_domain o /\
  permit_incarnation p = incarnation o /\ expected_root p = current_root o /\
  expected_generation p = publication_generation o.

Definition view_permit (o : owner) : mutation_permit :=
  MutationPermit (owner_domain o) (incarnation o)
    (current_root o) (publication_generation o).

Definition close (o : owner) : owner :=
  Owner (owner_domain o) (incarnation o) (current_root o)
    (publication_generation o) false false (retained_roots o) (authenticated_aliases o).

Definition reopen (o : owner) (fresh_incarnation : nat) : owner :=
  Owner (owner_domain o) fresh_incarnation (current_root o)
    (publication_generation o) true true (retained_roots o) (authenticated_aliases o).

Inductive reopen_transition (o : owner) (observable : list nat) (fresh : nat) : owner -> Prop :=
| Reopen : live o = false -> In (incarnation o) observable -> ~ In fresh observable ->
    reopen_transition o observable fresh (reopen o fresh).

Definition published (o : owner) (target : nat) : owner :=
  Owner (owner_domain o) (incarnation o) target
    (S (publication_generation o)) (live o) (ready o)
    (retained_roots o) (authenticated_aliases o).

Theorem permit_requires_exact_authority : forall o p,
  authorizes o p ->
  permit_domain p = owner_domain o /\ permit_incarnation p = incarnation o /\
  expected_root p = current_root o /\
  expected_generation p = publication_generation o.
Proof. unfold authorizes; intuition. Qed.

Theorem not_ready_refuses_mutation : forall o p,
  ready o = false -> ~ authorizes o p.
Proof. unfold authorizes; intros; intuition congruence. Qed.

Theorem closed_owner_revokes_every_mutation_permit : forall o p,
  ~ authorizes (close o) p.
Proof. unfold authorizes, close; simpl; intuition congruence. Qed.

Theorem reopened_owner_rejects_old_incarnation : forall o p fresh,
  permit_incarnation p = incarnation o -> fresh <> incarnation o ->
  ~ authorizes (reopen o fresh) p.
Proof. unfold authorizes, reopen; simpl; intros; intuition congruence. Qed.

Theorem fresh_permit_requires_live_ready_owner : forall o,
  authorizes o (view_permit o) <-> live o = true /\ ready o = true.
Proof. unfold authorizes, view_permit; simpl; intuition. Qed.

Theorem reopen_rejects_every_observable_incarnation : forall o observable fresh next p,
  reopen_transition o observable fresh next ->
  In (permit_incarnation p) observable -> ~ authorizes next p.
Proof.
  intros o observable fresh next p step present authorized.
  inversion step; subst; unfold authorizes, reopen in authorized; simpl in authorized.
  destruct authorized as [_ [_ [_ [same _]]]].
  rewrite same in present; contradiction.
Qed.

Record mutable_view := MutableView {
  view_domain : domain;
  view_incarnation : nat;
  view_history_root : nat
}.

Definition attached (o : owner) (v : mutable_view) : Prop :=
  view_domain v = owner_domain o /\ view_incarnation v = incarnation o.

Inductive attach_spawned_view (o : owner) (source : mutable_view) : owner -> mutable_view -> Prop :=
| SpawnView : attached o source -> live o = true ->
    attach_spawned_view o source o
      (MutableView (owner_domain o) (incarnation o) (view_history_root source)).

Theorem spawn_attachment_reuses_owner_and_preserves_view_history : forall o source next spawned,
  attach_spawned_view o source next spawned ->
  next = o /\ attached o spawned /\ view_history_root spawned = view_history_root source.
Proof. intros; inversion H; subst; unfold attached; simpl; auto. Qed.

Theorem publication_invalidates_all_old_view_permits : forall o p target,
  authorizes o p -> ~ authorizes (published o target) p.
Proof. unfold authorizes, published; simpl; intros; intuition lia. Qed.

Theorem root_aba_does_not_revive_a_permit : forall o p target,
  authorizes o p ->
  ~ authorizes (published (published o target) (current_root o)) p.
Proof. unfold authorizes, published; simpl; intros; intuition lia. Qed.

Section HistoricalAdmission.

Variable semantic_accept : domain -> nat -> bool.

Record admitted_root := AdmittedRoot {
  admitted_domain : domain;
  admitted_target : nat;
  semantic_evidence : semantic_accept admitted_domain admitted_target = true
}.

Definition admit_historical_root (o : owner) (target : nat)
    : option admitted_root :=
  if in_dec Nat.eq_dec target (retained_roots o) then
    match Bool.bool_dec (semantic_accept (owner_domain o) target) true with
    | left checked => Some (AdmittedRoot (owner_domain o) target checked)
    | right _ => None
    end
  else None.

Definition activation_ready (o : owner) (p : mutation_permit)
    (a : admitted_root) : Prop :=
  authorizes o p /\ admitted_domain a = owner_domain o /\
  In (admitted_target a) (retained_roots o).

Inductive activate (o : owner) (p : mutation_permit)
    (a : admitted_root) : owner -> Prop :=
| Activate : activation_ready o p a ->
    activate o p a (published o (admitted_target a)).

Theorem retained_membership_does_not_skip_semantic_admission :
  forall o target,
  semantic_accept (owner_domain o) target = false -> admit_historical_root o target = None.
Proof.
  intros o target refused; unfold admit_historical_root.
  destruct (in_dec Nat.eq_dec target (retained_roots o)); [|reflexivity].
  destruct (Bool.bool_dec (semantic_accept (owner_domain o) target) true);
    [congruence|reflexivity].
Qed.

Theorem unknown_target_cannot_be_admitted : forall o target,
  ~ In target (retained_roots o) -> admit_historical_root o target = None.
Proof.
  intros; unfold admit_historical_root; destruct (in_dec Nat.eq_dec target (retained_roots o));
    [contradiction|reflexivity].
Qed.

Theorem activation_requires_exact_semantically_admitted_target :
  forall o p a successor,
  activate o p a successor ->
  authorizes o p /\ In (current_root successor) (retained_roots o) /\
  semantic_accept (owner_domain o) (current_root successor) = true.
Proof.
  intros o p a successor step; inversion step; subst.
  unfold activation_ready in H; destruct H as [permit [same present]].
  simpl; split; [exact permit|]. split; [exact present|].
  rewrite <- same; apply semantic_evidence.
Qed.

Theorem historical_activation_has_fresh_publication_generation :
  forall o p a successor,
  activate o p a successor ->
  publication_generation successor = S (publication_generation o) /\
  current_root successor = admitted_target a /\
  owner_domain successor = owner_domain o /\ incarnation successor = incarnation o.
Proof. intros; inversion H; subst; simpl; auto. Qed.

Theorem activation_is_deterministic : forall o p a left right,
  activate o p a left -> activate o p a right -> left = right.
Proof. intros; inversion H; inversion H0; reflexivity. Qed.

Inductive full_spawn (o : owner) (p : mutation_permit)
    (source : mutable_view) (a : admitted_root) : owner -> mutable_view -> Prop :=
| FullSpawn : forall activated spawned,
    attached o source -> admitted_target a = view_history_root source ->
    activate o p a activated -> attach_spawned_view activated source activated spawned ->
    full_spawn o p source a activated spawned.

Theorem full_spawn_activates_history_before_attaching : forall o p source a next spawned,
  full_spawn o p source a next spawned ->
  publication_generation next = S (publication_generation o) /\
  current_root next = view_history_root source /\
  owner_domain next = owner_domain o /\ incarnation next = incarnation o /\
  attached next spawned /\ view_history_root spawned = view_history_root source.
Proof.
  intros o p source a next spawned step; inversion step; subst.
  apply historical_activation_has_fresh_publication_generation in H1.
  apply spawn_attachment_reuses_owner_and_preserves_view_history in H2.
  intuition congruence.
Qed.

End HistoricalAdmission.

Record alias_catalog := AliasCatalog {
  catalog_domain : domain;
  catalog_entries : list (nat * nat)
}.

Definition alias_targets (external : nat) (catalog : alias_catalog) : list nat :=
  map snd (filter (fun entry => Nat.eqb external (fst entry)) (catalog_entries catalog)).

Definition resolves_alias (o : owner) (catalog : alias_catalog)
    (external target : nat) : Prop :=
  catalog_domain catalog = owner_domain o /\
  catalog_entries catalog = authenticated_aliases o /\ alias_targets external catalog = [target].

Theorem alias_resolution_is_exact_and_unambiguous : forall o c external a b,
  resolves_alias o c external a -> resolves_alias o c external b -> a = b.
Proof.
  unfold resolves_alias; intros o c external a b [_ [_ first]] [_ [_ second]].
  rewrite first in second; inversion second; reflexivity.
Qed.

Theorem missing_or_ambiguous_alias_refuses : forall o c external target,
  length (alias_targets external c) <> 1 -> ~ resolves_alias o c external target.
Proof.
  unfold resolves_alias; intros o c external target wrong_length [_ [_ resolved]].
  rewrite resolved in wrong_length; simpl in wrong_length; contradiction.
Qed.

Theorem foreign_catalog_cannot_resolve : forall o c external target,
  catalog_domain c <> owner_domain o -> ~ resolves_alias o c external target.
Proof. unfold resolves_alias; intuition. Qed.

Theorem unappointed_alias_catalog_cannot_resolve : forall o c external target,
  catalog_entries c <> authenticated_aliases o -> ~ resolves_alias o c external target.
Proof. unfold resolves_alias; intuition. Qed.

Record state_manifest := StateManifest {
  manifest_root : nat;
  legacy_provenance : option nat
}.

Definition resolve_manifest_alias (o : owner) (c : alias_catalog)
    (external : nat) (m : state_manifest) : Prop :=
  resolves_alias o c external (manifest_root m).

Theorem provenance_is_not_an_alias_certificate : forall o c external target a b,
  resolve_manifest_alias o c external (StateManifest target a) <->
  resolve_manifest_alias o c external (StateManifest target b).
Proof. reflexivity. Qed.

Record retained_reader := RetainedReader {
  reader_domain : domain;
  reader_root : nat;
  storage_generation : nat;
  lease_identity : nat
}.

Definition reader_pinned (leases : list retained_reader) (reader : retained_reader) :=
  In reader leases.

Definition reclaimable (leases : list retained_reader) (d : domain) (generation : nat) :=
  forall reader, In reader leases ->
    reader_domain reader <> d \/ storage_generation reader <> generation.

Theorem pinned_generation_cannot_be_reclaimed : forall leases reader,
  reader_pinned leases reader ->
  ~ reclaimable leases (reader_domain reader) (storage_generation reader).
Proof.
  unfold reader_pinned, reclaimable; intros leases reader present allowed.
  specialize (allowed reader present); intuition.
Qed.

Theorem owner_teardown_preserves_retained_readers : forall o leases reader p,
  reader_pinned leases reader ->
  ~ authorizes (close o) p /\ reader_pinned leases reader.
Proof. intros; split; [apply closed_owner_revokes_every_mutation_permit|assumption]. Qed.

Record five_journals (Data Continuations Content Sources Joins : Type) := FiveJournals {
  journal_data : Data;
  journal_continuations : Continuations;
  journal_content : Content;
  journal_sources : Sources;
  journal_joins : Joins
}.

Section SoftRestore.

Context {Key Value Log Counter Data Continuations Content Sources Joins : Type}.
Let Journals := five_journals Data Continuations Content Sources Joins.

Record soft_payload := SoftPayload {
  captured_cache : Key -> option Value;
  captured_log : Log;
  captured_counter : Counter;
  captured_journals : Journals
}.

Definition overlay (payload : soft_payload) (current_history : Key -> Value) (key : Key)
    : Value :=
  match captured_cache payload key with
  | Some value => value
  | None => current_history key
  end.

Record restored_view := RestoredView {
  restored_values : Key -> Value;
  restored_log : Log;
  restored_counter : Counter;
  restored_journals : Journals
}.

Definition restore (payload : soft_payload) (current_history : Key -> Value)
    (journals : Journals) : restored_view :=
  RestoredView (overlay payload current_history) (captured_log payload)
    (captured_counter payload) journals.

Variable authenticated_view_history : mutable_view -> (Key -> Value) -> Prop.
Variable complete_journal_refinement : soft_payload -> (Key -> Value) -> Journals -> Prop.

Inductive permitted_restore (o : owner) (p : mutation_permit)
    (v : mutable_view) (payload : soft_payload) (current_history : Key -> Value)
    (journals : Journals) :
    owner -> restored_view -> Prop :=
| Restore : authorizes o p -> attached o v ->
    authenticated_view_history v current_history ->
    complete_journal_refinement payload current_history journals ->
    permitted_restore o p v payload current_history journals o
      (restore payload current_history journals).

Theorem every_legacy_overlay_with_refined_journals_has_a_permitted_restore :
  forall o p v payload current_history,
  authorizes o p -> attached o v -> authenticated_view_history v current_history ->
  (exists journals, complete_journal_refinement payload current_history journals) ->
  exists journals, permitted_restore o p v payload current_history journals o
    (restore payload current_history journals).
Proof.
  intros o p v payload current_history permit same history [journals valid].
  exists journals; constructor; assumption.
Qed.

Theorem soft_restore_preserves_durable_authority_log_and_counter :
  forall o p v payload current_history journals successor restored,
  permitted_restore o p v payload current_history journals successor restored ->
  successor = o /\ restored_log restored = captured_log payload /\
  restored_counter restored = captured_counter payload /\
  forall key, restored_values restored key = overlay payload current_history key.
Proof. intros; inversion H; subst; simpl; auto. Qed.

Theorem soft_restore_requires_receiving_history_and_complete_journals :
  forall o p v payload history journals successor restored,
  permitted_restore o p v payload history journals successor restored ->
  attached o v /\ authenticated_view_history v history /\
  complete_journal_refinement payload history (restored_journals restored).
Proof. intros; inversion H; subst; simpl; auto. Qed.

Theorem captured_values_override_current_history : forall payload history key value,
  captured_cache payload key = Some value -> overlay payload history key = value.
Proof. intros; unfold overlay; rewrite H; reflexivity. Qed.

Theorem uncaptured_values_use_receiving_history : forall payload history key,
  captured_cache payload key = None -> overlay payload history key = history key.
Proof. intros; unfold overlay; rewrite H; reflexivity. Qed.

Theorem payload_transfer_never_transfers_mutation_authority :
  forall o p v payload history journals successor restored,
  permitted_restore o p v payload history journals successor restored -> authorizes o p.
Proof. intros; inversion H; assumption. Qed.

End SoftRestore.

Definition rebased_addition (base target : nat) := target - base.
Definition rebased_removal (base target : nat) := base - target.
Definition apply_count_delta (base addition removal : nat) := base - removal + addition.

Theorem rebased_journal_reconstructs_exact_restored_count : forall base target,
  apply_count_delta base (rebased_addition base target) (rebased_removal base target) = target.
Proof. unfold apply_count_delta, rebased_addition, rebased_removal; intros; lia. Qed.

Theorem rebased_journal_has_no_underflow_or_intermediate_overflow : forall base target limit,
  base <= limit -> target <= limit ->
  rebased_removal base target <= base /\ rebased_addition base target <= limit /\
  base - rebased_removal base target <= limit /\
  apply_count_delta base (rebased_addition base target) (rebased_removal base target) <= limit.
Proof. unfold apply_count_delta, rebased_addition, rebased_removal; intros; lia. Qed.

Print Assumptions permit_requires_exact_authority.
Print Assumptions not_ready_refuses_mutation.
Print Assumptions closed_owner_revokes_every_mutation_permit.
Print Assumptions reopened_owner_rejects_old_incarnation.
Print Assumptions fresh_permit_requires_live_ready_owner.
Print Assumptions reopen_rejects_every_observable_incarnation.
Print Assumptions spawn_attachment_reuses_owner_and_preserves_view_history.
Print Assumptions publication_invalidates_all_old_view_permits.
Print Assumptions root_aba_does_not_revive_a_permit.
Print Assumptions retained_membership_does_not_skip_semantic_admission.
Print Assumptions unknown_target_cannot_be_admitted.
Print Assumptions activation_requires_exact_semantically_admitted_target.
Print Assumptions historical_activation_has_fresh_publication_generation.
Print Assumptions activation_is_deterministic.
Print Assumptions full_spawn_activates_history_before_attaching.
Print Assumptions alias_resolution_is_exact_and_unambiguous.
Print Assumptions missing_or_ambiguous_alias_refuses.
Print Assumptions foreign_catalog_cannot_resolve.
Print Assumptions unappointed_alias_catalog_cannot_resolve.
Print Assumptions provenance_is_not_an_alias_certificate.
Print Assumptions pinned_generation_cannot_be_reclaimed.
Print Assumptions owner_teardown_preserves_retained_readers.
Print Assumptions every_legacy_overlay_with_refined_journals_has_a_permitted_restore.
Print Assumptions soft_restore_preserves_durable_authority_log_and_counter.
Print Assumptions soft_restore_requires_receiving_history_and_complete_journals.
Print Assumptions captured_values_override_current_history.
Print Assumptions uncaptured_values_use_receiving_history.
Print Assumptions payload_transfer_never_transfers_mutation_authority.
Print Assumptions rebased_journal_reconstructs_exact_restored_count.
Print Assumptions rebased_journal_has_no_underflow_or_intermediate_overflow.

End RSpaceCallerAuthority.
