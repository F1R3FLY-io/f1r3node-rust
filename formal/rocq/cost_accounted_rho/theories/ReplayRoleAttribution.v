(* D-C5b (epic 8946, Phase D; decision record DR-102): the replay budget of
   an offered deploy charges the same usage on its producer and on every
   validator.

   The producer and every validator run [certify_offered_draft] on the same
   inputs, so the certified replay and the copy of the block context charge
   the same reservations on every role (premise S1). Each role then does its
   own acceptance work. Before DR-102 that work was charged to the replay
   budget: the producer charged its publication comparison and the context
   copy, and a validator charged the system continuation, the mergeable
   result vector and the mergeable pre-state read, but not the copy. The
   replay budgets of the two roles then differed, and a block near a cap was
   published by its producer and rejected by every validator.

   DR-102 charges the replay budget with the certified replay and the copy
   alone, on every role. A role's acceptance work goes to an acceptance
   budget of that role. The producer stores the certified mergeable map, so
   its execution budget charges exactly the mergeable work that a validator
   charges to its acceptance budget (premise S2, true by construction).

   A budget is sticky: once a reservation takes a dimension over its limit,
   that reservation and every later one fail. Usage only grows, so a budget
   accepts a sequence of reservations exactly when every per-dimension total
   fits its limit (accepts_iff_totals_fit).

   Results:
   - accepts_iff_totals_fit and verdict_depends_only_on_totals.
   - replay_usage_role_independent and replay_verdict_role_independent: the
     two roles charge the same reservations to the replay budget and reach
     the same replay verdict for every limit.
   - dr102_meters_every_performed_site: every site that a role performs is
     charged to some budget.
   - validator_acceptance_is_producer_execution_work and
     producer_acceptance_is_the_publication_comparison.
   - honest_block_never_exhausts_validator_acceptance: if the producer's
     execution budget accepted its play and its mergeable work, and the
     acceptance limits are at least the execution limits, a validator's
     acceptance budget accepts.
   - publish_implies_accept: under DR-102, a block that its producer
     publishes is accepted by every validator whose preflight accepts it.
   - Negative controls: before_dr102_charges_roles_differently,
     before_dr102_splits_the_verdict (the SSB witness: the producer charges
     101 and accepts, a validator charges 104 and rejects, limit 102),
     before_dr102_leaves_the_validator_context_copy_unmetered,
     smaller_acceptance_limit_rejects_an_honest_block and
     validator_acceptance_on_replay_splits_the_verdict.

   No axiom, parameter or admission is used. Section variables are
   universally quantified when the sections close.

   Rust correspondence: casper/src/rust/util/rholang/runtime_manager.rs
   (certify_offered_draft owns the replay budget and charges the context
   copy; the validator path charges its acceptance budget), casper/src/rust/
   rholang/runtime.rs (the producer's acceptance budget and the certified
   mergeable map), casper/src/rust/util/rholang/costacc/offered_acceptance.rs
   (OfferedAcceptanceBudget and its limits). *)

From Stdlib Require Import Lists.List Arith.PeanoNat Bool Lia.
Import ListNotations.

Inductive dim := VB | VOps | SSB.

Definition dim_eq_dec (a b : dim) : {a = b} + {a <> b}.
Proof. decide equality. Defined.

Definition reservation := (dim * nat)%type.

Definition limits := dim -> nat.

Fixpoint total (d : dim) (rs : list reservation) : nat :=
  match rs with
  | [] => 0
  | (d', n) :: rest => (if dim_eq_dec d d' then n else 0) + total d rest
  end.

Lemma total_app :
  forall d xs ys, total d (xs ++ ys) = total d xs + total d ys.
Proof.
  intros d xs ys. induction xs as [| [d' n] rest IH]; simpl.
  - reflexivity.
  - rewrite IH. lia.
Qed.

Definition bump (used : dim -> nat) (d : dim) (n : nat) : dim -> nat :=
  fun d' => if dim_eq_dec d' d then used d' + n else used d'.

(* The sticky budget: a reservation that takes its dimension over the limit
   fails, and so does every later reservation. *)
Fixpoint accepts_from (lim : limits) (used : dim -> nat) (rs : list reservation)
  : bool :=
  match rs with
  | [] => true
  | (d, n) :: rest =>
      if used d + n <=? lim d then accepts_from lim (bump used d n) rest else false
  end.

Definition accepts (lim : limits) (rs : list reservation) : bool :=
  accepts_from lim (fun _ => 0) rs.

Lemma accepts_from_iff :
  forall rs lim used,
    (forall d, used d <= lim d) ->
    (accepts_from lim used rs = true <-> forall d, used d + total d rs <= lim d).
Proof.
  induction rs as [| [d n] rest IH]; intros lim used Hused; simpl.
  - split.
    + intros _ d'. rewrite Nat.add_0_r. apply Hused.
    + intros _. reflexivity.
  - destruct (used d + n <=? lim d) eqn:Hfit.
    + apply Nat.leb_le in Hfit.
      assert (Hbump : forall d', bump used d n d' <= lim d').
      { intros d'. unfold bump. destruct (dim_eq_dec d' d) as [-> | _]; auto. }
      rewrite (IH lim (bump used d n) Hbump).
      split; intros H d'; specialize (H d'); unfold bump in *;
        destruct (dim_eq_dec d' d) as [-> | Hne];
        destruct (dim_eq_dec d d) as [_ | Hdd]; try contradiction;
        try (destruct (dim_eq_dec d' d) as [Heq | _]; [contradiction | ]); lia.
    + apply Nat.leb_gt in Hfit. split; [discriminate | intros H].
      specialize (H d). destruct (dim_eq_dec d d) as [_ | Hdd]; [lia | contradiction].
Qed.

Theorem accepts_iff_totals_fit :
  forall lim rs, accepts lim rs = true <-> forall d, total d rs <= lim d.
Proof.
  intros lim rs. unfold accepts.
  rewrite (accepts_from_iff rs lim (fun _ => 0)) by (intros; lia).
  split; intros H d; specialize (H d); lia.
Qed.

Theorem verdict_depends_only_on_totals :
  forall lim xs ys,
    (forall d, total d xs = total d ys) -> accepts lim xs = accepts lim ys.
Proof.
  intros lim xs ys Htotals.
  destruct (accepts lim xs) eqn:Hx; destruct (accepts lim ys) eqn:Hy; auto.
  - pose proof (proj1 (accepts_iff_totals_fit lim xs) Hx) as Hxs.
    assert (Hfit : forall d, total d ys <= lim d) by (intros d; rewrite <- Htotals; auto).
    pose proof (proj2 (accepts_iff_totals_fit lim ys) Hfit) as Hys. congruence.
  - pose proof (proj1 (accepts_iff_totals_fit lim ys) Hy) as Hys.
    assert (Hfit : forall d, total d xs <= lim d) by (intros d; rewrite Htotals; auto).
    pose proof (proj2 (accepts_iff_totals_fit lim xs) Hfit) as Hxs. congruence.
Qed.

Inductive role := Producer | Validator.
Inductive meter := Execution | Replay | Acceptance | Unmetered.
Inductive site :=
| ContextCopy
| CertifiedReplay
| PublicationComparison
| SystemContinuation
| MergeableResult
| MergeablePreState.

Definition sites : list site :=
  [ContextCopy; CertifiedReplay; PublicationComparison; SystemContinuation;
   MergeableResult; MergeablePreState].

(* Only the producer compares its candidate with the certified replay. *)
Definition performs (r : role) (s : site) : bool :=
  match r, s with
  | Validator, PublicationComparison => false
  | _, _ => true
  end.

Definition meter_eqb (a b : meter) : bool :=
  match a, b with
  | Execution, Execution | Replay, Replay | Acceptance, Acceptance
  | Unmetered, Unmetered => true
  | _, _ => false
  end.

(* The reservations that a role charges to a budget. The charges of a site
   are a function of the block alone: S1 for the certified replay and the
   copy, S2 for the mergeable work. *)
Definition usage (charge : site -> list reservation) (route : role -> site -> meter)
  (r : role) (m : meter) : list reservation :=
  flat_map (fun s => if performs r s && meter_eqb (route r s) m then charge s else [])
    sites.

Definition dr102 (r : role) (s : site) : meter :=
  match r, s with
  | _, ContextCopy | _, CertifiedReplay => Replay
  | Producer, PublicationComparison => Acceptance
  | Producer, _ => Execution
  | Validator, _ => Acceptance
  end.

Definition before_dr102 (r : role) (s : site) : meter :=
  match r, s with
  | Producer, ContextCopy | Producer, CertifiedReplay
  | Producer, PublicationComparison => Replay
  | Producer, _ => Execution
  | Validator, ContextCopy => Unmetered
  | Validator, _ => Replay
  end.

(* A routing that keeps DR-102 for the producer but charges a validator's
   acceptance work to its replay budget. *)
Definition validator_acceptance_on_replay (r : role) (s : site) : meter :=
  match r, s with
  | Validator, SystemContinuation | Validator, MergeableResult
  | Validator, MergeablePreState => Replay
  | _, _ => dr102 r s
  end.

Section Routing.
  Variable charge : site -> list reservation.

  Theorem replay_usage_role_independent :
    usage charge dr102 Producer Replay = usage charge dr102 Validator Replay.
  Proof. reflexivity. Qed.

  Theorem replay_verdict_role_independent :
    forall lim,
      accepts lim (usage charge dr102 Producer Replay)
      = accepts lim (usage charge dr102 Validator Replay).
  Proof. intros lim. rewrite replay_usage_role_independent. reflexivity. Qed.

  Theorem validator_acceptance_is_producer_execution_work :
    usage charge dr102 Validator Acceptance = usage charge dr102 Producer Execution.
  Proof. reflexivity. Qed.

  Theorem producer_acceptance_is_the_publication_comparison :
    usage charge dr102 Producer Acceptance = charge PublicationComparison.
  Proof. unfold usage, sites. simpl. rewrite !app_nil_r. reflexivity. Qed.

  Theorem honest_block_never_exhausts_validator_acceptance :
    forall (play : list reservation) (lim_e lim_a : limits),
      (forall d, lim_e d <= lim_a d) ->
      accepts lim_e (play ++ usage charge dr102 Producer Execution) = true ->
      accepts lim_a (usage charge dr102 Validator Acceptance) = true.
  Proof.
    intros play lim_e lim_a Hdominates Hexecution.
    apply (proj2 (accepts_iff_totals_fit _ _)). intros d.
    pose proof (proj1 (accepts_iff_totals_fit _ _) Hexecution d) as Hd.
    rewrite total_app in Hd.
    rewrite validator_acceptance_is_producer_execution_work.
    specialize (Hdominates d). lia.
  Qed.
End Routing.

Theorem dr102_meters_every_performed_site :
  forall r s, performs r s = true -> dr102 r s <> Unmetered.
Proof. intros [] [] _; discriminate. Qed.

(* A producer publishes when its replay, execution and acceptance budgets all
   accept. A validator accepts when its preflight, replay and acceptance
   budgets accept. *)
Definition publishes (route : role -> site -> meter) (charge : site -> list reservation)
  (play : list reservation) (lim_r lim_e lim_a : limits) : bool :=
  accepts lim_r (usage charge route Producer Replay)
  && accepts lim_e (play ++ usage charge route Producer Execution)
  && accepts lim_a (usage charge route Producer Acceptance).

Definition validator_accepts (route : role -> site -> meter)
  (charge : site -> list reservation) (preflight : list reservation)
  (lim_pre lim_r lim_a : limits) : bool :=
  accepts lim_pre preflight
  && accepts lim_r (usage charge route Validator Replay)
  && accepts lim_a (usage charge route Validator Acceptance).

Theorem publish_implies_accept :
  forall charge play preflight (lim_pre lim_r lim_e lim_a : limits),
    (forall d, lim_e d <= lim_a d) ->
    accepts lim_pre preflight = true ->
    publishes dr102 charge play lim_r lim_e lim_a = true ->
    validator_accepts dr102 charge preflight lim_pre lim_r lim_a = true.
Proof.
  intros charge play preflight lim_pre lim_r lim_e lim_a Hdominates Hpreflight Hpublish.
  unfold publishes in Hpublish.
  apply andb_true_iff in Hpublish as [Hpublish _].
  apply andb_true_iff in Hpublish as [Hreplay Hexecution].
  unfold validator_accepts.
  rewrite Hpreflight, <- replay_usage_role_independent, Hreplay.
  rewrite (honest_block_never_exhausts_validator_acceptance charge play lim_e lim_a
             Hdominates Hexecution).
  reflexivity.
Qed.

(* Negative controls. *)

(* Concrete charges: the certified replay keeps 100 search-state bytes, the
   copy 1, the mergeable result 1 and the pre-state read 3; the comparison
   charges 5 verification bytes and operations. *)
Definition witness_charge (s : site) : list reservation :=
  match s with
  | ContextCopy => [(SSB, 1)]
  | CertifiedReplay => [(SSB, 100)]
  | PublicationComparison => [(VB, 5); (VOps, 5)]
  | SystemContinuation => []
  | MergeableResult => [(SSB, 1)]
  | MergeablePreState => [(SSB, 3)]
  end.

Definition witness_replay_limits (d : dim) : nat :=
  match d with
  | SSB => 102
  | VB | VOps => 1000
  end.

Definition witness_large_limits (_ : dim) : nat := 1000.

Theorem before_dr102_charges_roles_differently :
  total SSB (usage witness_charge before_dr102 Producer Replay) = 101
  /\ total SSB (usage witness_charge before_dr102 Validator Replay) = 104.
Proof. split; reflexivity. Qed.

Theorem before_dr102_splits_the_verdict :
  publishes before_dr102 witness_charge [] witness_replay_limits witness_large_limits
    witness_large_limits = true
  /\ validator_accepts before_dr102 witness_charge [] witness_large_limits
       witness_replay_limits witness_large_limits = false.
Proof. split; reflexivity. Qed.

Theorem before_dr102_leaves_the_validator_context_copy_unmetered :
  performs Validator ContextCopy = true /\ before_dr102 Validator ContextCopy = Unmetered.
Proof. split; reflexivity. Qed.

Theorem dr102_accepts_the_witness :
  publishes dr102 witness_charge [] witness_replay_limits witness_large_limits
    witness_large_limits = true
  /\ validator_accepts dr102 witness_charge [] witness_large_limits
       witness_replay_limits witness_large_limits = true.
Proof. split; reflexivity. Qed.

Definition witness_execution_limits (d : dim) : nat :=
  match d with
  | SSB => 4
  | VB | VOps => 1000
  end.

Definition witness_small_acceptance_limits (d : dim) : nat :=
  match d with
  | SSB => 3
  | VB | VOps => 1000
  end.

Theorem smaller_acceptance_limit_rejects_an_honest_block :
  witness_small_acceptance_limits SSB < witness_execution_limits SSB
  /\ publishes dr102 witness_charge [] witness_replay_limits witness_execution_limits
       witness_large_limits = true
  /\ validator_accepts dr102 witness_charge [] witness_large_limits
       witness_replay_limits witness_small_acceptance_limits = false.
Proof. split; [cbv; lia | split; reflexivity]. Qed.

Theorem validator_acceptance_on_replay_splits_the_verdict :
  publishes validator_acceptance_on_replay witness_charge [] witness_replay_limits
    witness_large_limits witness_large_limits = true
  /\ validator_accepts validator_acceptance_on_replay witness_charge []
       witness_large_limits witness_replay_limits witness_large_limits = false.
Proof. split; reflexivity. Qed.
