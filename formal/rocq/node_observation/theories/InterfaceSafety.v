From Coq Require Import List Arith Bool Lia.
Import ListNotations.

Record component := {
  directory : bool;
  owner : nat;
  writable : bool;
  sticky : bool;
  permissions : nat
}.

Definition safe_component (uid : nat) (c : component) : bool :=
  directory c && ((owner c =? 0) || (owner c =? uid)) &&
  (negb (writable c) || ((owner c =? 0) && sticky c)).

Definition safe_path (uid : nat) (parents : list component) (leaf : component) : bool :=
  forallb (safe_component uid) (parents ++ [leaf]) &&
  (owner leaf =? uid) && (permissions leaf =? 448).

Theorem path_admission_sound : forall uid parents leaf,
  safe_path uid parents leaf = true ->
  Forall (fun c => directory c = true /\ (owner c = 0 \/ owner c = uid) /\
    (writable c = false \/ (owner c = 0 /\ sticky c = true))) (parents ++ [leaf]) /\
  owner leaf = uid /\ permissions leaf = 448.
Proof.
  intros uid parents leaf H. unfold safe_path in H.
  apply andb_true_iff in H as [H Hp]. apply andb_true_iff in H as [H Ho].
  apply Nat.eqb_eq in Ho. apply Nat.eqb_eq in Hp.
  split; [|auto]. apply Forall_forall. intros c Hin.
  apply forallb_forall with (x := c) in H; [|exact Hin].
  unfold safe_component in H.
  apply andb_true_iff in H as [H Hw]. apply andb_true_iff in H as [Hd Ho'].
  apply orb_true_iff in Ho'. apply orb_true_iff in Hw.
  split; [exact Hd|]. split.
  - destruct Ho'; [left|right]; apply Nat.eqb_eq; assumption.
  - destruct Hw as [Hw|Hw].
    + left. apply negb_true_iff. exact Hw.
    + right. apply andb_true_iff in Hw as [Hr Hs].
      split; [apply Nat.eqb_eq|]; assumption.
Qed.

Definition peer_admitted (uid pid start actual_uid : nat)
  (actual_pid : option nat) (actual_start : nat) : bool :=
  match actual_pid with
  | None => false
  | Some p => (actual_uid =? uid) && (p =? pid) && (actual_start =? start)
  end.

Theorem peer_admission_sound : forall uid pid start actual_uid actual_pid actual_start,
  peer_admitted uid pid start actual_uid actual_pid actual_start = true ->
  actual_uid = uid /\ actual_pid = Some pid /\ actual_start = start.
Proof.
  intros uid pid start au ap ast H. destruct ap as [p|]; [|discriminate].
  unfold peer_admitted in H.
  apply andb_true_iff in H as [H Hs]. apply andb_true_iff in H as [Hu Hp].
  apply Nat.eqb_eq in Hu. apply Nat.eqb_eq in Hp. apply Nat.eqb_eq in Hs.
  subst. auto.
Qed.

Record entry := { socket : bool; device : nat; inode : nat }.

Definition cleanup (dev ino : nat) (current : option entry) : option entry :=
  match current with
  | None => None
  | Some e => if socket e && (device e =? dev) && (inode e =? ino)
              then None else Some e
  end.

Theorem cleanup_preserves_replacement : forall dev ino e,
  socket e = false \/ device e <> dev \/ inode e <> ino ->
  cleanup dev ino (Some e) = Some e.
Proof.
  intros dev ino e H. unfold cleanup.
  destruct (socket e && (device e =? dev) && (inode e =? ino)) eqn:He; [|reflexivity].
  apply andb_true_iff in He as [He Hi]. apply andb_true_iff in He as [Hs Hd].
  apply Nat.eqb_eq in Hd. apply Nat.eqb_eq in Hi.
  destruct H as [H|[H|H]]; congruence.
Qed.

Inductive lifecycle := Running | Returned | Cancelling | Joined | Exited.
Inductive lifecycle_event := Return | Cancel | Join | Exit.

Definition shutdown_step (s : lifecycle * bool) (a : lifecycle_event)
  : option (lifecycle * bool) :=
  match fst s, a with
  | Running, Return => Some (Returned, snd s)
  | Returned, Cancel => Some (Cancelling, snd s)
  | Cancelling, Join => Some (Joined, true)
  | Joined, Exit => Some (Exited, snd s)
  | _, _ => None
  end.

Fixpoint shutdown_run (s : lifecycle * bool) (actions : list lifecycle_event)
  : option (lifecycle * bool) :=
  match actions with
  | [] => Some s
  | a :: rest => match shutdown_step s a with
                 | Some next => shutdown_run next rest
                 | None => None
                 end
  end.

Definition cleanup_joined (s : lifecycle * bool) : Prop :=
  fst s = Joined \/ fst s = Exited -> snd s = true.

Lemma shutdown_step_joined : forall s a next,
  cleanup_joined s -> shutdown_step s a = Some next -> cleanup_joined next.
Proof.
  intros [p cleaned] a next H Hs. destruct p, a; simpl in Hs;
    try discriminate; inversion Hs; subst; unfold cleanup_joined in *; simpl in *;
    intuition congruence.
Qed.

Theorem shutdown_run_joined : forall actions s final,
  cleanup_joined s -> shutdown_run s actions = Some final -> cleanup_joined final.
Proof.
  induction actions as [|a rest IH]; intros s final H Hr; simpl in Hr.
  - inversion Hr; subst. exact H.
  - destruct (shutdown_step s a) as [next|] eqn:Hs; [|discriminate].
    eapply IH; [eapply shutdown_step_joined; eauto|exact Hr].
Qed.

Theorem shutdown_before_exit : forall actions cleaned,
  shutdown_run (Running, false) actions = Some (Exited, cleaned) -> cleaned = true.
Proof.
  intros actions cleaned H.
  assert (cleanup_joined (Running, false)) as Hi by (unfold cleanup_joined; simpl; intuition discriminate).
  pose proof (shutdown_run_joined actions _ _ Hi H) as Hr.
  apply Hr. right. reflexivity.
Qed.

Definition write_admitted (deadline now : nat) : bool := now <? deadline.
Definition lock_admitted (deadline now : nat) (contended : bool) : bool :=
  negb contended || (now <? deadline).

Theorem write_before_deadline : forall deadline now,
  write_admitted deadline now = true -> now < deadline.
Proof. intros. apply Nat.ltb_lt. exact H. Qed.

Theorem contended_lock_before_deadline : forall deadline now,
  lock_admitted deadline now true = true -> now < deadline.
Proof. intros. apply Nat.ltb_lt. exact H. Qed.

Theorem lock_sequence_uses_one_deadline : forall deadline attempts,
  forallb (fun a => lock_admitted deadline (fst a) (snd a)) attempts = true ->
  forall now, In (now, true) attempts -> now < deadline.
Proof.
  intros deadline attempts H now Hin.
  apply forallb_forall with (x := (now, true)) in H; [|exact Hin].
  apply contended_lock_before_deadline. exact H.
Qed.
