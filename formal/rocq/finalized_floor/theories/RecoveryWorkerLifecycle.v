From Stdlib Require Import Bool Lists.List Arith.
Import ListNotations.

Inductive phase := Fresh | Queued | Running | BodyDropped | BytesReleased
                 | IdentityReleased | Woken | Retired | Joined.

Record worker := Worker {
  pc : phase;
  payload : bool;
  byte_lease : bool;
  identity_lease : bool;
  armed : bool;
  released_wake : bool;
  live : bool;
  attached : bool;
  cancellation : bool
}.

Definition initial := Worker Fresh false false false false false false false false.
Definition has_body (p : phase) :=
  match p with Queued | Running => true | _ => false end.
Definition has_bytes (p : phase) :=
  match p with Queued | Running | BodyDropped => true | _ => false end.
Definition has_identity (p : phase) :=
  match p with Queued | Running | BodyDropped | BytesReleased => true | _ => false end.
Definition is_armed (p : phase) :=
  match p with Queued | Running | BodyDropped | BytesReleased | IdentityReleased => true | _ => false end.
Definition has_woken (p : phase) :=
  match p with Woken | Retired | Joined => true | _ => false end.
Definition is_live (p : phase) :=
  match p with Running | BodyDropped | BytesReleased | IdentityReleased | Woken => true | _ => false end.
Definition can_attach (p : phase) :=
  match p with Running | BodyDropped | BytesReleased | IdentityReleased | Woken | Retired => true | _ => false end.

Definition well_formed (w : worker) :=
  payload w = has_body (pc w) /\
  byte_lease w = has_bytes (pc w) /\
  identity_lease w = has_identity (pc w) /\
  armed w = is_armed (pc w) /\
  released_wake w = has_woken (pc w) /\
  (live w = is_live (pc w) \/ pc w = BodyDropped \/ pc w = BytesReleased
     \/ pc w = IdentityReleased \/ pc w = Woken) /\
  (attached w = true -> can_attach (pc w) = true) /\
  (live w = true -> attached w = true \/ cancellation w = true) /\
  (cancellation w = true -> live w = true).

Inductive event := Enqueue | Dispatch | DropBody | ReleaseBytes | ReleaseIdentity
                 | PublishWake | Retire | Join | Abort | DropParent.

Definition apply (e : event) (w : worker) : worker :=
  let '(Worker p b c i a r l h x) := w in
  match e, p with
  | Enqueue, Fresh => Worker Queued true true true true r l h x
  | Dispatch, Queued => Worker Running b c i a r true true x
  | DropBody, Queued | DropBody, Running => Worker BodyDropped false c i a r l h x
  | ReleaseBytes, BodyDropped => Worker BytesReleased b false i a r l h x
  | ReleaseIdentity, BytesReleased => Worker IdentityReleased b c false a r l h x
  | PublishWake, IdentityReleased => Worker Woken b c i false true l h x
  | Retire, Woken => Worker Retired b c i a r false h false
  | Join, Retired => Worker Joined b c i a r l false x
  | Abort, _ => Worker p b c i a r l h (x || l)
  | DropParent, _ => Worker p b c i a r l false l
  | _, _ => w
  end.

Theorem initial_well_formed : well_formed initial.
Proof. repeat split; simpl; auto; discriminate. Qed.

Theorem every_step_preserves_worker : forall e w,
  well_formed w -> well_formed (apply e w).
Proof.
  intros e [p b c i a r l h x] H.
  unfold well_formed in H. cbn in H.
  destruct H as [Hb [Hc [Hi [Ha [Hr [Hl [Hh [Ho Hx]]]]]]]].
  subst b c i a r.
  destruct p, l, h, x; cbn in *; try (exfalso; intuition congruence).
  all: destruct e; cbn; unfold well_formed; cbn; intuition congruence.
Qed.

Definition population := nat -> worker.
Definition initial_population : population := fun _ => initial.
Definition perform (state : population) (op : nat * event) : population :=
  fun other => if Nat.eq_dec other (fst op) then apply (snd op) (state other) else state other.
Definition all_well_formed (state : population) := forall key, well_formed (state key).
Fixpoint history (ops : list (nat * event)) (state : population) : population :=
  match ops with [] => state | op :: rest => history rest (perform state op) end.

Theorem operation_preserves_other_workers : forall state key e other,
  other <> key -> perform state (key, e) other = state other.
Proof. intros. unfold perform. simpl. destruct Nat.eq_dec; congruence. Qed.

Theorem all_steps_preserve_population : forall state op,
  all_well_formed state -> all_well_formed (perform state op).
Proof.
  intros state [key e] H other. unfold perform; simpl.
  destruct Nat.eq_dec; [apply every_step_preserves_worker|]; apply H.
Qed.

Theorem arbitrary_interleavings_preserve_population : forall ops state,
  all_well_formed state -> all_well_formed (history ops state).
Proof.
  induction ops as [|op rest IH]; intros state H; simpl; [exact H|].
  apply IH. now apply all_steps_preserve_population.
Qed.

Theorem initial_histories_are_well_formed : forall ops,
  all_well_formed (history ops initial_population).
Proof.
  intros. apply arbitrary_interleavings_preserve_population.
  intros key. apply initial_well_formed.
Qed.

Theorem payload_remains_charged : forall w,
  well_formed w -> payload w = true -> byte_lease w = true.
Proof.
  intros [p b c i a r l h x] H B.
  unfold well_formed in H. cbn in H, B.
  destruct H as [Hb [Hc H]]. subst b c.
  destruct p; cbn in *; congruence.
Qed.

Theorem charged_payload_retains_identity : forall w,
  well_formed w -> byte_lease w = true -> identity_lease w = true.
Proof.
  intros [p b c i a r l h x] H B.
  unfold well_formed in H. cbn in H, B.
  destruct H as [Hb [Hc [Hi H]]]. subst c i.
  destruct p; cbn in *; congruence.
Qed.

Theorem wake_follows_physical_release : forall w,
  well_formed w -> released_wake w = true ->
  payload w = false /\ byte_lease w = false /\ identity_lease w = false.
Proof.
  intros [p b c i a r l h x] H R.
  unfold well_formed in H. cbn in H, R.
  destruct H as [Hb [Hc [Hi [Ha [Hr H]]]]]. subst b c i r.
  destruct p; cbn in *; intuition congruence.
Qed.

Theorem parent_drop_preserves_physical_resources : forall w,
  payload (apply DropParent w) = payload w /\
  byte_lease (apply DropParent w) = byte_lease w /\
  identity_lease (apply DropParent w) = identity_lease w /\
  live (apply DropParent w) = live w.
Proof. intros [p b c i a r l h x]. simpl. repeat split; reflexivity. Qed.

Theorem parent_drop_retains_cancellation_owner : forall w,
  live w = true -> cancellation (apply DropParent w) = true.
Proof. intros [p b c i a r l h x]. simpl. auto. Qed.

Theorem abort_does_not_retire_worker : forall w,
  pc (apply Abort w) = pc w /\ live (apply Abort w) = live w /\
  payload (apply Abort w) = payload w.
Proof. intros [p b c i a r l h x]. simpl. repeat split; reflexivity. Qed.

Theorem observed_retirement_has_no_resources : forall w,
  well_formed w -> pc w = Joined ->
  payload w = false /\ byte_lease w = false /\ identity_lease w = false /\
  live w = false /\ attached w = false /\ cancellation w = false.
Proof.
  intros [p b c i a r l h x] H P. simpl in P. subst p.
  unfold well_formed in H. cbn in H.
  destruct H as [Hb [Hc [Hi [Ha [Hr [Hl [Hh [Ho Hx]]]]]]]].
  subst b c i a r. destruct Hl as [Hl|[Hl|[Hl|[Hl|Hl]]]]; try discriminate.
  subst l. destruct h, x; cbn in *; intuition congruence.
Qed.

Definition cleanup (w : worker) : worker :=
  match pc w with
  | Queued | Running => apply DropBody w
  | BodyDropped => apply ReleaseBytes w
  | BytesReleased => apply ReleaseIdentity w
  | IdentityReleased => apply PublishWake w
  | Woken => apply Retire w
  | Retired => apply Join w
  | _ => w
  end.

Fixpoint cleanup_steps (n : nat) (w : worker) : worker :=
  match n with O => w | S rest => cleanup_steps rest (cleanup w) end.

Theorem cleanup_requires_only_finite_service : forall w,
  pc (cleanup_steps 6 w) = Fresh \/ pc (cleanup_steps 6 w) = Joined.
Proof. intros [[] b c i a r l h x]; simpl; auto. Qed.

Theorem cleanup_preserves_worker : forall w,
  well_formed w -> well_formed (cleanup w).
Proof.
  intros w H. unfold cleanup.
  destruct (pc w); try exact H; now apply every_step_preserves_worker.
Qed.

Theorem cleanup_history_preserves_worker : forall n w,
  well_formed w -> well_formed (cleanup_steps n w).
Proof.
  induction n; intros w H; simpl; [exact H|].
  apply IHn. now apply cleanup_preserves_worker.
Qed.

Theorem no_resource_survives_completed_cleanup : forall w,
  well_formed w ->
  payload (cleanup_steps 6 w) = false /\
  byte_lease (cleanup_steps 6 w) = false /\
  identity_lease (cleanup_steps 6 w) = false /\
  live (cleanup_steps 6 w) = false.
Proof.
  intros w H.
  pose proof (cleanup_history_preserves_worker 6 w H) as W.
  destruct (cleanup_requires_only_finite_service w) as [F|J].
  - destruct (cleanup_steps 6 w) as [p b c i a r l h x].
    simpl in F. subst p. unfold well_formed in W. cbn in W.
    destruct W as [Hb [Hc [Hi [Ha [Hr [Hl [Hh [Ho Hx]]]]]]]].
    subst b c i a r. destruct Hl as [Hl|[Hl|[Hl|[Hl|Hl]]]]; try discriminate.
    subst l. cbn. auto.
  - pose proof (observed_retirement_has_no_resources _ W J). tauto.
Qed.

Print Assumptions initial_well_formed.
Print Assumptions every_step_preserves_worker.
Print Assumptions operation_preserves_other_workers.
Print Assumptions all_steps_preserve_population.
Print Assumptions arbitrary_interleavings_preserve_population.
Print Assumptions initial_histories_are_well_formed.
Print Assumptions payload_remains_charged.
Print Assumptions charged_payload_retains_identity.
Print Assumptions wake_follows_physical_release.
Print Assumptions parent_drop_preserves_physical_resources.
Print Assumptions parent_drop_retains_cancellation_owner.
Print Assumptions abort_does_not_retire_worker.
Print Assumptions observed_retirement_has_no_resources.
Print Assumptions cleanup_requires_only_finite_service.
Print Assumptions cleanup_preserves_worker.
Print Assumptions cleanup_history_preserves_worker.
Print Assumptions no_resource_survives_completed_cleanup.
