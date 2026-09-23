From Coq Require Import List Arith Bool Lia.
Import ListNotations.

Definition token := (nat * nat)%type.

Record state := {
  sequence : nat;
  issued : list token
}.

Definition initial : state := {| sequence := 0; issued := [] |}.

Definition valid (s : state) : Prop :=
  NoDup (issued s) /\ Forall (fun t => snd t <= sequence s) (issued s).

Definition allocate (s : state) (nonce : nat) : state :=
  {| sequence := S (sequence s);
     issued := (nonce, S (sequence s)) :: issued s |}.

Definition advance (s : state) : state :=
  {| sequence := S (sequence s); issued := issued s |}.

Inductive event := Hello (nonce : nat) | Response | Rejected.

Definition step (s : state) (e : event) : state :=
  match e with
  | Hello nonce => allocate s nonce
  | Response => advance s
  | Rejected => s
  end.

Fixpoint run (s : state) (events : list event) : state :=
  match events with
  | [] => s
  | e :: rest => run (step s e) rest
  end.

Definition matches (left right : token) : bool :=
  Nat.eqb (fst left) (fst right) && Nat.eqb (snd left) (snd right).

Definition checked_allocate (maximum : nat) (s : state) (nonce : nat) : option state :=
  if sequence s <? maximum then Some (allocate s nonce) else None.

Lemma initial_valid : valid initial.
Proof. split; constructor. Qed.

Lemma allocation_fresh : forall s nonce,
  valid s -> ~ In (nonce, S (sequence s)) (issued s).
Proof.
  intros s nonce [_ Hbound] Hin.
  rewrite Forall_forall in Hbound.
  specialize (Hbound _ Hin). simpl in Hbound. lia.
Qed.

Lemma allocate_valid : forall s nonce, valid s -> valid (allocate s nonce).
Proof.
  intros s nonce Hvalid.
  destruct Hvalid as [Hunique Hbound]. split; simpl.
  - constructor.
    + apply allocation_fresh. split; assumption.
    + exact Hunique.
  - constructor; [simpl; lia |].
    rewrite Forall_forall in *.
    intros t Hin. specialize (Hbound t Hin). lia.
Qed.

Lemma advance_valid : forall s, valid s -> valid (advance s).
Proof.
  intros s [Hunique Hbound]. split; simpl; [exact Hunique |].
  rewrite Forall_forall in *.
  intros t Hin. specialize (Hbound t Hin). lia.
Qed.

Lemma step_valid : forall s e, valid s -> valid (step s e).
Proof.
  intros s e Hvalid. destruct e; simpl.
  - apply allocate_valid. exact Hvalid.
  - apply advance_valid. exact Hvalid.
  - exact Hvalid.
Qed.

Theorem run_valid : forall events s, valid s -> valid (run s events).
Proof.
  induction events as [| e rest IH]; intros s Hvalid; simpl.
  - exact Hvalid.
  - apply IH. apply step_valid. exact Hvalid.
Qed.

Theorem stale_request_refused : forall s nonce old,
  valid s -> In old (issued s) ->
  matches old (nonce, S (sequence s)) = false.
Proof.
  intros s nonce [old_nonce old_sequence] Hvalid Hin.
  unfold matches. simpl.
  apply Bool.andb_false_iff. right.
  apply Nat.eqb_neq.
  intros Hequal. subst old_sequence.
  apply (allocation_fresh s old_nonce Hvalid). exact Hin.
Qed.

Theorem counter_exhaustion_refused : forall maximum s nonce,
  maximum <= sequence s -> checked_allocate maximum s nonce = None.
Proof.
  intros maximum s nonce Hbound.
  unfold checked_allocate.
  assert (sequence s <? maximum = false) as Htest by (apply Nat.ltb_ge; lia).
  rewrite Htest. reflexivity.
Qed.

Theorem checked_allocation_valid : forall maximum s nonce next,
  valid s -> checked_allocate maximum s nonce = Some next -> valid next.
Proof.
  intros maximum s nonce next Hvalid Hstep.
  unfold checked_allocate in Hstep.
  destruct (sequence s <? maximum) eqn:Hlimit; inversion Hstep; subst.
  apply allocate_valid. exact Hvalid.
Qed.
