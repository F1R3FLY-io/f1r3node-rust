From Coq Require Import List Arith Lia.
Import ListNotations.

Definition clock := nat -> nat -> nat.

Definition monotone (c : clock) : Prop :=
  forall t e, c t e <= c (S t) e.

Definition commit_at (c : clock) (t e : nat) : Prop :=
  c t e < c (S t) e.

Lemma monotone_le : forall c, monotone c ->
  forall e t1 t2, t1 <= t2 -> c t1 e <= c t2 e.
Proof.
  intros c Hm e t1 t2 Hle.
  induction Hle as [| m Hle IH].
  - lia.
  - specialize (Hm m e). lia.
Qed.

Theorem equal_endpoints_no_commit : forall c, monotone c ->
  forall e t1 t2, t1 <= t2 -> c t1 e = c t2 e ->
  forall t, t1 <= t -> t < t2 -> ~ commit_at c t e.
Proof.
  intros c Hm e t1 t2 Hle Heq t Ht1 Ht2 Hc.
  unfold commit_at in Hc.
  assert (c t1 e <= c t e) as Hlow by (apply monotone_le; assumption).
  assert (c (S t) e <= c t2 e) as Hhigh by (apply monotone_le; [assumption | lia]).
  lia.
Qed.

Record observation := {
  participants : list nat;
  before_at : nat;
  open_at : nat;
  validate_at : nat
}.

Definition well_ordered (o : observation) : Prop :=
  before_at o <= open_at o /\ open_at o <= validate_at o.

Definition observed_consistent (c : clock) (o : observation) : Prop :=
  forall e, In e (participants o) ->
    c (before_at o) e = c (open_at o) e /\ c (open_at o) e = c (validate_at o) e.

Theorem accepted_capture_no_interference : forall c o,
  monotone c -> well_ordered o -> observed_consistent c o ->
  forall e, In e (participants o) ->
  forall t, before_at o <= t -> t < validate_at o -> ~ commit_at c t e.
Proof.
  intros c o Hm [Hbo Hov] Hcons e Hin t Ht1 Ht2.
  destruct (Hcons e Hin) as [H1 H2].
  assert (c (before_at o) e = c (validate_at o) e) as Hends by lia.
  assert (before_at o <= validate_at o) as Hspan by lia.
  exact (equal_endpoints_no_commit c Hm e (before_at o) (validate_at o) Hspan Hends t Ht1 Ht2).
Qed.

Definition generation_clock (g : nat -> nat) : clock := fun t _ => g t.

Definition generation_monotone (g : nat -> nat) : Prop :=
  forall t, g t <= g (S t).

Definition insert_at (g : nat -> nat) (t : nat) : Prop := g t < g (S t).

Theorem stable_generation_no_insert : forall g, generation_monotone g ->
  forall t1 t2, t1 <= t2 -> g t1 = g t2 ->
  forall t, t1 <= t -> t < t2 -> ~ insert_at g t.
Proof.
  intros g Hg t1 t2 Hle Heq t Ht1 Ht2 Hins.
  assert (monotone (generation_clock g)) as Hm.
  { intros t' e. unfold generation_clock. apply Hg. }
  exact (equal_endpoints_no_commit (generation_clock g) Hm 0 t1 t2 Hle Heq t Ht1 Ht2 Hins).
Qed.

Definition charge (limit used amount : nat) : option nat :=
  if used + amount <=? limit then Some (used + amount) else None.

Fixpoint charge_all (limit used : nat) (amounts : list nat) : option nat :=
  match amounts with
  | [] => Some used
  | a :: rest =>
      match charge limit used a with
      | Some next => charge_all limit next rest
      | None => None
      end
  end.

Definition total (amounts : list nat) : nat := fold_right Nat.add 0 amounts.

Theorem charge_all_bounded : forall amounts limit used final,
  used <= limit -> charge_all limit used amounts = Some final ->
  final <= limit /\ final = used + total amounts.
Proof.
  induction amounts as [| a rest IH]; intros limit used final Hused Hrun; simpl in Hrun.
  - inversion Hrun; subst. unfold total. simpl. split; lia.
  - unfold charge in Hrun.
    destruct (used + a <=? limit) eqn:Hstep.
    + apply Nat.leb_le in Hstep.
      destruct (IH limit (used + a) final Hstep Hrun) as [Hbound Hsum].
      split; [exact Hbound |]. unfold total in *. simpl. lia.
    + discriminate Hrun.
Qed.

Theorem charge_failure_keeps_usage : forall limit used amount,
  charge limit used amount = None -> limit < used + amount.
Proof.
  intros limit used amount Hfail. unfold charge in Hfail.
  destruct (used + amount <=? limit) eqn:Hstep; [discriminate |].
  apply Nat.leb_gt in Hstep. exact Hstep.
Qed.

Definition checked_add (bound used amount : nat) : option nat :=
  if used + amount <=? bound then Some (used + amount) else None.

Theorem overflow_implies_limit_failure : forall bound limit used amount,
  limit <= bound -> checked_add bound used amount = None -> charge limit used amount = None.
Proof.
  intros bound limit used amount Hle Hover. unfold checked_add in Hover. unfold charge.
  destruct (used + amount <=? bound) eqn:Hb; [discriminate |].
  apply Nat.leb_gt in Hb.
  destruct (used + amount <=? limit) eqn:Hl; [| reflexivity].
  apply Nat.leb_le in Hl. lia.
Qed.

Definition encode (payload : list nat) : nat * list nat := (length payload, payload).

Definition decode (raw : nat * list nat) : option (list nat) :=
  let (declared, payload) := raw in
  if declared =? length payload then Some payload else None.

Theorem decode_encode : forall payload, decode (encode payload) = Some payload.
Proof.
  intros payload. unfold encode, decode. simpl. rewrite Nat.eqb_refl. reflexivity.
Qed.

Theorem decode_sound : forall raw payload,
  decode raw = Some payload -> raw = encode payload.
Proof.
  intros [declared bytes] payload Hdec. unfold decode in Hdec. simpl in Hdec.
  destruct (declared =? length bytes) eqn:Heq; [| discriminate].
  inversion Hdec; subst. apply Nat.eqb_eq in Heq. subst. unfold encode. reflexivity.
Qed.

Inductive phase :=
  | New | Lock1 | Lock2 | Lock3 | Reading | Validating | Released | Done | Rejected.

Record cstate := {
  ph : phase;
  guards : list nat;
  txns : list nat
}.

Inductive action :=
  | Admit | LockA | LockB | LockC | OpenTxn (e : nat) | Validate | Release | Serialize | Reject.

Definition initial_capture : cstate := {| ph := New; guards := []; txns := [] |}.

Definition capture_step (s : cstate) (a : action) : option cstate :=
  match a, ph s with
  | Admit, New => Some {| ph := Lock1; guards := []; txns := [] |}
  | LockA, Lock1 => Some {| ph := Lock2; guards := [1]; txns := [] |}
  | LockB, Lock2 => Some {| ph := Lock3; guards := [2; 1]; txns := [] |}
  | LockC, Lock3 => Some {| ph := Reading; guards := [3; 2; 1]; txns := [] |}
  | OpenTxn e, Reading => Some {| ph := Reading; guards := [3; 2; 1]; txns := e :: txns s |}
  | Validate, Reading => Some {| ph := Validating; guards := [3; 2; 1]; txns := txns s |}
  | Release, Validating => Some {| ph := Released; guards := []; txns := [] |}
  | Serialize, Released => Some {| ph := Done; guards := []; txns := [] |}
  | Reject, Done => None
  | Reject, _ => Some {| ph := Rejected; guards := []; txns := [] |}
  | _, _ => None
  end.

Fixpoint capture_run (s : cstate) (actions : list action) : option cstate :=
  match actions with
  | [] => Some s
  | a :: rest =>
      match capture_step s a with
      | Some next => capture_run next rest
      | None => None
      end
  end.

Definition shape (s : cstate) : Prop :=
  match ph s with
  | New | Lock1 => guards s = [] /\ txns s = []
  | Lock2 => guards s = [1] /\ txns s = []
  | Lock3 => guards s = [2; 1] /\ txns s = []
  | Reading | Validating => guards s = [3; 2; 1]
  | Released | Done | Rejected => guards s = [] /\ txns s = []
  end.

Lemma initial_shape : shape initial_capture.
Proof. simpl. split; reflexivity. Qed.

Lemma step_shape : forall s a next,
  shape s -> capture_step s a = Some next -> shape next.
Proof.
  intros s a next Hshape Hstep.
  destruct a; destruct s as [p g t]; destruct p; simpl in *;
    try discriminate; inversion Hstep; subst; simpl;
    try (split; reflexivity); try reflexivity.
Qed.

Theorem run_shape : forall actions s final,
  shape s -> capture_run s actions = Some final -> shape final.
Proof.
  induction actions as [| a rest IH]; intros s final Hshape Hrun; simpl in Hrun.
  - inversion Hrun; subst. exact Hshape.
  - destruct (capture_step s a) as [next |] eqn:Hstep; [| discriminate].
    apply (IH next final); [| exact Hrun].
    apply (step_shape s a next Hshape Hstep).
Qed.

Definition guard_order_list (g : list nat) : Prop :=
  (In 2 g -> In 1 g) /\ (In 3 g -> In 1 g /\ In 2 g).

Definition guard_order (s : cstate) : Prop := guard_order_list (guards s).

Definition detached (s : cstate) : Prop :=
  (ph s = Released \/ ph s = Done \/ ph s = Rejected) -> guards s = [] /\ txns s = [].

Lemma order_nil : guard_order_list [].
Proof. split; intros H; inversion H. Qed.

Lemma order_one : guard_order_list [1].
Proof.
  split; intros H; simpl in H; destruct H as [H | H]; try lia; contradiction.
Qed.

Lemma order_two : guard_order_list [2; 1].
Proof.
  split; intros H.
  - simpl. right. left. reflexivity.
  - simpl in H. destruct H as [H | [H | H]]; try lia; contradiction.
Qed.

Lemma order_three : guard_order_list [3; 2; 1].
Proof.
  split; intros _.
  - simpl. right. right. left. reflexivity.
  - split; simpl.
    + right. right. left. reflexivity.
    + right. left. reflexivity.
Qed.

Lemma shape_guard_order : forall s, shape s -> guard_order s.
Proof.
  intros [p g t] Hshape. unfold guard_order. simpl in *.
  destruct p; unfold shape in Hshape; simpl in Hshape.
  - destruct Hshape as [Hg _]; subst g; exact order_nil.
  - destruct Hshape as [Hg _]; subst g; exact order_nil.
  - destruct Hshape as [Hg _]; subst g; exact order_one.
  - destruct Hshape as [Hg _]; subst g; exact order_two.
  - subst g; exact order_three.
  - subst g; exact order_three.
  - destruct Hshape as [Hg _]; subst g; exact order_nil.
  - destruct Hshape as [Hg _]; subst g; exact order_nil.
  - destruct Hshape as [Hg _]; subst g; exact order_nil.
Qed.

Lemma shape_detached : forall s, shape s -> detached s.
Proof.
  intros [p g t] Hshape. unfold detached. simpl in *. intros Hph.
  destruct p; unfold shape in Hshape; simpl in Hshape; try exact Hshape;
    destruct Hph as [H | [H | H]]; discriminate.
Qed.

Theorem capture_reachable_guard_order : forall actions final,
  capture_run initial_capture actions = Some final -> guard_order final.
Proof.
  intros actions final Hrun. apply shape_guard_order.
  apply (run_shape actions initial_capture final initial_shape Hrun).
Qed.

Theorem capture_reachable_detached : forall actions final,
  capture_run initial_capture actions = Some final -> detached final.
Proof.
  intros actions final Hrun. apply shape_detached.
  apply (run_shape actions initial_capture final initial_shape Hrun).
Qed.
