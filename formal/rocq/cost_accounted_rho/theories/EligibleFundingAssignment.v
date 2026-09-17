From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
Import ListNotations.

Fixpoint funding_sum (count : nat) (amount : nat -> nat) : nat :=
  match count with
  | 0 => 0
  | S rest => funding_sum rest amount + amount rest
  end.

Definition funding_flow := nat -> nat -> nat.

Definition source_draw (obligations : nat) (flow : funding_flow) (source : nat) :=
  funding_sum obligations (flow source).

Definition obligation_draw (sources : nat) (flow : funding_flow) (obligation : nat) :=
  funding_sum sources (fun source => flow source obligation).

Definition source_valid sources_obligations eligible capacity flow source : Prop :=
  source_draw sources_obligations flow source <= capacity source /\
  forall obligation, obligation < sources_obligations ->
    eligible source obligation = false -> flow source obligation = 0.

Definition assignment_valid sources obligations eligible capacity demand flow : Prop :=
  (forall source, source < sources -> source_valid obligations eligible capacity flow source) /\
  (forall obligation, obligation < obligations -> obligation_draw sources flow obligation = demand obligation).

Definition source_check obligations eligible capacity flow source : bool :=
  Nat.leb (source_draw obligations flow source) (capacity source) &&
  forallb (fun obligation => eligible source obligation || Nat.eqb (flow source obligation) 0)
          (seq 0 obligations).

Definition assignment_check sources obligations eligible capacity demand flow : bool :=
  forallb (source_check obligations eligible capacity flow) (seq 0 sources) &&
  forallb (fun obligation => Nat.eqb (obligation_draw sources flow obligation) (demand obligation))
          (seq 0 obligations).

Lemma funding_forallb_range : forall count predicate,
  forallb predicate (seq 0 count) = true <->
  forall index, index < count -> predicate index = true.
Proof.
  intros count predicate. rewrite forallb_forall. split.
  - intros all_values index bound. apply all_values. apply in_seq. lia.
  - intros all_values index included. apply all_values. apply in_seq in included. lia.
Qed.

Lemma funding_edge_check : forall eligible amount,
  eligible || Nat.eqb amount 0 = true <-> (eligible = false -> amount = 0).
Proof.
  destruct eligible; intros amount; simpl.
  - split; [intros _ impossible; discriminate|intros; reflexivity].
  - rewrite Nat.eqb_eq. tauto.
Qed.

Theorem source_check_exact : forall obligations eligible capacity flow source,
  source_check obligations eligible capacity flow source = true <->
  source_valid obligations eligible capacity flow source.
Proof.
  intros. unfold source_check, source_valid.
  rewrite andb_true_iff, Nat.leb_le, funding_forallb_range.
  split; intros [bound edges]; split; try assumption.
  - intros obligation inside. apply funding_edge_check. now apply edges.
  - intros obligation inside. apply funding_edge_check. now apply edges.
Qed.

Theorem assignment_check_exact : forall sources obligations eligible capacity demand flow,
  assignment_check sources obligations eligible capacity demand flow = true <->
  assignment_valid sources obligations eligible capacity demand flow.
Proof.
  intros. unfold assignment_check, assignment_valid.
  rewrite andb_true_iff, !funding_forallb_range.
  split; intros [rows columns]; split.
  - intros source inside. apply source_check_exact. now apply rows.
  - intros obligation inside. apply Nat.eqb_eq. now apply columns.
  - intros source inside. apply source_check_exact. now apply rows.
  - intros obligation inside. apply Nat.eqb_eq. now apply columns.
Qed.

Lemma funding_sum_ext : forall count lhs rhs,
  (forall index, index < count -> lhs index = rhs index) ->
  funding_sum count lhs = funding_sum count rhs.
Proof.
  induction count; intros; simpl; [reflexivity|].
  rewrite (IHcount lhs rhs) by (intros; apply H; lia).
  rewrite H by lia. reflexivity.
Qed.

Lemma funding_sum_add : forall count lhs rhs,
  funding_sum count (fun index => lhs index + rhs index) =
  funding_sum count lhs + funding_sum count rhs.
Proof. induction count; intros; simpl; [reflexivity|rewrite IHcount; lia]. Qed.

Lemma funding_sum_zero : forall count, funding_sum count (fun _ => 0) = 0.
Proof. induction count; simpl; lia. Qed.

Theorem funding_assignment_rows_equal_columns : forall sources obligations flow,
  funding_sum sources (source_draw obligations flow) =
  funding_sum obligations (obligation_draw sources flow).
Proof.
  induction sources; intros obligations flow.
  - unfold obligation_draw. simpl. rewrite funding_sum_zero. reflexivity.
  - change (funding_sum sources (source_draw obligations flow) + source_draw obligations flow sources =
      funding_sum obligations (fun obligation =>
        obligation_draw sources flow obligation + flow sources obligation)).
    rewrite funding_sum_add, IHsources. reflexivity.
Qed.

Theorem accepted_assignment_conserves_obligation : forall sources obligations eligible capacity demand flow,
  assignment_valid sources obligations eligible capacity demand flow ->
  funding_sum sources (source_draw obligations flow) = funding_sum obligations demand.
Proof.
  intros sources obligations eligible capacity demand flow [_ columns].
  rewrite funding_assignment_rows_equal_columns.
  apply funding_sum_ext. exact columns.
Qed.

Theorem accepted_assignment_respects_reserved_sources :
  forall sources obligations eligible capacity demand flow reserved,
  assignment_valid sources obligations eligible capacity demand flow ->
  (forall source, source < sources -> source_draw obligations flow source <= reserved source) ->
  assignment_valid sources obligations eligible reserved demand flow.
Proof.
  intros sources obligations eligible capacity demand flow reserved [rows columns] covered.
  split; [|exact columns]. intros source inside.
  destruct (rows source inside) as [_ edges]. split; [now apply covered|exact edges].
Qed.

Fixpoint branch_reservation (branches : list nat) obligations (plans : nat -> funding_flow) source : nat :=
  match branches with
  | [] => 0
  | branch :: rest => Nat.max (source_draw obligations (plans branch) source)
                              (branch_reservation rest obligations plans source)
  end.

Theorem branch_reservation_covers_every_plan : forall branches obligations plans source branch,
  In branch branches -> source_draw obligations (plans branch) source <=
                       branch_reservation branches obligations plans source.
Proof.
  induction branches as [|head rest IH]; intros obligations plans source branch included; simpl in *.
  - contradiction.
  - destruct included as [same|inside].
    + subst branch. apply Nat.le_max_l.
    + eapply Nat.le_trans; [apply IH; exact inside|apply Nat.le_max_r].
Qed.

Theorem branch_reservation_within_capacity : forall branches obligations plans capacity source,
  (forall branch, In branch branches -> source_draw obligations (plans branch) source <= capacity source) ->
  branch_reservation branches obligations plans source <= capacity source.
Proof.
  induction branches as [|head rest IH]; intros obligations plans capacity source bounded; simpl; [lia|].
  apply Nat.max_lub.
  - apply bounded. now left.
  - apply IH. intros branch included. apply bounded. now right.
Qed.

Theorem branch_envelope_preserves_feasibility : forall branches sources obligations eligible capacity demands plans branch,
  (forall selected, In selected branches ->
    assignment_valid sources obligations (eligible selected) capacity (demands selected) (plans selected)) ->
  In branch branches ->
  assignment_valid sources obligations (eligible branch)
    (branch_reservation branches obligations plans) (demands branch) (plans branch).
Proof.
  intros branches sources obligations eligible capacity demands plans branch valid included.
  eapply accepted_assignment_respects_reserved_sources; [apply valid; exact included|].
  intros source _. now apply branch_reservation_covers_every_plan.
Qed.

Definition selected_branch_refund branches obligations plans branch source :=
  branch_reservation branches obligations plans source - source_draw obligations (plans branch) source.

Theorem branch_settlement_conserves_each_source : forall branches obligations plans branch source,
  In branch branches ->
  source_draw obligations (plans branch) source +
    selected_branch_refund branches obligations plans branch source =
  branch_reservation branches obligations plans source.
Proof.
  intros branches obligations plans branch source included.
  pose proof (branch_reservation_covers_every_plan branches obligations plans source branch included).
  unfold selected_branch_refund. lia.
Qed.

Theorem branch_settlement_conserves_total : forall branches sources obligations plans branch,
  In branch branches ->
  funding_sum sources (source_draw obligations (plans branch)) +
    funding_sum sources (selected_branch_refund branches obligations plans branch) =
  funding_sum sources (branch_reservation branches obligations plans).
Proof.
  intros branches sources obligations plans branch included.
  rewrite <- funding_sum_add. apply funding_sum_ext.
  intros source _. now apply branch_settlement_conserves_each_source.
Qed.

Theorem branch_reservation_is_least_for_fixed_plans : forall branches obligations plans proposed,
  (forall branch source, In branch branches ->
    source_draw obligations (plans branch) source <= proposed source) ->
  forall source, branch_reservation branches obligations plans source <= proposed source.
Proof.
  intros branches obligations plans proposed covers source.
  apply branch_reservation_within_capacity. intros branch included. now apply covers.
Qed.

Definition restricted_eligibility (source obligation : nat) :=
  Nat.eqb source 0 || Nat.eqb obligation 0.

Definition feasible_two_source (source obligation : nat) :=
  if Nat.eqb source 0 then (if Nat.eqb obligation 1 then 1 else 0)
  else if Nat.eqb obligation 0 then 1 else 0.

Definition greedy_two_source (source obligation : nat) :=
  if Nat.eqb source 0 then 1 else 0.

Example greedy_order_is_not_infeasibility :
  assignment_check 2 2 restricted_eligibility (fun _ => 1) (fun _ => 1) feasible_two_source = true /\
  assignment_check 2 2 restricted_eligibility (fun _ => 1) (fun _ => 1) greedy_two_source = false.
Proof. vm_compute. split; reflexivity. Qed.

Definition branch_plan branch source obligation :=
  if Nat.eqb obligation 0 && Nat.eqb source branch then 1 else 0.

Example exclusive_branches_need_distinct_backing :
  funding_sum 2 (branch_reservation [0; 1] 1 branch_plan) = 2 /\
  funding_sum 1 (obligation_draw 2 (branch_plan 0)) = 1 /\
  funding_sum 1 (obligation_draw 2 (branch_plan 1)) = 1.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem exclusive_branches_cannot_share_one_restricted_hold : forall reserved,
  1 <= reserved 0 -> 1 <= reserved 1 ->
  ~ funding_sum 2 reserved <= 1.
Proof. intros. simpl. lia. Qed.

Print Assumptions assignment_check_exact.
Print Assumptions accepted_assignment_conserves_obligation.
Print Assumptions branch_envelope_preserves_feasibility.
Print Assumptions exclusive_branches_cannot_share_one_restricted_hold.
Print Assumptions branch_settlement_conserves_each_source.
Print Assumptions branch_settlement_conserves_total.
Print Assumptions branch_reservation_is_least_for_fixed_plans.
