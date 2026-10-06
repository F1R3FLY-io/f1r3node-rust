From Coq Require Import List Arith Lia Bool.
Import ListNotations.

Inductive attach_result := Attached | AlreadyAttached.

Definition cell := option nat.

Definition attach (c : cell) (b : nat) : cell * attach_result :=
  match c with
  | None => (Some b, Attached)
  | Some x => (Some x, AlreadyAttached)
  end.

Fixpoint attach_all (c : cell) (bs : list nat) : cell * list attach_result :=
  match bs with
  | [] => (c, [])
  | b :: rest =>
      match attach c b with
      | (c', r) =>
          match attach_all c' rest with
          | (c'', rs) => (c'', r :: rs)
          end
      end
  end.

Definition is_attached (r : attach_result) : bool :=
  match r with Attached => true | AlreadyAttached => false end.

Definition successes (rs : list attach_result) : nat := length (filter is_attached rs).

Lemma attach_all_some : forall bs x,
  attach_all (Some x) bs = (Some x, map (fun _ => AlreadyAttached) bs).
Proof.
  induction bs as [| b rest IH]; intros x; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
Qed.

Lemma successes_refused : forall bs,
  successes (map (fun _ : nat => AlreadyAttached) bs) = 0.
Proof.
  induction bs as [| b rest IH]; simpl.
  - reflexivity.
  - unfold successes in *. simpl. exact IH.
Qed.

Theorem instance_attaches_at_most_once : forall bs c rs,
  attach_all None bs = (c, rs) -> successes rs <= 1.
Proof.
  intros bs c rs H.
  destruct bs as [| b rest]; simpl in H.
  - inversion H; subst. unfold successes. simpl. lia.
  - rewrite attach_all_some in H. simpl in H. inversion H; subst.
    pose proof (successes_refused rest) as Hr. unfold successes in *. simpl in *. lia.
Qed.

Theorem first_attachment_wins : forall b rest c rs,
  attach_all None (b :: rest) = (c, rs) -> c = Some b.
Proof.
  intros b rest c rs H. simpl in H.
  rewrite attach_all_some in H. simpl in H. inversion H. reflexivity.
Qed.

Record controller := {
  current_installation : nat;
  live : option nat
}.

Definition install (bound : nat) (c : controller) : controller :=
  if S (current_installation c) <=? bound
  then {| current_installation := S (current_installation c);
          live := Some (S (current_installation c)) |}
  else {| current_installation := current_installation c; live := None |}.

Definition shutdown (c : controller) : controller :=
  {| current_installation := current_installation c; live := None |}.

Definition accepted (c : controller) (b : nat) : bool :=
  match live c with
  | Some x => (x =? b) && (b =? current_installation c)
  | None => false
  end.

Theorem replaced_binding_refused : forall bound c b,
  accepted c b = true -> accepted (install bound c) b = false.
Proof.
  intros bound c b Hacc. unfold accepted in Hacc.
  destruct (live c) as [x |] eqn:Hlive; [| discriminate].
  apply andb_true_iff in Hacc. destruct Hacc as [_ Hb].
  apply Nat.eqb_eq in Hb.
  unfold install, accepted.
  destruct (S (current_installation c) <=? bound); cbn [live current_installation].
  - destruct (S (current_installation c) =? b) eqn:Heq.
    + apply Nat.eqb_eq in Heq. lia.
    + reflexivity.
  - reflexivity.
Qed.

Theorem shutdown_refuses_all : forall c b, accepted (shutdown c) b = false.
Proof. intros c b. reflexivity. Qed.

Theorem installation_never_wraps : forall bound c,
  current_installation (install bound c) <= bound \/
  current_installation (install bound c) = current_installation c.
Proof.
  intros bound c. unfold install.
  destruct (S (current_installation c) <=? bound) eqn:H; simpl.
  - left. apply Nat.leb_le in H. exact H.
  - right. reflexivity.
Qed.

Record ledger := {
  attempted : nat;
  delivered : nat;
  lost : nat;
  queued : nat;
  overflow : bool
}.

Inductive attempt := ValidHash | InvalidHash.

Inductive step := Emit (a : attempt) | Drain.

Definition initial_ledger : ledger :=
  {| attempted := 0; delivered := 0; lost := 0; queued := 0; overflow := false |}.

Definition emit (cap bound : nat) (active : bool) (l : ledger) (a : attempt) : ledger :=
  if negb active then l
  else if bound <=? attempted l
  then {| attempted := attempted l; delivered := delivered l; lost := lost l;
          queued := queued l; overflow := true |}
  else match a with
       | InvalidHash =>
           {| attempted := S (attempted l); delivered := delivered l; lost := S (lost l);
              queued := queued l; overflow := overflow l |}
       | ValidHash =>
           if queued l <? cap
           then {| attempted := S (attempted l); delivered := S (delivered l); lost := lost l;
                   queued := S (queued l); overflow := overflow l |}
           else {| attempted := S (attempted l); delivered := delivered l; lost := S (lost l);
                   queued := queued l; overflow := overflow l |}
       end.

Definition drain (l : ledger) : ledger :=
  {| attempted := attempted l; delivered := delivered l; lost := lost l;
     queued := pred (queued l); overflow := overflow l |}.

Definition ledger_step (cap bound : nat) (active : bool) (l : ledger) (s : step) : ledger :=
  match s with
  | Emit a => emit cap bound active l a
  | Drain => drain l
  end.

Definition ledger_run (cap bound : nat) (active : bool) (steps : list step) : ledger :=
  fold_left (ledger_step cap bound active) steps initial_ledger.

Definition accounted (l : ledger) : Prop := attempted l = delivered l + lost l.

Definition within (cap : nat) (l : ledger) : Prop := queued l <= cap.

Lemma emit_accounted : forall cap bound active l a,
  accounted l -> accounted (emit cap bound active l a).
Proof.
  intros cap bound active l a Hacc. unfold accounted in *. unfold emit.
  destruct (negb active); [exact Hacc |].
  destruct (bound <=? attempted l); [exact Hacc |].
  destruct a; simpl.
  - destruct (queued l <? cap); simpl; lia.
  - lia.
Qed.

Lemma emit_within : forall cap bound active l a,
  within cap l -> within cap (emit cap bound active l a).
Proof.
  intros cap bound active l a Hw. unfold within in *. unfold emit.
  destruct (negb active); [exact Hw |].
  destruct (bound <=? attempted l); [exact Hw |].
  destruct a; simpl.
  - destruct (queued l <? cap) eqn:Hq; simpl.
    + apply Nat.ltb_lt in Hq. lia.
    + exact Hw.
  - exact Hw.
Qed.

Lemma step_invariant : forall cap bound active l s,
  accounted l /\ within cap l ->
  accounted (ledger_step cap bound active l s) /\ within cap (ledger_step cap bound active l s).
Proof.
  intros cap bound active l s [Hacc Hw]. destruct s; simpl.
  - split; [apply emit_accounted | apply emit_within]; assumption.
  - unfold accounted, within, drain in *. simpl. split; [exact Hacc | lia].
Qed.

Lemma fold_invariant : forall cap bound active steps l,
  accounted l /\ within cap l ->
  accounted (fold_left (ledger_step cap bound active) steps l) /\
  within cap (fold_left (ledger_step cap bound active) steps l).
Proof.
  intros cap bound active steps. induction steps as [| s rest IH]; intros l H; simpl.
  - exact H.
  - apply IH. apply step_invariant. exact H.
Qed.

Theorem queue_never_exceeds_capacity : forall cap bound active steps,
  queued (ledger_run cap bound active steps) <= cap.
Proof.
  intros cap bound active steps.
  apply (proj2 (fold_invariant cap bound active steps initial_ledger
    (conj (eq_refl : accounted initial_ledger) (Nat.le_0_l cap)))).
Qed.

Theorem ledger_accounts_for_every_attempt : forall cap bound active steps,
  attempted (ledger_run cap bound active steps) =
  delivered (ledger_run cap bound active steps) + lost (ledger_run cap bound active steps).
Proof.
  intros cap bound active steps.
  apply (proj1 (fold_invariant cap bound active steps initial_ledger
    (conj (eq_refl : accounted initial_ledger) (Nat.le_0_l cap)))).
Qed.

Definition complete (active : bool) (l : ledger) : Prop :=
  active = true /\ lost l = 0 /\ overflow l = false.

Theorem complete_coverage_delivers_every_attempt : forall cap bound active steps,
  complete active (ledger_run cap bound active steps) ->
  delivered (ledger_run cap bound active steps) = attempted (ledger_run cap bound active steps).
Proof.
  intros cap bound active steps [_ [Hlost _]].
  rewrite (ledger_accounts_for_every_attempt cap bound active steps). lia.
Qed.

Theorem sequence_exhaustion_refused : forall cap bound l a,
  bound <= attempted l ->
  attempted (emit cap bound true l a) = attempted l /\ overflow (emit cap bound true l a) = true.
Proof.
  intros cap bound l a Hle. unfold emit. simpl.
  destruct (bound <=? attempted l) eqn:H.
  - simpl. split; reflexivity.
  - apply Nat.leb_gt in H. lia.
Qed.
