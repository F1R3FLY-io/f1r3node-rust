From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.

Record recovery_episode := {
  episode_revision : nat;
  episode_floor : nat;
  episode_post_state : nat;
  episode_certificate : option nat
}.

Definition certified_episode (episode : recovery_episode) : Prop :=
  match episode_revision episode, episode_certificate episode with
  | 0, None => True
  | S _, Some _ => True
  | _, _ => False
  end.

Definition certified_advance
  (previous next : recovery_episode) : Prop :=
  certified_episode next /\
  episode_revision previous < episode_revision next /\
  episode_floor previous <> episode_floor next.

Theorem recovery_episode_advance_requires_certificate :
  forall previous next,
    certified_advance previous next ->
    episode_revision next > 0 ->
    exists certificate, episode_certificate next = Some certificate.
Proof.
  intros previous [revision floor post certificate] advance positive.
  destruct advance as [certified _].
  simpl in *.
  destruct revision; [lia|].
  destruct certificate; [eauto|contradiction].
Qed.

Record recovery_charge := {
  charge_episode : nat;
  charge_target : nat;
  charge_citer : nat;
  charge_generation : nat
}.

Definition recovery_charge_eq_dec :
  forall left right : recovery_charge, {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Theorem recovery_charge_identity_is_complete :
  forall left right,
    charge_episode left = charge_episode right ->
    charge_target left = charge_target right ->
    charge_citer left = charge_citer right ->
    charge_generation left = charge_generation right ->
    left = right.
Proof.
  intros [le lt lc lg] [re rt rc rg].
  simpl. intros. subst. reflexivity.
Qed.

Record recovery_ledger := {
  durable_charges : list recovery_charge;
  durable_usage : nat
}.

Definition ledger_invariant (ledger : recovery_ledger) : Prop :=
  durable_usage ledger = length (durable_charges ledger) /\
  NoDup (durable_charges ledger).

Definition commit_recovery_charge
  (capacity : nat)
  (charge : recovery_charge)
  (ledger : recovery_ledger) : option recovery_ledger :=
  if in_dec recovery_charge_eq_dec charge (durable_charges ledger) then
    Some ledger
  else if Nat.ltb (durable_usage ledger) capacity then
    Some
      {| durable_charges := charge :: durable_charges ledger;
         durable_usage := S (durable_usage ledger) |}
  else None.

Theorem exact_charge_retry_is_idempotent :
  forall capacity charge ledger,
    In charge (durable_charges ledger) ->
    commit_recovery_charge capacity charge ledger = Some ledger.
Proof.
  intros capacity charge ledger present.
  unfold commit_recovery_charge.
  destruct (in_dec recovery_charge_eq_dec charge (durable_charges ledger)).
  - reflexivity.
  - contradiction.
Qed.

Theorem fresh_charge_commits_atomically :
  forall capacity charge ledger,
    ~ In charge (durable_charges ledger) ->
    durable_usage ledger < capacity ->
    commit_recovery_charge capacity charge ledger =
      Some
        {| durable_charges := charge :: durable_charges ledger;
           durable_usage := S (durable_usage ledger) |}.
Proof.
  intros capacity charge ledger absent available.
  unfold commit_recovery_charge.
  destruct (in_dec recovery_charge_eq_dec charge (durable_charges ledger)).
  - contradiction.
  - destruct (Nat.ltb (durable_usage ledger) capacity) eqn:comparison.
    + reflexivity.
    + apply Nat.ltb_ge in comparison. lia.
Qed.

Theorem exhausted_charge_is_effect_free :
  forall capacity charge ledger,
    ~ In charge (durable_charges ledger) ->
    capacity <= durable_usage ledger ->
    commit_recovery_charge capacity charge ledger = None.
Proof.
  intros capacity charge ledger absent exhausted.
  unfold commit_recovery_charge.
  destruct (in_dec recovery_charge_eq_dec charge (durable_charges ledger)).
  - contradiction.
  - destruct (Nat.ltb (durable_usage ledger) capacity) eqn:comparison.
    + apply Nat.ltb_lt in comparison. lia.
    + reflexivity.
Qed.

Theorem fresh_charge_preserves_ledger_invariant :
  forall capacity charge ledger committed,
    ledger_invariant ledger ->
    commit_recovery_charge capacity charge ledger = Some committed ->
    ledger_invariant committed.
Proof.
  intros capacity charge ledger committed [usage_exact unique] result.
  unfold commit_recovery_charge in result.
  destruct (in_dec recovery_charge_eq_dec charge (durable_charges ledger)).
  - inversion result. subst. split; assumption.
  - destruct (Nat.ltb (durable_usage ledger) capacity) eqn:available.
    + inversion result. subst. unfold ledger_invariant. simpl. split.
      * f_equal. exact usage_exact.
      * constructor; assumption.
    + discriminate.
Qed.

Theorem fresh_charge_preserves_capacity :
  forall capacity charge ledger committed,
    durable_usage ledger <= capacity ->
    commit_recovery_charge capacity charge ledger = Some committed ->
    durable_usage committed <= capacity.
Proof.
  intros capacity charge ledger committed bounded result.
  unfold commit_recovery_charge in result.
  destruct (in_dec recovery_charge_eq_dec charge (durable_charges ledger)).
  - inversion result. subst. exact bounded.
  - destruct (Nat.ltb (durable_usage ledger) capacity) eqn:available.
    + apply Nat.ltb_lt in available. inversion result. subst. simpl. lia.
    + discriminate.
Qed.

Definition migrate_recovery_charges
  (missing : list recovery_charge)
  (ledger : recovery_ledger) : recovery_ledger :=
  {| durable_charges := missing ++ durable_charges ledger;
     durable_usage := durable_usage ledger + length missing |}.

Theorem migration_records_exact_usage :
  forall missing ledger,
    durable_usage (migrate_recovery_charges missing ledger) =
    durable_usage ledger + length missing.
Proof. reflexivity. Qed.

Theorem migration_preserves_ledger_invariant :
  forall missing ledger,
    ledger_invariant ledger ->
    NoDup (missing ++ durable_charges ledger) ->
    ledger_invariant (migrate_recovery_charges missing ledger).
Proof.
  intros missing ledger [usage_exact _] unique.
  unfold migrate_recovery_charges, ledger_invariant. simpl.
  split; [rewrite length_app, usage_exact; lia|exact unique].
Qed.

Theorem empty_migration_is_idempotent :
  forall ledger, migrate_recovery_charges [] ledger = ledger.
Proof.
  intros [charges usage]. unfold migrate_recovery_charges. simpl.
  rewrite Nat.add_0_r. reflexivity.
Qed.

Definition restart_recovery_ledger (ledger : recovery_ledger) : recovery_ledger := ledger.

Theorem restart_preserves_durable_recovery_state :
  forall ledger, restart_recovery_ledger ledger = ledger.
Proof. reflexivity. Qed.

Definition saturating_increment (maximum current : nat) : nat :=
  if Nat.ltb current maximum then S current else maximum.

Theorem retry_attempts_saturate :
  forall maximum current,
    current <= maximum ->
    saturating_increment maximum current <= maximum.
Proof.
  intros maximum current bounded.
  unfold saturating_increment.
  destruct (Nat.ltb_spec0 current maximum); lia.
Qed.

Section RetryIdentity.

Context {identity : Type}.
Variable identity_eq_dec : forall left right : identity, {left = right} + {left <> right}.

Record retry_entry := {
  retry_attempts : nat;
  retry_token : option identity;
  retry_owners : list nat
}.

Definition fresh_identity_source :=
  forall used : list identity, {fresh : identity | ~ In fresh used}.

Definition dispatch_retry
  (maximum : nat)
  (token : identity)
  (entry : retry_entry) : retry_entry :=
  {| retry_attempts := saturating_increment maximum (retry_attempts entry);
     retry_token := Some token;
     retry_owners := retry_owners entry |}.

Definition dispatch_fresh_retry
  (maximum : nat)
  (source : fresh_identity_source)
  (used : list identity)
  (entry : retry_entry) : retry_entry * identity :=
  let witness := source used in
  let token := proj1_sig witness in
  (dispatch_retry maximum token entry, token).

Definition finish_retry
  (token : identity)
  (progress : bool)
  (entry : retry_entry) : retry_entry :=
  match retry_token entry with
  | Some active =>
      if identity_eq_dec token active then
        {| retry_attempts := if progress then 0 else retry_attempts entry;
           retry_token := None;
           retry_owners := retry_owners entry |}
      else entry
  | None => entry
  end.

Definition expire_retry
  (token : identity)
  (entry : retry_entry) : retry_entry :=
  match retry_token entry with
  | Some active =>
      if identity_eq_dec token active then
        {| retry_attempts := retry_attempts entry;
           retry_token := None;
           retry_owners := retry_owners entry |}
      else entry
  | None => entry
  end.

Definition resolve_retry (entry : retry_entry) : retry_entry :=
  {| retry_attempts := 0;
     retry_token := None;
     retry_owners := [] |}.

Definition register_retry_owner (owner : nat) (entry : retry_entry) : retry_entry :=
  if in_dec Nat.eq_dec owner (retry_owners entry) then entry
  else
    {| retry_attempts := retry_attempts entry;
       retry_token := retry_token entry;
       retry_owners := owner :: retry_owners entry |}.

Definition release_retry_owner (owner : nat) (entry : retry_entry) : retry_entry :=
  {| retry_attempts := retry_attempts entry;
     retry_token := retry_token entry;
     retry_owners := remove Nat.eq_dec owner (retry_owners entry) |}.

Theorem dispatch_fresh_token_is_unused :
  forall maximum source used entry,
    ~ In (snd (dispatch_fresh_retry maximum source used entry)) used.
Proof.
  intros maximum source used entry.
  unfold dispatch_fresh_retry.
  destruct (source used) as [fresh unused].
  simpl. exact unused.
Qed.

Theorem dispatch_fresh_installs_exact_token :
  forall maximum source used entry,
    retry_token (fst (dispatch_fresh_retry maximum source used entry)) =
    Some (snd (dispatch_fresh_retry maximum source used entry)).
Proof.
  intros maximum source used entry.
  unfold dispatch_fresh_retry.
  destruct (source used) as [fresh unused].
  reflexivity.
Qed.

Theorem stale_completion_is_effect_free :
  forall entry active stale progress,
    retry_token entry = Some active ->
    stale <> active ->
    finish_retry stale progress entry = entry.
Proof.
  intros entry active stale progress token different.
  unfold finish_retry. rewrite token.
  destruct (identity_eq_dec stale active); [contradiction|reflexivity].
Qed.

Theorem stale_expiration_is_effect_free :
  forall entry active stale,
    retry_token entry = Some active ->
    stale <> active ->
    expire_retry stale entry = entry.
Proof.
  intros entry active stale token different.
  unfold expire_retry. rewrite token.
  destruct (identity_eq_dec stale active); [contradiction|reflexivity].
Qed.

Theorem no_progress_preserves_attempts :
  forall entry active,
    retry_token entry = Some active ->
    retry_attempts (finish_retry active false entry) = retry_attempts entry.
Proof.
  intros entry active token.
  unfold finish_retry. rewrite token.
  destruct (identity_eq_dec active active); [reflexivity|contradiction].
Qed.

Theorem progress_resets_attempts :
  forall entry active,
    retry_token entry = Some active ->
    retry_attempts (finish_retry active true entry) = 0.
Proof.
  intros entry active token.
  unfold finish_retry. rewrite token.
  destruct (identity_eq_dec active active); [reflexivity|contradiction].
Qed.

Theorem expiration_releases_exact_claim :
  forall entry active,
    retry_token entry = Some active ->
    retry_token (expire_retry active entry) = None.
Proof.
  intros entry active token.
  unfold expire_retry. rewrite token.
  destruct (identity_eq_dec active active); [reflexivity|contradiction].
Qed.

Theorem expiration_preserves_backoff :
  forall entry active,
    retry_token entry = Some active ->
    retry_attempts (expire_retry active entry) = retry_attempts entry.
Proof.
  intros entry active token.
  unfold expire_retry. rewrite token.
  destruct (identity_eq_dec active active); [reflexivity|contradiction].
Qed.

Theorem owner_registration_preserves_claim :
  forall owner entry,
    retry_token (register_retry_owner owner entry) = retry_token entry.
Proof.
  intros owner entry. unfold register_retry_owner.
  destruct (in_dec Nat.eq_dec owner (retry_owners entry)); reflexivity.
Qed.

Theorem owner_registration_preserves_backoff :
  forall owner entry,
    retry_attempts (register_retry_owner owner entry) = retry_attempts entry.
Proof.
  intros owner entry. unfold register_retry_owner.
  destruct (in_dec Nat.eq_dec owner (retry_owners entry)); reflexivity.
Qed.

Theorem existing_owner_registration_is_idempotent :
  forall owner entry,
    In owner (retry_owners entry) ->
    register_retry_owner owner entry = entry.
Proof.
  intros owner entry present. unfold register_retry_owner.
  destruct (in_dec Nat.eq_dec owner (retry_owners entry)); [reflexivity|contradiction].
Qed.

Theorem stale_completion_after_resolution_is_effect_free :
  forall entry stale progress owner,
    finish_retry stale progress (register_retry_owner owner (resolve_retry entry)) =
    register_retry_owner owner (resolve_retry entry).
Proof.
  intros entry stale progress owner.
  unfold finish_retry.
  rewrite owner_registration_preserves_claim.
  reflexivity.
Qed.

Theorem owner_release_never_increases_live_ownership :
  forall owner entry,
    length (retry_owners (release_retry_owner owner entry)) <=
    length (retry_owners entry).
Proof.
  intros owner entry. simpl. apply remove_length_le.
Qed.

Theorem retry_identity_protocol_correct :
  (forall maximum source used entry,
      ~ In (snd (dispatch_fresh_retry maximum source used entry)) used) /\
  (forall entry active stale progress,
      retry_token entry = Some active ->
      stale <> active ->
      finish_retry stale progress entry = entry) /\
  (forall entry active,
      retry_token entry = Some active ->
      retry_token (expire_retry active entry) = None) /\
  (forall owner entry,
      retry_token (register_retry_owner owner entry) = retry_token entry).
Proof.
  split; [exact dispatch_fresh_token_is_unused|].
  split; [exact stale_completion_is_effect_free|].
  split; [exact expiration_releases_exact_claim|].
  exact owner_registration_preserves_claim.
Qed.

End RetryIdentity.

Fixpoint maximum_identity (used : list nat) : nat :=
  match used with
  | [] => 0
  | identity :: rest => Nat.max identity (maximum_identity rest)
  end.

Definition fresh_nat_identity (used : list nat) : nat :=
  S (maximum_identity used).

Theorem member_does_not_exceed_maximum_identity :
  forall used identity,
    In identity used -> identity <= maximum_identity used.
Proof.
  induction used as [|head rest induction]; intros identity present.
  - contradiction.
  - simpl in *. destruct present as [equal|present].
    + subst. apply Nat.le_max_l.
    + eapply Nat.le_trans; [apply induction; exact present|apply Nat.le_max_r].
Qed.

Theorem fresh_nat_identity_is_unused :
  forall used, ~ In (fresh_nat_identity used) used.
Proof.
  intros used present.
  apply member_does_not_exceed_maximum_identity in present.
  unfold fresh_nat_identity in present. lia.
Qed.

Definition nat_fresh_identity_source : @fresh_identity_source nat :=
  fun used => exist _ (fresh_nat_identity used) (fresh_nat_identity_is_unused used).

Definition batch_within_limit {A : Type} (limit : nat) (batch : list A) : Prop :=
  length batch <= limit.

Theorem bounded_dispatch_preserves_batch_limit :
  forall (A : Type) limit (batch : list A),
    batch_within_limit limit batch -> length batch <= limit.
Proof. auto. Qed.

Theorem recovery_budget_episode_correct :
  (forall capacity charge ledger,
      In charge (durable_charges ledger) ->
      commit_recovery_charge capacity charge ledger = Some ledger) /\
  (forall maximum current,
      current <= maximum ->
      saturating_increment maximum current <= maximum) /\
  (forall used, ~ In (fresh_nat_identity used) used) /\
  (forall entry active stale progress,
      @retry_token nat entry = Some active ->
      stale <> active ->
      @finish_retry nat Nat.eq_dec stale progress entry = entry) /\
  (forall ledger, restart_recovery_ledger ledger = ledger).
Proof.
  split; [exact exact_charge_retry_is_idempotent|].
  split; [exact retry_attempts_saturate|].
  split; [exact fresh_nat_identity_is_unused|].
  split; [exact (@stale_completion_is_effect_free nat Nat.eq_dec)|].
  exact restart_preserves_durable_recovery_state.
Qed.
