From Stdlib Require Import Bool Arith Lists.List Lia.
Import ListNotations.

Record Resources := resources {
  pending_owner : option nat;
  active_owner : option nat;
  physical_episode : nat -> bool;
  capture_permission : nat -> bool
}.

Definition set_flag (f : nat -> bool) key value :=
  fun candidate => if Nat.eqb candidate key then value else f candidate.

Definition supported s key :=
  pending_owner s = Some key \/ active_owner s = Some key.

Definition conserved s := forall key,
  physical_episode s key = true \/ capture_permission s key = true -> supported s key.

Definition initial := resources None None (fun _ => false) (fun _ => false).

Definition reserve s key :=
  resources (Some key) (active_owner s) (physical_episode s) (capture_permission s).

Definition begin_capture s key :=
  resources (pending_owner s) (active_owner s)
    (set_flag (physical_episode s) key true) (set_flag (capture_permission s) key true).

Definition finish_capture s key :=
  resources (pending_owner s) (active_owner s) (physical_episode s)
    (set_flag (capture_permission s) key false).

Definition transfer s key :=
  resources None (Some key) (physical_episode s) (capture_permission s).

Definition destroy s key :=
  resources (pending_owner s) (active_owner s)
    (set_flag (physical_episode s) key false) (capture_permission s).

Definition release_pending s :=
  resources None (active_owner s) (physical_episode s) (capture_permission s).

Definition release_active s :=
  resources (pending_owner s) None (physical_episode s) (capture_permission s).

Inductive ResourceStep : Resources -> Resources -> Prop :=
| ReserveStep : forall s key,
    pending_owner s = None -> active_owner s <> Some key ->
    ResourceStep s (reserve s key)
| BeginCaptureStep : forall s key,
    pending_owner s = Some key -> ResourceStep s (begin_capture s key)
| FinishCaptureStep : forall s key,
    ResourceStep s (finish_capture s key)
| TransferStep : forall s key,
    pending_owner s = Some key -> active_owner s = None ->
    capture_permission s key = false -> ResourceStep s (transfer s key)
| DestroyStep : forall s key,
    capture_permission s key = false -> ResourceStep s (destroy s key)
| ReleasePendingStep : forall s key,
    pending_owner s = Some key -> physical_episode s key = false ->
    capture_permission s key = false -> ResourceStep s (release_pending s)
| ReleaseActiveStep : forall s key,
    active_owner s = Some key -> physical_episode s key = false ->
    capture_permission s key = false -> ResourceStep s (release_active s)
| ControlStep : forall s, ResourceStep s s.

Theorem initial_conserved : conserved initial.
Proof. intros key [H|H]; discriminate. Qed.

Theorem reserve_conserved : forall s key,
  conserved s -> pending_owner s = None -> conserved (reserve s key).
Proof.
  intros s key H P candidate R. specialize (H candidate R).
  destruct H as [H|H]; [rewrite P in H; discriminate|]. right. exact H.
Qed.

Theorem begin_capture_conserved : forall s key,
  conserved s -> pending_owner s = Some key -> conserved (begin_capture s key).
Proof.
  intros s key H P candidate R. unfold begin_capture in R. simpl in R.
  unfold set_flag in R. destruct (Nat.eqb candidate key) eqn:E.
  - apply Nat.eqb_eq in E. subst. left. exact P.
  - exact (H candidate R).
Qed.

Theorem finish_capture_conserved : forall s key,
  conserved s -> conserved (finish_capture s key).
Proof.
  intros s key H candidate R. unfold finish_capture in R. simpl in R.
  destruct R as [R|R].
  - apply H. left. exact R.
  - unfold set_flag in R. destruct (Nat.eqb candidate key); [discriminate|].
    apply H. right. exact R.
Qed.

Theorem transfer_conserved : forall s key,
  conserved s -> pending_owner s = Some key -> active_owner s = None ->
  conserved (transfer s key).
Proof.
  intros s key H P A candidate R. specialize (H candidate R).
  destruct H as [H|H].
  - rewrite P in H. inversion H. subst. right. reflexivity.
  - rewrite A in H. discriminate.
Qed.

Theorem destroy_conserved : forall s key,
  conserved s -> conserved (destroy s key).
Proof.
  intros s key H candidate R. unfold destroy in R. simpl in R.
  destruct R as [R|R].
  - unfold set_flag in R. destruct (Nat.eqb candidate key); [discriminate|].
    apply H. left. exact R.
  - apply H. right. exact R.
Qed.

Theorem release_pending_conserved : forall s key,
  conserved s -> pending_owner s = Some key -> physical_episode s key = false ->
  capture_permission s key = false -> conserved (release_pending s).
Proof.
  intros s key H P R B candidate Live. specialize (H candidate Live).
  destruct H as [H|H]; [|right; exact H].
  rewrite P in H. inversion H. subst. destruct Live as [Live|Live].
  - simpl in Live. rewrite R in Live. discriminate.
  - simpl in Live. rewrite B in Live. discriminate.
Qed.

Theorem release_active_conserved : forall s key,
  conserved s -> active_owner s = Some key -> physical_episode s key = false ->
  capture_permission s key = false -> conserved (release_active s).
Proof.
  intros s key H A R B candidate Live. specialize (H candidate Live).
  destruct H as [H|H]; [left; exact H|].
  rewrite A in H. inversion H. subst. destruct Live as [Live|Live].
  - simpl in Live. rewrite R in Live. discriminate.
  - simpl in Live. rewrite B in Live. discriminate.
Qed.

Theorem resource_step_preserves_conservation : forall s next,
  ResourceStep s next -> conserved s -> conserved next.
Proof.
  intros s next Step H. destruct Step.
  - eapply reserve_conserved; eauto.
  - eapply begin_capture_conserved; eauto.
  - eapply finish_capture_conserved; eauto.
  - eapply transfer_conserved; eauto.
  - eapply destroy_conserved; eauto.
  - eapply release_pending_conserved; eauto.
  - eapply release_active_conserved; eauto.
  - exact H.
Qed.

Inductive ResourceHistory : Resources -> Resources -> Prop :=
| HistoryNil : forall s, ResourceHistory s s
| HistoryCons : forall s middle last,
    ResourceStep s middle -> ResourceHistory middle last -> ResourceHistory s last.

Theorem history_preserves_conservation : forall s last,
  ResourceHistory s last -> conserved s -> conserved last.
Proof.
  intros s last H. induction H; intros C; [exact C|].
  apply IHResourceHistory. eapply resource_step_preserves_conservation; eauto.
Qed.

Definition slot_list s :=
  match pending_owner s with None => [] | Some key => [key] end ++
  match active_owner s with None => [] | Some key => [key] end.

Theorem supported_in_slot_list : forall s key,
  supported s key -> In key (slot_list s).
Proof.
  intros s key [H|H]; unfold slot_list; apply in_or_app.
  - left. rewrite H. simpl. auto.
  - right. rewrite H. simpl. auto.
Qed.

Theorem slot_list_length_bound : forall s, length (slot_list s) <= 2.
Proof.
  intros s. unfold slot_list.
  destruct (pending_owner s); destruct (active_owner s); simpl; lia.
Qed.

Theorem conserved_episode_bound : forall s episodes,
  conserved s -> NoDup episodes ->
  (forall key, In key episodes -> physical_episode s key = true) ->
  length episodes <= 2.
Proof.
  intros s episodes C N R. assert (I : incl episodes (slot_list s)).
  { intros key H. apply supported_in_slot_list. apply C. left. apply R. exact H. }
  pose proof (NoDup_incl_length N I). pose proof (slot_list_length_bound s). lia.
Qed.

Theorem arbitrary_history_episode_bound : forall last episodes,
  ResourceHistory initial last -> NoDup episodes ->
  (forall key, In key episodes -> physical_episode last key = true) ->
  length episodes <= 2.
Proof.
  intros last episodes H N R. eapply conserved_episode_bound; eauto.
  eapply history_preserves_conservation; [exact H|exact initial_conserved].
Qed.

Theorem arbitrary_history_capture_covered : forall last key,
  ResourceHistory initial last -> capture_permission last key = true -> supported last key.
Proof.
  intros last key H P.
  pose proof (history_preserves_conservation initial last H initial_conserved) as C.
  apply C. right. exact P.
Qed.

Theorem transfer_preserves_physical_episodes : forall s key,
  physical_episode (transfer s key) = physical_episode s.
Proof. reflexivity. Qed.

Theorem control_preserves_capture_permission : forall s,
  ResourceStep s s /\ capture_permission s = capture_permission s.
Proof. intros. split; [constructor|reflexivity]. Qed.

Theorem absent_roots_do_not_authorize_capture_release : forall s key,
  capture_permission s key = true ->
  ~ (physical_episode s key = false /\ capture_permission s key = false).
Proof. intros s key H [_ F]. rewrite H in F. discriminate. Qed.

Record EpisodeLedger := episode_ledger {
  memory : Resources;
  begun_episodes : list nat;
  live_episodes : list nat;
  live_builders : list nat
}.

Definition ledger_initial := episode_ledger initial [] [] [].

Definition ledger_invariant ledger :=
  conserved (memory ledger) /\
  NoDup (begun_episodes ledger) /\ NoDup (live_episodes ledger) /\
  incl (live_episodes ledger) (begun_episodes ledger) /\
  NoDup (live_builders ledger) /\ incl (live_builders ledger) (live_episodes ledger) /\
  (forall key, physical_episode (memory ledger) key = true <-> In key (live_episodes ledger)) /\
  (forall key, capture_permission (memory ledger) key = true <-> In key (live_builders ledger)).

Inductive EpisodeStep : EpisodeLedger -> EpisodeLedger -> Prop :=
| EpisodeReserve : forall s key,
    pending_owner (memory s) = None -> active_owner (memory s) <> Some key ->
    ~ In key (begun_episodes s) ->
    EpisodeStep s (episode_ledger (reserve (memory s) key)
      (begun_episodes s) (live_episodes s) (live_builders s))
| EpisodeBegin : forall s key,
    pending_owner (memory s) = Some key -> ~ In key (begun_episodes s) ->
    EpisodeStep s (episode_ledger (begin_capture (memory s) key)
      (key :: begun_episodes s) (key :: live_episodes s) (key :: live_builders s))
| EpisodeFinish : forall s key,
    EpisodeStep s (episode_ledger (finish_capture (memory s) key)
      (begun_episodes s) (live_episodes s) (remove Nat.eq_dec key (live_builders s)))
| EpisodeTransfer : forall s key,
    pending_owner (memory s) = Some key -> active_owner (memory s) = None ->
    ~ In key (live_builders s) ->
    EpisodeStep s (episode_ledger (transfer (memory s) key)
      (begun_episodes s) (live_episodes s) (live_builders s))
| EpisodeDestroy : forall s key,
    ~ In key (live_builders s) ->
    EpisodeStep s (episode_ledger (destroy (memory s) key)
      (begun_episodes s) (remove Nat.eq_dec key (live_episodes s)) (live_builders s))
| EpisodeReleasePending : forall s key,
    pending_owner (memory s) = Some key -> ~ In key (live_episodes s) ->
    ~ In key (live_builders s) ->
    EpisodeStep s (episode_ledger (release_pending (memory s))
      (begun_episodes s) (live_episodes s) (live_builders s))
| EpisodeReleaseActive : forall s key,
    active_owner (memory s) = Some key -> ~ In key (live_episodes s) ->
    ~ In key (live_builders s) ->
    EpisodeStep s (episode_ledger (release_active (memory s))
      (begun_episodes s) (live_episodes s) (live_builders s))
| EpisodeControl : forall s, EpisodeStep s s.

Lemma remove_membership : forall key candidate values,
  In candidate (remove Nat.eq_dec key values) <-> In candidate values /\ candidate <> key.
Proof.
  intros key candidate values. split.
  - apply in_remove.
  - intros [H N]. apply in_in_remove; assumption.
Qed.

Lemma remove_nodup : forall key values,
  NoDup values -> NoDup (remove Nat.eq_dec key values).
Proof.
  intros key values H. induction H; simpl; [constructor|].
  destruct (Nat.eq_dec key x); [exact IHNoDup|]. constructor; [|exact IHNoDup].
  intro I. apply remove_membership in I. tauto.
Qed.

Lemma clear_flag_correspondence : forall f values key,
  (forall candidate, f candidate = true <-> In candidate values) ->
  forall candidate, set_flag f key false candidate = true <->
    In candidate (remove Nat.eq_dec key values).
Proof.
  intros f values key H candidate. rewrite remove_membership.
  unfold set_flag. destruct (Nat.eqb candidate key) eqn:E.
  - apply Nat.eqb_eq in E. subst. split; [discriminate|tauto].
  - apply Nat.eqb_neq in E. rewrite H. tauto.
Qed.

Lemma set_flag_correspondence : forall f values key,
  (forall candidate, f candidate = true <-> In candidate values) ->
  forall candidate, set_flag f key true candidate = true <-> In candidate (key :: values).
Proof.
  intros f values key H candidate. unfold set_flag.
  destruct (Nat.eqb candidate key) eqn:E; simpl.
  - apply Nat.eqb_eq in E. subst. tauto.
  - apply Nat.eqb_neq in E. rewrite H. intuition congruence.
Qed.

Lemma absent_flag_false : forall (f : nat -> bool) values key,
  (forall candidate, f candidate = true <-> In candidate values) ->
  ~ In key values -> f key = false.
Proof.
  intros f values key H N. destruct (f key) eqn:E; [|reflexivity].
  exfalso. apply N. apply H. exact E.
Qed.

Theorem ledger_initial_invariant : ledger_invariant ledger_initial.
Proof.
  unfold ledger_invariant, ledger_initial. simpl.
  split; [exact initial_conserved|].
  repeat split; try constructor; unfold incl; simpl; intuition discriminate.
Qed.

Theorem episode_step_preserves_ledger : forall before after,
  EpisodeStep before after -> ledger_invariant before -> ledger_invariant after.
Proof.
  intros before after Step Inv. destruct Inv as [C [N [L [I [B [J [R F]]]]]]].
  destruct Step; unfold ledger_invariant; simpl in *.
  - split; [eapply reserve_conserved; eauto|]. repeat first [assumption | split].
  - assert (NL : ~ In key (live_episodes s)) by (intro X; apply H0; apply I; exact X).
    assert (NB : ~ In key (live_builders s)) by (intro X; apply NL; apply J; exact X).
    split; [eapply begin_capture_conserved; eauto|].
    split; [constructor; auto|]. split; [constructor; auto|].
    split; [intros candidate [E|X]; simpl; [left; exact E|right; apply I; exact X]|].
    split; [constructor; auto|].
    split; [intros candidate [E|X]; simpl; [left; exact E|right; apply J; exact X]|].
    split; apply set_flag_correspondence; assumption.
  - split; [apply finish_capture_conserved; exact C|].
    split; [exact N|]. split; [exact L|]. split; [exact I|].
    split; [apply remove_nodup; exact B|].
    split; [intros candidate X; apply remove_membership in X; apply J; tauto|].
    split; [exact R|]. apply clear_flag_correspondence. exact F.
  - split; [eapply transfer_conserved; eauto|]. repeat first [assumption | split].
  - split; [apply destroy_conserved; exact C|].
    split; [exact N|]. split; [apply remove_nodup; exact L|].
    split; [intros candidate X; apply remove_membership in X; apply I; tauto|].
    split; [exact B|].
    split.
    + intros candidate X. apply remove_membership. split; [apply J; exact X|].
      intro E. subst. contradiction.
    + split; [apply clear_flag_correspondence; exact R|exact F].
  - split.
    + eapply release_pending_conserved; eauto; eapply absent_flag_false; eauto.
    + repeat first [assumption | split].
  - split.
    + eapply release_active_conserved; eauto; eapply absent_flag_false; eauto.
    + repeat first [assumption | split].
  - repeat first [assumption | split].
Qed.

Inductive EpisodeHistory : EpisodeLedger -> EpisodeLedger -> Prop :=
| EpisodeHistoryNil : forall s, EpisodeHistory s s
| EpisodeHistoryCons : forall s middle last,
    EpisodeStep s middle -> EpisodeHistory middle last -> EpisodeHistory s last.

Theorem episode_history_preserves_ledger : forall before after,
  EpisodeHistory before after -> ledger_invariant before -> ledger_invariant after.
Proof.
  intros before after H. induction H; intro Inv; [exact Inv|].
  apply IHEpisodeHistory. eapply episode_step_preserves_ledger; eauto.
Qed.

Theorem arbitrary_history_physical_episode_bound : forall last,
  EpisodeHistory ledger_initial last -> length (live_episodes last) <= 2.
Proof.
  intros last H.
  pose proof (episode_history_preserves_ledger ledger_initial last H ledger_initial_invariant)
    as [C [_ [N [_ [_ [_ [R _]]]]]]].
  eapply conserved_episode_bound; eauto. intros key I. apply R. exact I.
Qed.

Theorem arbitrary_history_no_duplicate_capture : forall last,
  EpisodeHistory ledger_initial last -> NoDup (begun_episodes last).
Proof.
  intros last H.
  pose proof (episode_history_preserves_ledger ledger_initial last H ledger_initial_invariant)
    as [_ [N _]]. exact N.
Qed.

Theorem arbitrary_history_builder_is_charged : forall last key,
  EpisodeHistory ledger_initial last -> In key (live_builders last) ->
  In key (live_episodes last) /\ supported (memory last) key.
Proof.
  intros last key H B.
  pose proof (episode_history_preserves_ledger ledger_initial last H ledger_initial_invariant)
    as [C [_ [_ [_ [_ [J [R _]]]]]]].
  split; [apply J; exact B|]. apply C. left. apply R. apply J. exact B.
Qed.

Theorem episode_step_projects_to_resource_step : forall before after,
  EpisodeStep before after -> ledger_invariant before -> ResourceStep (memory before) (memory after).
Proof.
  intros before after Step [_ [_ [_ [_ [_ [_ [R F]]]]]]].
  destruct Step; simpl in *.
  - apply ReserveStep; assumption.
  - apply BeginCaptureStep; assumption.
  - apply FinishCaptureStep.
  - apply TransferStep with (key := key); auto. eapply absent_flag_false; eauto.
  - apply DestroyStep. eapply absent_flag_false; eauto.
  - apply ReleasePendingStep with (key := key); auto; eapply absent_flag_false; eauto.
  - apply ReleaseActiveStep with (key := key); auto; eapply absent_flag_false; eauto.
  - apply ControlStep.
Qed.

Definition separate_roles s := forall key,
  pending_owner s = Some key -> active_owner s <> Some key.

Theorem resource_step_preserves_separate_roles : forall before after,
  ResourceStep before after -> separate_roles before -> separate_roles after.
Proof.
  intros before after Step S candidate P. destruct Step; simpl in *.
  - inversion P. subst. assumption.
  - apply S. exact P.
  - apply S. exact P.
  - discriminate.
  - apply S. exact P.
  - discriminate.
  - discriminate.
  - apply S. exact P.
Qed.

Definition builders_pending s := forall key,
  capture_permission s key = true -> pending_owner s = Some key.

Theorem resource_step_preserves_pending_builders : forall before after,
  ResourceStep before after -> builders_pending before -> builders_pending after.
Proof.
  intros before after Step B candidate P. destruct Step; simpl in *.
  - specialize (B candidate P). congruence.
  - unfold set_flag in P. destruct (Nat.eqb candidate key) eqn:E.
    + apply Nat.eqb_eq in E. subst. assumption.
    + apply B. exact P.
  - unfold set_flag in P. destruct (Nat.eqb candidate key); [discriminate|]. apply B. exact P.
  - specialize (B candidate P). assert (candidate = key) by congruence. subst. congruence.
  - apply B. exact P.
  - specialize (B candidate P). assert (candidate = key) by congruence. subst. congruence.
  - apply B. exact P.
  - apply B. exact P.
Qed.

Theorem arbitrary_history_role_and_builder_separation : forall last,
  ResourceHistory initial last -> separate_roles last /\ builders_pending last.
Proof.
  intros last H.
  assert (All : forall before after, ResourceHistory before after ->
    separate_roles before -> builders_pending before -> separate_roles after /\ builders_pending after).
  {
    intros before after History. induction History; intros S B; [auto|].
    apply IHHistory.
    - eapply resource_step_preserves_separate_roles; eauto.
    - eapply resource_step_preserves_pending_builders; eauto.
  }
  eapply All; [exact H| |]; intros key P; discriminate.
Qed.

Print Assumptions initial_conserved.
Print Assumptions reserve_conserved.
Print Assumptions begin_capture_conserved.
Print Assumptions finish_capture_conserved.
Print Assumptions transfer_conserved.
Print Assumptions destroy_conserved.
Print Assumptions release_pending_conserved.
Print Assumptions release_active_conserved.
Print Assumptions resource_step_preserves_conservation.
Print Assumptions history_preserves_conservation.
Print Assumptions supported_in_slot_list.
Print Assumptions slot_list_length_bound.
Print Assumptions conserved_episode_bound.
Print Assumptions arbitrary_history_episode_bound.
Print Assumptions arbitrary_history_capture_covered.
Print Assumptions transfer_preserves_physical_episodes.
Print Assumptions control_preserves_capture_permission.
Print Assumptions absent_roots_do_not_authorize_capture_release.
Print Assumptions ledger_initial_invariant.
Print Assumptions episode_step_preserves_ledger.
Print Assumptions episode_history_preserves_ledger.
Print Assumptions arbitrary_history_physical_episode_bound.
Print Assumptions arbitrary_history_no_duplicate_capture.
Print Assumptions arbitrary_history_builder_is_charged.
Print Assumptions episode_step_projects_to_resource_step.
Print Assumptions resource_step_preserves_separate_roles.
Print Assumptions resource_step_preserves_pending_builders.
Print Assumptions arbitrary_history_role_and_builder_separation.
