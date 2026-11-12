From Stdlib Require Import Bool Arith PeanoNat Lia Lists.List.
From StackSafePDA Require Import ChannelImage.
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

(** A reset or a spawn is local to its view and writes nothing.  A checkpoint names
    the view's own base and publishes the successor of that base at the owner's next
    publication generation, whatever the owner's current root is.

    [view_checkpoint o v step next]: the view [v] is attached to the live and ready
    owner [o] and its base [view_history_root v] is retained by [o]; [step] maps a
    base to the root of that base with the view's journal applied.  No premise
    mentions the current root or the generation of [o]. *)
Inductive view_checkpoint (o : owner) (v : mutable_view) (step : nat -> nat) :
    owner -> Prop :=
| ViewCheckpoint : attached o v -> In (view_history_root v) (retained_roots o) ->
    live o = true -> ready o = true ->
    view_checkpoint o v step (published o (step (view_history_root v))).

(** Two owners that agree on everything except their current root and generation. *)
Definition differ_only_in_current_root_and_generation (left right : owner) : Prop :=
  owner_domain left = owner_domain right /\ incarnation left = incarnation right /\
  live left = live right /\ ready left = ready right /\
  retained_roots left = retained_roots right /\
  authenticated_aliases left = authenticated_aliases right.

(** A checkpoint rule under which two owners that differ only in current root and
    generation publish the same target for the same view and step. *)
Definition checkpoint_target_is_independent_of_current_root
    (checkpoint : owner -> mutable_view -> (nat -> nat) -> owner -> Prop) : Prop :=
  forall left right v step left_next right_next,
  differ_only_in_current_root_and_generation left right ->
  checkpoint left v step left_next -> checkpoint right v step right_next ->
  current_root left_next = current_root right_next.

(** [view_reset o v target next reset_view]: the view [v] is attached to [o] and
    [target] passes [admit_historical_root]; the owner is left as it was, and the view
    keeps its domain and incarnation and takes [target] as its base. *)
Inductive view_reset (o : owner) (v : mutable_view) (target : nat) :
    owner -> mutable_view -> Prop :=
| ViewReset : forall admitted, attached o v ->
    admit_historical_root o target = Some admitted ->
    view_reset o v target o (MutableView (view_domain v) (view_incarnation v) target).

(** A reset rule that leaves the owner unchanged and yields a view attached at the
    reset's target. *)
Definition reset_leaves_owner_unchanged
    (reset : owner -> mutable_view -> nat -> owner -> mutable_view -> Prop) : Prop :=
  forall o v target next reset_view,
  reset o v target next reset_view ->
  next = o /\ attached o reset_view /\ view_history_root reset_view = target.

(** A full spawn attaches a view to the same owner and resets it to the source's
    history root. *)
Inductive full_spawn (o : owner) (source : mutable_view) : owner -> mutable_view -> Prop :=
| FullSpawn : forall shared attached_view next spawned,
    attach_spawned_view o source shared attached_view ->
    view_reset shared attached_view (view_history_root source) next spawned ->
    full_spawn o source next spawned.

Theorem view_checkpoint_publishes_successor_of_view_base : forall o v step next,
  view_checkpoint o v step next ->
  current_root next = step (view_history_root v) /\
  publication_generation next = S (publication_generation o) /\
  owner_domain next = owner_domain o /\ incarnation next = incarnation o.
Proof. intros o v step next checkpoint; destruct checkpoint; simpl; repeat split. Qed.

Theorem view_reset_leaves_owner_unchanged : reset_leaves_owner_unchanged view_reset.
Proof.
  intros o v target next reset_view reset.
  destruct reset as [admitted [same_domain same_incarnation] _].
  split; [reflexivity|].
  split; [split; [exact same_domain|exact same_incarnation]|reflexivity].
Qed.

(** The conclusion does not use the premise that the two owners differ only in current
    root and generation; the premise is kept because a rule that reads the current root
    fails even this comparison, between owners that differ in nothing else. *)
Theorem view_checkpoint_target_is_independent_of_current_root :
  checkpoint_target_is_independent_of_current_root view_checkpoint.
Proof.
  intros left right v step left_next right_next _ left_checkpoint right_checkpoint.
  apply view_checkpoint_publishes_successor_of_view_base in left_checkpoint
    as [left_target _].
  apply view_checkpoint_publishes_successor_of_view_base in right_checkpoint
    as [right_target _].
  rewrite left_target, right_target; reflexivity.
Qed.

Theorem full_spawn_attaches_without_publication : forall o source next spawned,
  full_spawn o source next spawned ->
  next = o /\ attached o spawned /\ view_history_root spawned = view_history_root source.
Proof.
  intros o source next spawned spawn.
  destruct spawn as [shared attached_view reset_owner reset_view attachment reset].
  apply spawn_attachment_reuses_owner_and_preserves_view_history in attachment
    as [same_owner _].
  subst shared; exact (view_reset_leaves_owner_unchanged _ _ _ _ _ reset).
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
Print Assumptions view_checkpoint_publishes_successor_of_view_base.
Print Assumptions view_reset_leaves_owner_unchanged.
Print Assumptions view_checkpoint_target_is_independent_of_current_root.
Print Assumptions full_spawn_attaches_without_publication.
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

Section CountJournalRebase.

Context {Key : Type}.

Definition count_overlay (captured : Key -> option nat) (base : Key -> nat) (key : Key) : nat :=
  match captured key with
  | Some count => count
  | None => base key
  end.

Definition rebased_count_addition (captured : Key -> option nat) (base : Key -> nat)
    (key : Key) : nat :=
  rebased_addition (base key) (count_overlay captured base key).

Definition rebased_count_removal (captured : Key -> option nat) (base : Key -> nat)
    (key : Key) : nat :=
  rebased_removal (base key) (count_overlay captured base key).

Definition apply_count_journal (base addition removal : Key -> nat) (key : Key) : nat :=
  apply_count_delta (base key) (addition key) (removal key).

Definition exact_count_journal (base target addition removal : Key -> nat) : Prop :=
  forall key, addition key = rebased_addition (base key) (target key) /\
    removal key = rebased_removal (base key) (target key).

Theorem rebased_counts_apply_to_target : forall captured base key,
  apply_count_journal base (rebased_count_addition captured base)
    (rebased_count_removal captured base) key = count_overlay captured base key.
Proof.
  intros; unfold apply_count_journal, rebased_count_addition, rebased_count_removal.
  apply rebased_journal_reconstructs_exact_restored_count.
Qed.

Theorem rebased_counts_are_disjoint : forall captured base key,
  rebased_count_addition captured base key = 0 \/ rebased_count_removal captured base key = 0.
Proof.
  intros; unfold rebased_count_addition, rebased_count_removal, rebased_addition,
    rebased_removal; lia.
Qed.

Theorem rebased_counts_bounded : forall captured base limit,
  (forall key, base key <= limit) ->
  (forall key count, captured key = Some count -> count <= limit) ->
  forall key,
    rebased_count_removal captured base key <= base key /\
    rebased_count_addition captured base key <= limit /\
    base key - rebased_count_removal captured base key <= limit /\
    apply_count_journal base (rebased_count_addition captured base)
      (rebased_count_removal captured base) key <= limit.
Proof.
  intros captured base limit base_bound captured_bound key.
  unfold apply_count_journal, rebased_count_addition, rebased_count_removal.
  apply rebased_journal_has_no_underflow_or_intermediate_overflow; [apply base_bound|].
  unfold count_overlay; destruct (captured key) as [count|] eqn:cell;
    [exact (captured_bound key count cell)|apply base_bound].
Qed.

Theorem rebase_is_zero_outside_overlay_domain : forall captured base key,
  captured key = None ->
  rebased_count_addition captured base key = 0 /\ rebased_count_removal captured base key = 0.
Proof.
  intros captured base key uncaptured.
  unfold rebased_count_addition, rebased_count_removal, rebased_addition, rebased_removal,
    count_overlay.
  rewrite uncaptured; simpl; lia.
Qed.

Theorem same_base_rebase_is_identity_for_exact_journals :
  forall captured capture_base receiving_base addition removal,
  (forall key, receiving_base key = capture_base key) ->
  exact_count_journal capture_base (count_overlay captured capture_base) addition removal ->
  forall key,
    rebased_count_addition captured receiving_base key = addition key /\
    rebased_count_removal captured receiving_base key = removal key.
Proof.
  intros captured capture_base receiving_base addition removal same exact key.
  destruct (exact key) as [added removed]; rewrite added, removed.
  unfold rebased_count_addition, rebased_count_removal, count_overlay.
  destruct (captured key); rewrite (same key); split; reflexivity.
Qed.

End CountJournalRebase.

Print Assumptions rebased_counts_apply_to_target.
Print Assumptions rebased_counts_are_disjoint.
Print Assumptions rebased_counts_bounded.
Print Assumptions rebase_is_zero_outside_overlay_domain.
Print Assumptions same_base_rebase_is_identity_for_exact_journals.

Section FiveJournalRebase.

Context {Key Ref Bytes Log Counter : Type}.
Variable ref_eq_dec : forall one other : Ref, {one = other} + {one <> other}.
Variable source_bytes : Ref -> Bytes.

Record hot_value := HotValue {
  data_count : nat;
  continuation_count : nat;
  continuation_content : option Ref;
  join_count : nat
}.

Definition count_delta := (nat * nat)%type.
Definition content_transition := (option Ref * option Ref)%type.
Definition hot_journals := five_journals (Key -> count_delta) (Key -> count_delta)
  (Key -> option content_transition) (Key -> option Bytes) (Key -> count_delta).
Definition hot_payload := @soft_payload Key hot_value Log Counter (Key -> count_delta)
  (Key -> count_delta) (Key -> option content_transition) (Key -> option Bytes)
  (Key -> count_delta).

Definition option_ref_eq_dec (one other : option Ref) : {one = other} + {one <> other}.
Proof.
  destruct one as [one|], other as [other|].
  - destruct (ref_eq_dec one other) as [same|different].
    + left; rewrite same; reflexivity.
    + right; intros equal; injection equal; exact different.
  - right; discriminate.
  - right; discriminate.
  - left; reflexivity.
Defined.

Definition rebased_delta (base target : nat) : count_delta :=
  (rebased_addition base target, rebased_removal base target).

Definition rebased_content (base target : option Ref) : option content_transition :=
  if option_ref_eq_dec base target then None else Some (base, target).

Definition source_of_transition (transition : option content_transition) : option Bytes :=
  match transition with
  | Some (_, Some reference) => Some (source_bytes reference)
  | _ => None
  end.

Definition rebase_journals (payload : hot_payload) (history : Key -> hot_value)
    : hot_journals :=
  FiveJournals _ _ _ _ _
    (fun key => rebased_delta (data_count (history key))
      (data_count (overlay payload history key)))
    (fun key => rebased_delta (continuation_count (history key))
      (continuation_count (overlay payload history key)))
    (fun key => rebased_content (continuation_content (history key))
      (continuation_content (overlay payload history key)))
    (fun key => source_of_transition (rebased_content (continuation_content (history key))
      (continuation_content (overlay payload history key))))
    (fun key => rebased_delta (join_count (history key))
      (join_count (overlay payload history key))).

Definition exact_count_delta (base target : nat) (delta : count_delta) : Prop :=
  apply_count_delta base (fst delta) (snd delta) = target /\ snd delta <= base /\
  (fst delta = 0 \/ snd delta = 0).

Definition exact_content_transition (base target : option Ref)
    (transition : option content_transition) : Prop :=
  match transition with
  | None => base = target
  | Some (before, after) => before = base /\ after = target /\ before <> after
  end.

Definition hot_journal_refinement (payload : hot_payload) (history : Key -> hot_value)
    (journals : hot_journals) : Prop :=
  forall key,
    exact_count_delta (data_count (history key)) (data_count (overlay payload history key))
      (journal_data _ _ _ _ _ journals key) /\
    exact_count_delta (continuation_count (history key))
      (continuation_count (overlay payload history key))
      (journal_continuations _ _ _ _ _ journals key) /\
    exact_content_transition (continuation_content (history key))
      (continuation_content (overlay payload history key))
      (journal_content _ _ _ _ _ journals key) /\
    journal_sources _ _ _ _ _ journals key =
      source_of_transition (journal_content _ _ _ _ _ journals key) /\
    exact_count_delta (join_count (history key)) (join_count (overlay payload history key))
      (journal_joins _ _ _ _ _ journals key).

Lemma rebased_delta_is_exact : forall base target,
  exact_count_delta base target (rebased_delta base target).
Proof.
  unfold exact_count_delta, rebased_delta, apply_count_delta, rebased_addition,
    rebased_removal; simpl; intros; lia.
Qed.

Lemma exact_count_delta_is_the_rebased_delta : forall base target delta,
  exact_count_delta base target delta -> delta = rebased_delta base target.
Proof.
  unfold exact_count_delta, rebased_delta, apply_count_delta, rebased_addition,
    rebased_removal; intros base target [addition removal]; simpl.
  intros [reconstructed [bounded disjoint]]; f_equal; lia.
Qed.

Lemma rebased_content_is_exact : forall base target,
  exact_content_transition base target (rebased_content base target).
Proof.
  intros base target; unfold exact_content_transition, rebased_content.
  destruct (option_ref_eq_dec base target) as [same|different]; [exact same|].
  split; [reflexivity|split; [reflexivity|exact different]].
Qed.

Lemma exact_content_transition_is_the_rebased_content : forall base target transition,
  exact_content_transition base target transition ->
  transition = rebased_content base target.
Proof.
  intros base target transition exact; unfold rebased_content.
  destruct transition as [[before after]|]; simpl in exact;
    destruct (option_ref_eq_dec base target) as [same|different].
  - destruct exact as [from [to changed]]; exfalso; apply changed; rewrite from, to;
      exact same.
  - destruct exact as [from [to _]]; rewrite from, to; reflexivity.
  - reflexivity.
  - contradiction.
Qed.

Theorem rebased_journals_refine_soft_restore : forall payload history,
  hot_journal_refinement payload history (rebase_journals payload history).
Proof.
  intros payload history key; simpl.
  split; [apply rebased_delta_is_exact|].
  split; [apply rebased_delta_is_exact|].
  split; [apply rebased_content_is_exact|].
  split; [reflexivity|apply rebased_delta_is_exact].
Qed.

Theorem soft_restore_refinement_determines_the_journals : forall payload history journals,
  hot_journal_refinement payload history journals ->
  forall key,
    journal_data _ _ _ _ _ journals key =
      journal_data _ _ _ _ _ (rebase_journals payload history) key /\
    journal_continuations _ _ _ _ _ journals key =
      journal_continuations _ _ _ _ _ (rebase_journals payload history) key /\
    journal_content _ _ _ _ _ journals key =
      journal_content _ _ _ _ _ (rebase_journals payload history) key /\
    journal_sources _ _ _ _ _ journals key =
      journal_sources _ _ _ _ _ (rebase_journals payload history) key /\
    journal_joins _ _ _ _ _ journals key =
      journal_joins _ _ _ _ _ (rebase_journals payload history) key.
Proof.
  intros payload history journals refined key; simpl.
  destruct (refined key) as [data [continuations [content [sources joins]]]].
  apply exact_content_transition_is_the_rebased_content in content.
  split; [apply exact_count_delta_is_the_rebased_delta; exact data|].
  split; [apply exact_count_delta_is_the_rebased_delta; exact continuations|].
  split; [exact content|].
  split; [rewrite sources, content; reflexivity|].
  apply exact_count_delta_is_the_rebased_delta; exact joins.
Qed.

Corollary every_legacy_overlay_has_a_permitted_restore :
  forall (authenticated_view_history : mutable_view -> (Key -> hot_value) -> Prop)
    o p v (payload : hot_payload) history,
  authorizes o p -> attached o v -> authenticated_view_history v history ->
  exists journals, permitted_restore authenticated_view_history hot_journal_refinement
    o p v payload history journals o (restore payload history journals).
Proof.
  intros authenticated_view_history o p v payload history permit same authenticated.
  apply every_legacy_overlay_with_refined_journals_has_a_permitted_restore;
    [exact permit|exact same|exact authenticated|].
  exists (rebase_journals payload history).
  apply rebased_journals_refine_soft_restore.
Qed.

End FiveJournalRebase.

Print Assumptions rebased_journals_refine_soft_restore.
Print Assumptions soft_restore_refinement_determines_the_journals.
Print Assumptions every_legacy_overlay_has_a_permitted_restore.

Module ScopeBytes := ChannelImage.
Set Implicit Arguments.

Definition database_name := list nat.
Definition name_eq_dec := list_eq_dec Nat.eq_dec.

Record resolved_names := ResolvedNames {
  logical_prefix : database_name;
  history_name : database_name;
  roots_name : database_name;
  cold_name : database_name
}.

Definition history_suffix : database_name := [45; 104; 105; 115; 116; 111; 114; 121].
Definition roots_suffix : database_name := [45; 114; 111; 111; 116; 115].
Definition cold_suffix : database_name := [45; 99; 111; 108; 100].
Definition scope_namespace_tag : database_name :=
  [0; 114; 115; 112; 97; 99; 101; 45; 97; 117; 116; 104; 111; 114; 105; 116; 121;
   45; 108; 97; 121; 111; 117; 116; 0; 1].

Definition field_fits (field : database_name) :=
  ScopeBytes.fits_fixed_be 8 (length field) = true.

Definition names_fit (names : resolved_names) :=
  field_fits (logical_prefix names) /\ field_fits (history_name names) /\
  field_fits (roots_name names) /\ field_fits (cold_name names).

Definition physical_names_have_no_nul (names : resolved_names) :=
  ~ In 0 (history_name names) /\ ~ In 0 (roots_name names) /\ ~ In 0 (cold_name names).

Definition name_frame (field : database_name) : database_name :=
  ScopeBytes.encode_u64_be (length field) ++ field.

Definition framed_names (names : resolved_names) : database_name :=
  name_frame (logical_prefix names) ++ name_frame (history_name names) ++
  name_frame (roots_name names) ++ name_frame (cold_name names).

Definition default_names (names : resolved_names) : bool :=
  if name_eq_dec (history_name names) (logical_prefix names ++ history_suffix) then
    if name_eq_dec (roots_name names) (logical_prefix names ++ roots_suffix) then
      if name_eq_dec (cold_name names) (logical_prefix names ++ cold_suffix) then true else false
    else false
  else false.

Definition migration_namespace (names : resolved_names) : database_name :=
  if default_names names then logical_prefix names
  else scope_namespace_tag ++ framed_names names.

Lemma scope_equal_length_append_cancels : forall (left right tail suffix : database_name),
  length left = length right -> left ++ tail = right ++ suffix -> left = right /\ tail = suffix.
Proof.
  induction left as [|head rest IH]; intros [|other remaining] tail suffix same_length same;
    simpl in *; try discriminate.
  - auto.
  - inversion same; subst. injection same_length as lengths.
    destruct (IH remaining tail suffix lengths H1) as [prefix suffixes]. subst. auto.
Qed.

Theorem checked_u64_name_frames_cancel_exactly : forall left right tail suffix,
  field_fits left -> field_fits right ->
  name_frame left ++ tail = name_frame right ++ suffix -> left = right /\ tail = suffix.
Proof.
  intros left right tail suffix left_fits right_fits encoded.
  unfold name_frame in encoded. repeat rewrite <- app_assoc in encoded.
  apply (f_equal ScopeBytes.decode_u64_be) in encoded.
  unfold field_fits in left_fits, right_fits.
  apply ScopeBytes.fits_fixed_be_true_iff in left_fits, right_fits.
  rewrite !ScopeBytes.u64_big_endian_round_trip in encoded by assumption.
  injection encoded as lengths bodies.
  now apply scope_equal_length_append_cancels.
Qed.

Theorem four_checked_name_fields_encode_injectively : forall left right,
  names_fit left -> names_fit right -> framed_names left = framed_names right -> left = right.
Proof.
  intros [lp lh lr lc] [rp rh rr rc] [lp_fit [lh_fit [lr_fit lc_fit]]]
    [rp_fit [rh_fit [rr_fit rc_fit]]] same.
  unfold framed_names in same; cbn in *.
  apply checked_u64_name_frames_cancel_exactly in same as [prefix same]; [|assumption..].
  apply checked_u64_name_frames_cancel_exactly in same as [history same]; [|assumption..].
  apply checked_u64_name_frames_cancel_exactly in same as [roots same]; [|assumption..].
  assert (cold : lc = rc).
  { apply (proj1 (checked_u64_name_frames_cancel_exactly lc_fit rc_fit
      (tail := []) (suffix := []) ltac:(now rewrite !app_nil_r))). }
  subst. reflexivity.
Qed.

Theorem default_layout_has_exact_legacy_names : forall names,
  default_names names = true <->
  history_name names = logical_prefix names ++ history_suffix /\
  roots_name names = logical_prefix names ++ roots_suffix /\
  cold_name names = logical_prefix names ++ cold_suffix.
Proof.
  intro names. unfold default_names.
  repeat destruct name_eq_dec; intuition congruence.
Qed.

Lemma default_prefix_has_no_nul : forall names,
  default_names names = true -> physical_names_have_no_nul names ->
  ~ In 0 (logical_prefix names).
Proof.
  intros names legacy [history_clean _] present.
  apply default_layout_has_exact_legacy_names in legacy as [history _].
  apply history_clean. rewrite history. apply in_or_app. left. exact present.
Qed.

Theorem default_namespace_bytes_are_unchanged : forall names,
  default_names names = true -> migration_namespace names = logical_prefix names.
Proof. intros names legacy; unfold migration_namespace; now rewrite legacy. Qed.

Theorem resolved_namespace_encoding_is_injective : forall left right,
  names_fit left -> names_fit right ->
  physical_names_have_no_nul left -> physical_names_have_no_nul right ->
  migration_namespace left = migration_namespace right -> left = right.
Proof.
  intros left right left_fit right_fit left_clean right_clean same.
  unfold migration_namespace in same.
  destruct (default_names left) eqn:left_default;
    destruct (default_names right) eqn:right_default.
  - apply default_layout_has_exact_legacy_names in left_default, right_default.
    destruct left, right; cbn in *. intuition congruence.
  - exfalso. apply (default_prefix_has_no_nul left_default left_clean).
    rewrite same. cbn. auto.
  - exfalso. apply (default_prefix_has_no_nul right_default right_clean).
    rewrite <- same. cbn. auto.
  - apply app_inv_head in same. now apply four_checked_name_fields_encode_injectively.
Qed.

Inductive database_role := HistoryRole | RootsRole | ColdRole.

Definition role_suffix (role : database_role) : database_name :=
  match role with HistoryRole => history_suffix | RootsRole => roots_suffix | ColdRole => cold_suffix end.

Record lmdb_configuration := LmdbConfiguration {
  physical_environment : nat;
  configured_map_size : nat;
  configured_max_databases : nat
}.

Record database_mapping := DatabaseMapping {
  logical_database_id : database_name;
  database_name_override : option database_name;
  database_configuration : lmdb_configuration
}.

Definition resolve_database_name (mapping : database_mapping) : database_name :=
  match database_name_override mapping with
  | Some literal => literal
  | None => logical_database_id mapping
  end.

Definition role_mappings (prefix : database_name) (role : database_role)
    (mappings : list database_mapping) : list database_mapping :=
  filter (fun mapping =>
    if name_eq_dec (logical_database_id mapping) (prefix ++ role_suffix role) then true else false)
    mappings.

Record authority_selection := AuthoritySelection {
  selected_prefix : database_name;
  selected_history : database_mapping;
  selected_roots : database_mapping;
  selected_cold : database_mapping
}.

Definition selection_names (selection : authority_selection) : resolved_names :=
  ResolvedNames (selected_prefix selection)
    (resolve_database_name (selected_history selection))
    (resolve_database_name (selected_roots selection))
    (resolve_database_name (selected_cold selection)).

Definition exact_role_selection (mappings : list database_mapping) (selection : authority_selection) :=
  role_mappings (selected_prefix selection) HistoryRole mappings = [selected_history selection] /\
  role_mappings (selected_prefix selection) RootsRole mappings = [selected_roots selection] /\
  role_mappings (selected_prefix selection) ColdRole mappings = [selected_cold selection].

Definition selected_mapping (selection : authority_selection) (role : database_role) :=
  match role with
  | HistoryRole => selected_history selection
  | RootsRole => selected_roots selection
  | ColdRole => selected_cold selection
  end.

Definition physical_slot := (nat * database_name)%type.

Definition physical_slot_eq_dec : forall left right : physical_slot,
  {left = right} + {left <> right}.
Proof. decide equality; [apply name_eq_dec|apply Nat.eq_dec]. Defined.

Definition live_environment (selection : authority_selection) : nat :=
  physical_environment (database_configuration (selected_history selection)).

Definition live_slot (selection : authority_selection) (role : database_role) : physical_slot :=
  (live_environment selection, resolve_database_name (selected_mapping selection role)).

Definition authority_footprint (selection : authority_selection) : list physical_slot :=
  [live_slot selection HistoryRole; live_slot selection RootsRole; live_slot selection ColdRole].

Definition footprints_disjoint (left right : list physical_slot) : Prop :=
  forall slot, In slot left -> ~ In slot right.

Definition physical_roles_distinct (selection : authority_selection) : Prop :=
  NoDup [history_name (selection_names selection); roots_name (selection_names selection);
    cold_name (selection_names selection)].

Definition scope_admitted (mappings : list database_mapping) (ordinary_writable : list physical_slot)
    (selection : authority_selection) : Prop :=
  exact_role_selection mappings selection /\
  database_configuration (selected_history selection) =
    database_configuration (selected_roots selection) /\
  physical_names_have_no_nul (selection_names selection) /\
  names_fit (selection_names selection) /\ physical_roles_distinct selection /\
  footprints_disjoint (authority_footprint selection) ordinary_writable.

Theorem each_name_override_is_resolved_independently : forall identifier override config,
  resolve_database_name (DatabaseMapping identifier (Some override) config) = override /\
  resolve_database_name (DatabaseMapping identifier None config) = identifier.
Proof. repeat split; reflexivity. Qed.

Theorem exact_role_resolution_is_unique : forall mappings left right,
  selected_prefix left = selected_prefix right ->
  exact_role_selection mappings left -> exact_role_selection mappings right -> left = right.
Proof.
  intros mappings [lp lh lr lc] [rp rh rr rc] prefix left right.
  unfold exact_role_selection in *; cbn in *. intuition congruence.
Qed.

Theorem admitted_roles_are_present_exactly_once : forall mappings ordinary selection role,
  scope_admitted mappings ordinary selection ->
  role_mappings (selected_prefix selection) role mappings = [selected_mapping selection role].
Proof.
  intros mappings ordinary selection role [[history [roots cold]] _].
  destruct role; assumption.
Qed.

Theorem missing_or_duplicate_logical_role_refuses : forall mappings ordinary selection role,
  length (role_mappings (selected_prefix selection) role mappings) <> 1 ->
  ~ scope_admitted mappings ordinary selection.
Proof.
  intros mappings ordinary selection role count admitted.
  apply count. rewrite (admitted_roles_are_present_exactly_once role admitted). reflexivity.
Qed.

Theorem incompatible_history_roots_configuration_refuses : forall mappings ordinary selection,
  database_configuration (selected_history selection) <>
    database_configuration (selected_roots selection) ->
  ~ scope_admitted mappings ordinary selection.
Proof. unfold scope_admitted; intuition. Qed.

Theorem nul_physical_name_refuses : forall mappings ordinary selection,
  ~ physical_names_have_no_nul (selection_names selection) ->
  ~ scope_admitted mappings ordinary selection.
Proof. unfold scope_admitted; intuition. Qed.

Theorem unchecked_field_length_refuses : forall mappings ordinary selection,
  ~ names_fit (selection_names selection) -> ~ scope_admitted mappings ordinary selection.
Proof. unfold scope_admitted; intuition. Qed.

Theorem colliding_physical_roles_refuse : forall mappings ordinary selection,
  ~ physical_roles_distinct selection -> ~ scope_admitted mappings ordinary selection.
Proof. unfold scope_admitted; intuition. Qed.

Theorem writable_ordinary_alias_refuses : forall mappings ordinary selection slot,
  In slot (authority_footprint selection) -> In slot ordinary ->
  ~ scope_admitted mappings ordinary selection.
Proof. unfold scope_admitted, footprints_disjoint; intuition eauto. Qed.

Inductive scope_open_attempt (mappings : list database_mapping)
    (ordinary : list physical_slot) (selection : authority_selection) : list physical_slot -> Prop :=
| ScopeOpenRefused : ~ scope_admitted mappings ordinary selection ->
    scope_open_attempt mappings ordinary selection []
| ScopeOpenAllowed : scope_admitted mappings ordinary selection ->
    scope_open_attempt mappings ordinary selection (authority_footprint selection).

Theorem rejected_scope_performs_no_database_open : forall mappings ordinary selection opened,
  ~ scope_admitted mappings ordinary selection ->
  scope_open_attempt mappings ordinary selection opened -> opened = [].
Proof. intros mappings ordinary selection opened refused step; inversion step; subst; tauto. Qed.

Definition physical_contents := physical_slot -> nat -> option nat.

Definition update_physical_cell (before : physical_contents) (slot : physical_slot)
    (key value : nat) : physical_contents :=
  fun observed_slot observed_key =>
    if physical_slot_eq_dec observed_slot slot then
      if Nat.eq_dec observed_key key then Some value else before observed_slot observed_key
    else before observed_slot observed_key.

Theorem disjoint_physical_footprints_preserve_other_content : forall before left right
    written observed key value observed_key,
  footprints_disjoint left right -> In written left -> In observed right ->
  update_physical_cell before written key value observed observed_key = before observed observed_key.
Proof.
  intros before left right written observed key value observed_key disjoint in_left in_right.
  unfold update_physical_cell. destruct physical_slot_eq_dec as [same|different]; [|reflexivity].
  subst. exfalso. exact (disjoint written in_left in_right).
Qed.

Theorem same_physical_layout_reopens_same_content : forall (before : physical_contents) left right,
  authority_footprint left = authority_footprint right ->
  forall role key, before (live_slot left role) key = before (live_slot right role) key.
Proof.
  intros before left right same role key.
  destruct role.
  - pose proof (f_equal (fun slots => nth 0 slots (0, [])) same) as history.
    change (live_slot left HistoryRole = live_slot right HistoryRole) in history.
    now rewrite history.
  - pose proof (f_equal (fun slots => nth 1 slots (0, [])) same) as roots.
    change (live_slot left RootsRole = live_slot right RootsRole) in roots.
    now rewrite roots.
  - pose proof (f_equal (fun slots => nth 2 slots (0, [])) same) as cold.
    change (live_slot left ColdRole = live_slot right ColdRole) in cold.
    now rewrite cold.
Qed.

Definition migration_cold_source (selection : authority_selection) : physical_slot :=
  (physical_environment (database_configuration (selected_cold selection)),
    resolve_database_name (selected_cold selection)).

Definition legacy_db_name : database_name := [100; 98].
Definition rspace_prefix : database_name := [114; 115; 112; 97; 99; 101].

Definition migration_cold_candidates (selection : authority_selection) : list database_name :=
  if default_names (selection_names selection) then
    [resolve_database_name (selected_cold selection); legacy_db_name]
  else [resolve_database_name (selected_cold selection)].

Definition legacy_v2_allowed (selection : authority_selection) : Prop :=
  default_names (selection_names selection) = true /\ selected_prefix selection = rspace_prefix.

Theorem migration_primary_source_uses_literal_cold_name : forall selection,
  snd (migration_cold_source selection) = cold_name (selection_names selection) /\
  fst (migration_cold_source selection) =
    physical_environment (database_configuration (selected_cold selection)).
Proof. repeat split; reflexivity. Qed.

Theorem nondefault_layout_has_no_ambient_migration_sources : forall selection,
  default_names (selection_names selection) = false ->
  migration_cold_candidates selection = [cold_name (selection_names selection)] /\
  ~ legacy_v2_allowed selection.
Proof.
  intros selection scoped. unfold migration_cold_candidates, legacy_v2_allowed.
  rewrite scoped. split; [reflexivity|intuition discriminate].
Qed.

Theorem default_layout_preserves_legacy_cold_candidates : forall selection,
  default_names (selection_names selection) = true ->
  migration_cold_candidates selection = [cold_name (selection_names selection); legacy_db_name].
Proof. intros selection legacy; unfold migration_cold_candidates; now rewrite legacy. Qed.

Definition migration_marker_accepts (names : resolved_names) (marker_namespace : database_name) :=
  marker_namespace = migration_namespace names.

Theorem foreign_resolved_namespace_marker_refuses : forall source target,
  names_fit source -> names_fit target ->
  physical_names_have_no_nul source -> physical_names_have_no_nul target -> source <> target ->
  ~ migration_marker_accepts target (migration_namespace source).
Proof.
  unfold migration_marker_accepts. intros source target sf tf sc tc different same.
  apply different. now apply resolved_namespace_encoding_is_injective.
Qed.

Theorem unrelated_environment_does_not_reserve_literal_names : forall selection environment name,
  environment <> live_environment selection ->
  footprints_disjoint (authority_footprint selection) [(environment, name)].
Proof.
  intros selection environment name different slot in_authority in_ordinary.
  unfold authority_footprint, live_slot in in_authority.
  simpl in in_authority, in_ordinary. intuition congruence.
Qed.

Inductive scope_invariant_id :=
| IndependentNameResolution | UniqueRoleResolution | ExactlyOneLogicalRole
| HistoryRootsConfiguration | PhysicalNamesRejectNul | CheckedFieldLengths
| DistinctPhysicalRoles | OrdinaryAliasExclusion | ValidationBeforeDatabaseOpen
| ExactU64FrameCancellation | ExactFourFieldEncoding | LegacyNamespaceCompatibility
| FullNamespaceInjectivity | DisjointPhysicalIsolation | SameLayoutReopen
| LiteralMigrationColdSource | NoScopedAmbientMigration | LegacyColdCompatibility
| MigrationMarkerBinding | UnrelatedEnvironmentLiteralNames.

Definition scope_invariant_catalog : list scope_invariant_id :=
  [IndependentNameResolution; UniqueRoleResolution; ExactlyOneLogicalRole;
   HistoryRootsConfiguration; PhysicalNamesRejectNul; CheckedFieldLengths;
   DistinctPhysicalRoles; OrdinaryAliasExclusion; ValidationBeforeDatabaseOpen;
   ExactU64FrameCancellation; ExactFourFieldEncoding; LegacyNamespaceCompatibility;
   FullNamespaceInjectivity; DisjointPhysicalIsolation; SameLayoutReopen;
   LiteralMigrationColdSource; NoScopedAmbientMigration; LegacyColdCompatibility;
   MigrationMarkerBinding; UnrelatedEnvironmentLiteralNames].

Definition scope_invariant_binding (invariant : scope_invariant_id) : Prop :=
  match invariant with
  | IndependentNameResolution => forall identifier override config,
      resolve_database_name (DatabaseMapping identifier (Some override) config) = override /\
      resolve_database_name (DatabaseMapping identifier None config) = identifier
  | UniqueRoleResolution => forall mappings left right,
      selected_prefix left = selected_prefix right -> exact_role_selection mappings left ->
      exact_role_selection mappings right -> left = right
  | ExactlyOneLogicalRole => forall mappings ordinary selection role,
      scope_admitted mappings ordinary selection ->
      role_mappings (selected_prefix selection) role mappings = [selected_mapping selection role]
  | HistoryRootsConfiguration => forall mappings ordinary selection,
      database_configuration (selected_history selection) <>
        database_configuration (selected_roots selection) -> ~ scope_admitted mappings ordinary selection
  | PhysicalNamesRejectNul => forall mappings ordinary selection,
      ~ physical_names_have_no_nul (selection_names selection) -> ~ scope_admitted mappings ordinary selection
  | CheckedFieldLengths => forall mappings ordinary selection,
      ~ names_fit (selection_names selection) -> ~ scope_admitted mappings ordinary selection
  | DistinctPhysicalRoles => forall mappings ordinary selection,
      ~ physical_roles_distinct selection -> ~ scope_admitted mappings ordinary selection
  | OrdinaryAliasExclusion => forall mappings ordinary selection slot,
      In slot (authority_footprint selection) -> In slot ordinary -> ~ scope_admitted mappings ordinary selection
  | ValidationBeforeDatabaseOpen => forall mappings ordinary selection opened,
      ~ scope_admitted mappings ordinary selection -> scope_open_attempt mappings ordinary selection opened ->
      opened = []
  | ExactU64FrameCancellation => forall left right tail suffix,
      field_fits left -> field_fits right ->
      name_frame left ++ tail = name_frame right ++ suffix -> left = right /\ tail = suffix
  | ExactFourFieldEncoding => forall left right,
      names_fit left -> names_fit right -> framed_names left = framed_names right -> left = right
  | LegacyNamespaceCompatibility => forall names,
      default_names names = true -> migration_namespace names = logical_prefix names
  | FullNamespaceInjectivity => forall left right,
      names_fit left -> names_fit right -> physical_names_have_no_nul left ->
      physical_names_have_no_nul right -> migration_namespace left = migration_namespace right -> left = right
  | DisjointPhysicalIsolation => forall before left right written observed key value observed_key,
      footprints_disjoint left right -> In written left -> In observed right ->
      update_physical_cell before written key value observed observed_key = before observed observed_key
  | SameLayoutReopen => forall (before : physical_contents) left right,
      authority_footprint left = authority_footprint right ->
      forall role key, before (live_slot left role) key = before (live_slot right role) key
  | LiteralMigrationColdSource => forall selection,
      snd (migration_cold_source selection) = cold_name (selection_names selection) /\
      fst (migration_cold_source selection) =
        physical_environment (database_configuration (selected_cold selection))
  | NoScopedAmbientMigration => forall selection,
      default_names (selection_names selection) = false ->
      migration_cold_candidates selection = [cold_name (selection_names selection)] /\ ~ legacy_v2_allowed selection
  | LegacyColdCompatibility => forall selection,
      default_names (selection_names selection) = true ->
      migration_cold_candidates selection = [cold_name (selection_names selection); legacy_db_name]
  | MigrationMarkerBinding => forall source target,
      names_fit source -> names_fit target -> physical_names_have_no_nul source ->
      physical_names_have_no_nul target -> source <> target ->
      ~ migration_marker_accepts target (migration_namespace source)
  | UnrelatedEnvironmentLiteralNames => forall selection environment name,
      environment <> live_environment selection ->
      footprints_disjoint (authority_footprint selection) [(environment, name)]
  end.

Theorem every_scope_invariant_is_bound : forall invariant, scope_invariant_binding invariant.
Proof.
  intro invariant; destruct invariant; cbn.
  - apply each_name_override_is_resolved_independently.
  - apply exact_role_resolution_is_unique.
  - apply admitted_roles_are_present_exactly_once.
  - apply incompatible_history_roots_configuration_refuses.
  - apply nul_physical_name_refuses.
  - apply unchecked_field_length_refuses.
  - apply colliding_physical_roles_refuse.
  - apply writable_ordinary_alias_refuses.
  - apply rejected_scope_performs_no_database_open.
  - apply checked_u64_name_frames_cancel_exactly.
  - apply four_checked_name_fields_encode_injectively.
  - apply default_namespace_bytes_are_unchanged.
  - apply resolved_namespace_encoding_is_injective.
  - apply disjoint_physical_footprints_preserve_other_content.
  - apply same_physical_layout_reopens_same_content.
  - apply migration_primary_source_uses_literal_cold_name.
  - apply nondefault_layout_has_no_ambient_migration_sources.
  - apply default_layout_preserves_legacy_cold_candidates.
  - apply foreign_resolved_namespace_marker_refuses.
  - apply unrelated_environment_does_not_reserve_literal_names.
Qed.

Theorem scope_invariant_catalog_is_unique_complete_and_exact :
  NoDup scope_invariant_catalog /\ Forall scope_invariant_binding scope_invariant_catalog /\
  (forall invariant, In invariant scope_invariant_catalog) /\ length scope_invariant_catalog = 20.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - apply Forall_forall. intros invariant _. apply every_scope_invariant_is_bound.
  - intro invariant; destruct invariant; simpl; tauto.
Qed.

Print Assumptions resolved_namespace_encoding_is_injective.
Print Assumptions rejected_scope_performs_no_database_open.
Print Assumptions disjoint_physical_footprints_preserve_other_content.
Print Assumptions migration_primary_source_uses_literal_cold_name.
Print Assumptions every_scope_invariant_is_bound.
Print Assumptions scope_invariant_catalog_is_unique_complete_and_exact.

End RSpaceCallerAuthority.
