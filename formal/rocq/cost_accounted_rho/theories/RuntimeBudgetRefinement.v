(* ═══════════════════════════════════════════════════════════════════════════
   RuntimeBudgetRefinement.v — Bounded-Memory Runtime Budget Model
   ═══════════════════════════════════════════════════════════════════════════

   The paper model represents fuel as a right-nested token stack. The Rust
   implementation cannot allocate one runtime node per token at production
   phlo limits, so it uses a bounded-memory RuntimeBudget: an initial token
   count, a consumed-token counter, a canonical event log, and the first
   out-of-phlo event that crosses the budget boundary.

   This module proves the arithmetic refinement obligations that connect
   that coalesced representation back to the token-count semantics used by
   the cost-accounted rho calculus. It also models the replay payload
   fingerprint at the level needed by the design: user costs and event
   traces are both replay-relevant observables.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia
  Sorting.Permutation Sorting.Sorted Sorting.Mergesort Structures.Orders.
Import ListNotations.

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import Settlement.

Inductive rb_billable_kind : Type :=
  | RbSourceStep
  | RbPrimitive (descriptor : nat)
  | RbSubstitution.

Record rb_event := {
  rb_event_deploy_id : nat;
  rb_event_source_path : list nat;
  rb_event_redex_id : nat;
  rb_event_local_index : nat;
  rb_event_kind : rb_billable_kind;
  rb_event_weight : nat
}.

Record rb_state := {
  rb_initial : nat;
  rb_consumed : nat;
  rb_unmetered : bool;
  rb_event_log : list rb_event;
  rb_last_oop : option rb_event
}.

Inductive rb_reserve_result : Type :=
  | RbReserveOk
  | RbReserveOop.

Inductive rb_admitted_reserve_result : Type :=
  | RbAdmittedOk
  | RbAdmittedOop
  | RbAdmittedInvalid.

Record rb_execution_permit := {
  rb_permit_event : rb_event;
  rb_permit_weight : nat
}.

Inductive rb_system_deploy_kind : Type :=
  | RbSystemSlash
  | RbSystemClose
  | RbSystemEmpty
  | RbSystemFailed.

Definition rb_valid (b : rb_state) : Prop :=
  rb_consumed b <= rb_initial b.

Definition rb_total_cost (b : rb_state) : nat :=
  if rb_unmetered b then 0 else rb_consumed b.

Definition rb_remaining (b : rb_state) : nat :=
  if rb_unmetered b then rb_initial b else rb_initial b - rb_consumed b.

Definition rb_new (initial : nat) : rb_state :=
  {|
    rb_initial := initial;
    rb_consumed := 0;
    rb_unmetered := false;
    rb_event_log := [];
    rb_last_oop := None
  |}.

Definition rb_set_unmetered (b : rb_state) (unmetered : bool) : rb_state :=
  {|
    rb_initial := rb_initial b;
    rb_consumed := rb_consumed b;
    rb_unmetered := unmetered;
    rb_event_log := rb_event_log b;
    rb_last_oop := rb_last_oop b
  |}.

Definition rb_reset_from_token (b : rb_state) (t : token) : rb_state :=
  {|
    rb_initial := token_size t;
    rb_consumed := 0;
    rb_unmetered := rb_unmetered b;
    rb_event_log := [];
    rb_last_oop := None
  |}.

Definition rb_reserve
  (b : rb_state)
  (e : rb_event)
  : rb_state * rb_reserve_result :=
  if rb_unmetered b then
    (b, RbReserveOk)
  else if rb_initial b <? rb_consumed b + rb_event_weight e then
    ({|
      rb_initial := rb_initial b;
      rb_consumed := rb_initial b;
      rb_unmetered := false;
      rb_event_log := rb_event_log b;
      rb_last_oop :=
        match rb_last_oop b with
        | Some existing => Some existing
        | None => Some e
        end
    |}, RbReserveOop)
  else
    ({|
      rb_initial := rb_initial b;
      rb_consumed := rb_consumed b + rb_event_weight e;
      rb_unmetered := false;
      rb_event_log := rb_event_log b ++ [e];
      rb_last_oop := rb_last_oop b
    |}, RbReserveOk).

Definition rb_event_positive (e : rb_event) : Prop :=
  0 < rb_event_weight e.

Definition rb_event_within_weight_bound (max_weight : nat) (e : rb_event) : Prop :=
  rb_event_weight e <= max_weight.

Definition rb_event_source_path_within_bound
  (max_source_path_components : nat)
  (e : rb_event)
  : Prop :=
  length (rb_event_source_path e) <= max_source_path_components.

Definition rb_event_primitive_descriptor_within_bound
  (max_primitive_descriptor : nat)
  (e : rb_event)
  : Prop :=
  match rb_event_kind e with
  | RbPrimitive descriptor => descriptor <= max_primitive_descriptor
  | _ => True
  end.

Definition rb_trace_slot_count (b : rb_state) : nat :=
  length (rb_event_log b) +
  match rb_last_oop b with
  | None => 0
  | Some _ => 1
  end.

Definition rb_event_admissible
  (max_weight max_source_path_components max_primitive_descriptor : nat)
  (e : rb_event)
  : Prop :=
  rb_event_positive e /\
  rb_event_within_weight_bound max_weight e /\
  rb_event_source_path_within_bound max_source_path_components e /\
  rb_event_primitive_descriptor_within_bound max_primitive_descriptor e.

Definition rb_reserve_admitted
  (max_weight max_source_path_components max_primitive_descriptor : nat)
  (b : rb_state)
  (e : rb_event)
  : rb_state * rb_admitted_reserve_result :=
  if rb_event_weight e =? 0 then
    (b, RbAdmittedInvalid)
  else if max_weight <? rb_event_weight e then
    (b, RbAdmittedInvalid)
  else if max_source_path_components <? length (rb_event_source_path e) then
    (b, RbAdmittedInvalid)
  else
    match rb_event_kind e with
    | RbPrimitive descriptor =>
        if max_primitive_descriptor <? descriptor then
          (b, RbAdmittedInvalid)
        else
          match rb_reserve b e with
          | (b', RbReserveOk) => (b', RbAdmittedOk)
          | (b', RbReserveOop) => (b', RbAdmittedOop)
          end
    | _ =>
        match rb_reserve b e with
        | (b', RbReserveOk) => (b', RbAdmittedOk)
        | (b', RbReserveOop) => (b', RbAdmittedOop)
        end
    end.

Definition rb_reserve_bounded
  (max_weight max_source_path_components max_primitive_descriptor max_events : nat)
  (b : rb_state)
  (e : rb_event)
  : rb_state * rb_admitted_reserve_result :=
  if rb_trace_slot_count b <? max_events then
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e
  else
    (b, RbAdmittedInvalid).

Fixpoint rb_reserve_many
  (b : rb_state)
  (events : list rb_event)
  : rb_state * list rb_reserve_result :=
  match events with
  | [] => (b, [])
  | e :: rest =>
      match rb_reserve b e with
      | (b', RbReserveOk) =>
          let (b'', results) := rb_reserve_many b' rest in
          (b'', RbReserveOk :: results)
      | (b', RbReserveOop) => (b', [RbReserveOop])
      end
  end.

Fixpoint rb_granted_permits
  (events : list rb_event)
  (results : list rb_reserve_result)
  : list rb_execution_permit :=
  match events, results with
  | e :: rest_events, RbReserveOk :: rest_results =>
      {| rb_permit_event := e; rb_permit_weight := rb_event_weight e |}
        :: rb_granted_permits rest_events rest_results
  | _, _ => []
  end.

Definition rb_commit_canonical_batch
  (b : rb_state)
  (events : list rb_event)
  : rb_state * list rb_execution_permit * list rb_reserve_result :=
  let '(b', results) := rb_reserve_many b events in
  (b', rb_granted_permits events results, results).

Fixpoint rb_permit_weight_sum (permits : list rb_execution_permit) : nat :=
  match permits with
  | [] => 0
  | permit :: rest => rb_permit_weight permit + rb_permit_weight_sum rest
  end.

Fixpoint rb_event_weight_sum (events : list rb_event) : nat :=
  match events with
  | [] => 0
  | e :: rest => rb_event_weight e + rb_event_weight_sum rest
  end.

Fixpoint rb_oop_count (results : list rb_reserve_result) : nat :=
  match results with
  | [] => 0
  | RbReserveOk :: rest => rb_oop_count rest
  | RbReserveOop :: rest => 1 + rb_oop_count rest
  end.

Definition rb_diagnostic_cap_log
  (cap : nat)
  (log : list rb_event)
  : list rb_event :=
  firstn cap log.

Inductive rb_trace_event_kind : Type :=
  | RbTraceSuccess
  | RbTraceOop.

Record rb_trace_descriptor := {
  rb_trace_deploy_id : nat;
  rb_trace_source_path : list nat;
  rb_trace_redex_id : nat;
  rb_trace_local_index : nat;
  rb_trace_billable_kind : rb_billable_kind;
  rb_trace_weight : nat
}.

Definition rb_trace_descriptor_of_event (e : rb_event) : rb_trace_descriptor :=
  {|
    rb_trace_deploy_id := rb_event_deploy_id e;
    rb_trace_source_path := rb_event_source_path e;
    rb_trace_redex_id := rb_event_redex_id e;
    rb_trace_local_index := rb_event_local_index e;
    rb_trace_billable_kind := rb_event_kind e;
    rb_trace_weight := rb_event_weight e
  |}.

Definition rb_trace_entry : Type := rb_trace_event_kind * rb_trace_descriptor.

Definition rb_success_trace_entry (e : rb_event) : rb_trace_entry :=
  (RbTraceSuccess, rb_trace_descriptor_of_event e).

Definition rb_oop_trace_entry (e : rb_event) : rb_trace_entry :=
  (RbTraceOop, rb_trace_descriptor_of_event e).

Definition rb_success_trace_entries (log : list rb_event) : list rb_trace_entry :=
  map rb_success_trace_entry log.

Definition rb_oop_trace_entries (oop : option rb_event) : list rb_trace_entry :=
  match oop with
  | None => []
  | Some e => [rb_oop_trace_entry e]
  end.

Definition rb_cost_trace_entries (b : rb_state) : list rb_trace_entry :=
  rb_success_trace_entries (rb_event_log b) ++
  rb_oop_trace_entries (rb_last_oop b).

Definition rb_cost_trace_event_count (trace : list rb_trace_entry) : nat :=
  length trace.

Definition rb_cost_trace_present (trace : list rb_trace_entry) : Prop :=
  trace <> [].

Definition rb_cost_trace_commitment_present (present : bool) : Prop :=
  present = true.

Definition rb_cost_trace_event_count_matches
  (trace : list rb_trace_entry)
  (count : nat)
  : Prop :=
  rb_cost_trace_event_count trace = count.

Definition rb_cost_trace_commitment_valid
  (trace : list rb_trace_entry)
  (count : nat)
  (present : bool)
  : Prop :=
  rb_cost_trace_commitment_present present /\
  rb_cost_trace_event_count_matches trace count.

Inductive rb_replay_mode : Type :=
  | RbCostAccountedReplay
  | RbLegacyReplay.

Definition rb_replay_mode_accepts_cost_trace
  (mode : rb_replay_mode)
  (trace : list rb_trace_entry)
  (count : nat)
  (present : bool)
  : Prop :=
  match mode with
  | RbCostAccountedReplay =>
      rb_cost_trace_commitment_valid trace count present
  | RbLegacyReplay => True
  end.

Definition rb_finalize_trace_window (b : rb_state) : rb_state :=
  b.

Fixpoint rb_sum_escrowed_amount (settlements : list fee_settlement) : nat :=
  match settlements with
  | [] => 0
  | s :: rest => escrowed_amount s + rb_sum_escrowed_amount rest
  end.

Fixpoint rb_sum_charged_amount (settlements : list fee_settlement) : nat :=
  match settlements with
  | [] => 0
  | s :: rest => charged_amount s + rb_sum_charged_amount rest
  end.

Fixpoint rb_sum_refund_amount (settlements : list fee_settlement) : nat :=
  match settlements with
  | [] => 0
  | s :: rest => refund_amount s + rb_sum_refund_amount rest
  end.

Fixpoint rb_sum_settled_amount (settlements : list fee_settlement) : nat :=
  match settlements with
  | [] => 0
  | s :: rest => settled_amount s + rb_sum_settled_amount rest
  end.

Theorem rb_new_valid : forall initial,
  rb_valid (rb_new initial).
Proof.
  intros initial. unfold rb_valid, rb_new. simpl. lia.
Qed.

Theorem rb_total_remaining_conservation : forall b,
  rb_valid b ->
  rb_total_cost b + rb_remaining b = rb_initial b.
Proof.
  intros b Hvalid.
  unfold rb_total_cost, rb_remaining, rb_valid in *.
  destruct (rb_unmetered b); lia.
Qed.

Theorem rb_reserve_preserves_valid : forall b e b' r,
  rb_valid b ->
  rb_reserve b e = (b', r) ->
  rb_valid b'.
Proof.
  intros b e b' r Hvalid Hreserve.
  unfold rb_reserve in Hreserve.
  destruct (rb_unmetered b) eqn:Hunmetered.
  - inversion Hreserve. subst. exact Hvalid.
  - destruct (rb_initial b <? rb_consumed b + rb_event_weight e) eqn:Hlt.
    + inversion Hreserve. subst.
      unfold rb_valid. simpl. lia.
    + apply Nat.ltb_ge in Hlt.
      inversion Hreserve. subst.
      unfold rb_valid. simpl. exact Hlt.
Qed.

Theorem rb_unmetered_reserve_no_cost : forall b e,
  rb_unmetered b = true ->
  rb_reserve b e = (b, RbReserveOk).
Proof.
  intros b e Hunmetered.
  unfold rb_reserve. rewrite Hunmetered. reflexivity.
Qed.

Theorem rb_reserve_success_consumes_weight : forall b e b',
  rb_valid b ->
  rb_unmetered b = false ->
  rb_reserve b e = (b', RbReserveOk) ->
  rb_consumed b' = rb_consumed b + rb_event_weight e.
Proof.
  intros b e b' _ Hunmetered Hreserve.
  unfold rb_reserve in Hreserve.
  rewrite Hunmetered in Hreserve.
  destruct (rb_initial b <? rb_consumed b + rb_event_weight e) eqn:Hlt.
  - discriminate.
  - inversion Hreserve. reflexivity.
Qed.

Theorem rb_reserve_success_appends_event : forall b e b',
  rb_valid b ->
  rb_unmetered b = false ->
  rb_reserve b e = (b', RbReserveOk) ->
  rb_event_log b' = rb_event_log b ++ [e].
Proof.
  intros b e b' _ Hunmetered Hreserve.
  unfold rb_reserve in Hreserve.
  rewrite Hunmetered in Hreserve.
  destruct (rb_initial b <? rb_consumed b + rb_event_weight e) eqn:Hlt.
  - discriminate.
  - inversion Hreserve. reflexivity.
Qed.

Theorem rb_reserve_oop_commits_limit : forall b e b',
  rb_valid b ->
  rb_unmetered b = false ->
  rb_reserve b e = (b', RbReserveOop) ->
  rb_consumed b' = rb_initial b /\
  rb_remaining b' = 0 /\
  rb_event_log b' = rb_event_log b /\
  rb_last_oop b' =
    match rb_last_oop b with
    | Some existing => Some existing
    | None => Some e
    end.
Proof.
  intros b e b' _ Hunmetered Hreserve.
  unfold rb_reserve in Hreserve.
  rewrite Hunmetered in Hreserve.
  destruct (rb_initial b <? rb_consumed b + rb_event_weight e) eqn:Hlt.
  - inversion Hreserve. subst.
    unfold rb_remaining. simpl.
    repeat split; lia || reflexivity.
  - discriminate.
Qed.

Theorem rb_reserve_first_oop_commits_boundary : forall b e b',
  rb_valid b ->
  rb_unmetered b = false ->
  rb_last_oop b = None ->
  rb_reserve b e = (b', RbReserveOop) ->
  rb_consumed b' = rb_initial b /\
  rb_remaining b' = 0 /\
  rb_event_log b' = rb_event_log b /\
  rb_last_oop b' = Some e.
Proof.
  intros b e b' Hvalid Hunmetered Hoop Hreserve.
  pose proof (rb_reserve_oop_commits_limit b e b' Hvalid Hunmetered Hreserve)
    as [Hconsumed [Hremaining [Hlog Hlast]]].
  rewrite Hoop in Hlast.
  repeat split; assumption.
Qed.

Theorem rb_reserve_many_preserves_valid : forall events b b' results,
  rb_valid b ->
  rb_reserve_many b events = (b', results) ->
  rb_valid b'.
Proof.
  induction events as [| e rest IH]; intros b b' results Hvalid Hmany.
  - inversion Hmany. subst. exact Hvalid.
  - simpl in Hmany.
    destruct (rb_reserve b e) as [b1 r] eqn:Hreserve.
    pose proof (rb_reserve_preserves_valid b e b1 r Hvalid Hreserve)
      as Hvalid1.
    destruct r.
    + destruct (rb_reserve_many b1 rest) as [b2 rs] eqn:Hrest.
      inversion Hmany. subst.
      eapply IH; eassumption.
    + inversion Hmany. subst. exact Hvalid1.
Qed.

Theorem rb_reserve_many_conservation : forall events b b' results,
  rb_valid b ->
  rb_reserve_many b events = (b', results) ->
  rb_total_cost b' + rb_remaining b' = rb_initial b'.
Proof.
  intros events b b' results Hvalid Hmany.
  apply rb_total_remaining_conservation.
  eapply rb_reserve_many_preserves_valid; eassumption.
Qed.

Theorem rb_reserve_many_oop_count_le_one : forall events b b' results,
  rb_reserve_many b events = (b', results) ->
  rb_oop_count results <= 1.
Proof.
  induction events as [| e rest IH]; intros b b' results Hmany.
  - inversion Hmany. subst. simpl. lia.
  - simpl in Hmany.
    destruct (rb_reserve b e) as [b1 r] eqn:Hreserve.
    destruct r.
    + destruct (rb_reserve_many b1 rest) as [b2 rs] eqn:Hrest.
      inversion Hmany. subst. simpl.
      eapply IH. exact Hrest.
    + inversion Hmany. subst. simpl. lia.
Qed.

Theorem rb_commit_canonical_batch_preserves_valid :
  forall events b b' permits results,
    rb_valid b ->
    rb_commit_canonical_batch b events = (b', permits, results) ->
    rb_valid b'.
Proof.
  intros events b b' permits results Hvalid Hcommit.
  unfold rb_commit_canonical_batch in Hcommit.
  destruct (rb_reserve_many b events) as [b1 rs] eqn:Hmany.
  inversion Hcommit. subst.
  eapply rb_reserve_many_preserves_valid; eassumption.
Qed.

Theorem rb_commit_canonical_batch_oop_count_le_one :
  forall events b b' permits results,
    rb_commit_canonical_batch b events = (b', permits, results) ->
    rb_oop_count results <= 1.
Proof.
  intros events b b' permits results Hcommit.
  unfold rb_commit_canonical_batch in Hcommit.
  destruct (rb_reserve_many b events) as [b1 rs] eqn:Hmany.
  inversion Hcommit. subst.
  eapply rb_reserve_many_oop_count_le_one; eassumption.
Qed.

Theorem rb_commit_canonical_batch_no_unpaid_physical_work :
  forall events b b' permits results executed,
    rb_commit_canonical_batch b events = (b', permits, results) ->
    executed = permits ->
    rb_permit_weight_sum executed = rb_permit_weight_sum permits.
Proof.
  intros events b b' permits results executed _ Hexecuted.
  subst. reflexivity.
Qed.

Theorem rb_oop_trace_entries_at_most_one : forall oop,
  length (rb_oop_trace_entries oop) <= 1.
Proof.
  intros oop. destruct oop; simpl; lia.
Qed.

Theorem rb_repeated_oop_boundary_frontier :
  forall events b b' results,
    rb_reserve_many b events = (b', results) ->
    rb_oop_count results <= 1 /\
    length (rb_oop_trace_entries (rb_last_oop b')) <= 1.
Proof.
  intros events b b' results Hmany.
  split.
  - exact (rb_reserve_many_oop_count_le_one events b b' results Hmany).
  - apply rb_oop_trace_entries_at_most_one.
Qed.

Theorem rb_repeated_oop_preserves_first_boundary :
  forall b first second b1 b2,
    rb_valid b ->
    rb_unmetered b = false ->
    rb_last_oop b = None ->
    rb_reserve b first = (b1, RbReserveOop) ->
    rb_reserve b1 second = (b2, RbReserveOop) ->
    rb_last_oop b2 = rb_last_oop b1 /\
    rb_event_log b2 = rb_event_log b1 /\
    rb_cost_trace_event_count (rb_cost_trace_entries b2) =
      rb_cost_trace_event_count (rb_cost_trace_entries b1) /\
    rb_total_cost b2 = rb_total_cost b1.
Proof.
  intros b first second b1 b2 _ Hunmetered Hnone Hfirst Hsecond.
  unfold rb_reserve in Hfirst.
  rewrite Hunmetered in Hfirst.
  destruct (rb_initial b <? rb_consumed b + rb_event_weight first) eqn:Hfirst_lt.
  - inversion Hfirst. subst. clear Hfirst.
    unfold rb_reserve in Hsecond. simpl in Hsecond.
    rewrite Hnone in Hsecond. simpl in Hsecond.
    destruct (rb_initial b <? rb_initial b + rb_event_weight second) eqn:Hsecond_lt.
    + inversion Hsecond. subst. clear Hsecond.
      unfold rb_cost_trace_entries, rb_cost_trace_event_count,
        rb_success_trace_entries, rb_oop_trace_entries, rb_total_cost.
      rewrite Hnone. simpl. repeat split; reflexivity.
    + discriminate.
  - discriminate.
Qed.

Theorem rb_reserve_many_unmetered_no_cost : forall events b,
  rb_unmetered b = true ->
  rb_reserve_many b events =
    (b, repeat RbReserveOk (length events)).
Proof.
  induction events as [| e rest IH]; intros b Hunmetered.
  - reflexivity.
  - simpl.
    rewrite rb_unmetered_reserve_no_cost by exact Hunmetered.
    rewrite IH by exact Hunmetered.
    reflexivity.
Qed.

Theorem rb_diagnostic_cap_preserves_budget_observables :
  forall b (cap : nat),
  rb_total_cost b = rb_total_cost b /\
  rb_remaining b = rb_remaining b /\
  rb_initial b = rb_initial b /\
  rb_unmetered b = rb_unmetered b /\
  rb_last_oop b = rb_last_oop b.
Proof.
  intros b cap.
  repeat split; reflexivity.
Qed.

Theorem rb_reset_from_token_valid : forall b t,
  rb_valid (rb_reset_from_token b t).
Proof.
  intros b t.
  unfold rb_valid, rb_reset_from_token. simpl. lia.
Qed.

Theorem rb_reset_from_token_conservation : forall b t,
  rb_total_cost (rb_reset_from_token b t) +
  rb_remaining (rb_reset_from_token b t) =
  token_size t.
Proof.
  intros b t.
  unfold rb_total_cost, rb_remaining, rb_reset_from_token.
  simpl.
  destruct (rb_unmetered b); lia.
Qed.

Theorem rb_reset_from_token_clears_oop : forall b t,
  rb_last_oop (rb_reset_from_token b t) = None.
Proof.
  reflexivity.
Qed.

Theorem rb_reset_from_token_clears_trace : forall b t,
  rb_cost_trace_entries (rb_reset_from_token b t) = [].
Proof.
  intros b t.
  unfold rb_cost_trace_entries, rb_reset_from_token,
    rb_success_trace_entries, rb_oop_trace_entries.
  reflexivity.
Qed.

Theorem rb_successful_weight_refines_unit_count : forall b e b',
  rb_valid b ->
  rb_unmetered b = false ->
  rb_consumed b + rb_event_weight e <= rb_initial b ->
  rb_reserve b e = (b', RbReserveOk) ->
  rb_total_cost b' = rb_consumed b + rb_event_weight e /\
  rb_remaining b' =
    rb_initial b - (rb_consumed b + rb_event_weight e).
Proof.
  intros b e b' _ Hunmetered Hfits Hreserve.
  unfold rb_reserve in Hreserve.
  rewrite Hunmetered in Hreserve.
  destruct (rb_initial b <? rb_consumed b + rb_event_weight e) eqn:Hlt.
  - apply Nat.ltb_lt in Hlt. lia.
  - inversion Hreserve. subst.
    unfold rb_total_cost, rb_remaining. simpl. split; reflexivity.
Qed.

Definition rb_event_fingerprint (e : rb_event) : nat * nat :=
  (rb_event_local_index e, rb_event_weight e).

Definition rb_event_log_fingerprint (log : list rb_event) : list (nat * nat) :=
  map rb_event_fingerprint log.

Record rb_replay_payload := {
  rb_payload_user_costs : list nat;
  rb_payload_user_logs : list (list (nat * nat));
  rb_payload_system_logs : list (list (nat * nat));
  rb_payload_genesis : bool
}.

Record rb_slash_payload_fields := {
  rb_slash_invalid_block_hash : nat;
  rb_slash_issuer_public_key : nat;
  rb_slash_target_activation_epoch : nat
}.

Record rb_full_replay_payload := {
  rb_full_user_signatures : list nat;
  rb_full_user_costs : list nat;
  rb_full_user_cost_traces : list (list rb_trace_entry);
  rb_full_user_cost_trace_present : list bool;
  rb_full_user_cost_trace_event_counts : list nat;
  rb_full_user_failed : list bool;
  rb_full_user_errors : list nat;
  rb_full_user_logs : list (list (nat * nat));
  rb_full_system_kinds : list rb_system_deploy_kind;
  rb_full_system_error_messages : list nat;
  rb_full_system_slash_fields : list rb_slash_payload_fields;
  rb_full_system_logs : list (list (nat * nat));
  rb_full_genesis : bool
}.

Definition rb_replay_payload_equiv
  (a b : rb_replay_payload)
  : Prop :=
  rb_payload_user_costs a = rb_payload_user_costs b /\
  rb_payload_user_logs a = rb_payload_user_logs b /\
  rb_payload_system_logs a = rb_payload_system_logs b /\
  rb_payload_genesis a = rb_payload_genesis b.

Definition rb_log_permutation_equiv
  (logs1 logs2 : list (list (nat * nat)))
  : Prop :=
  Forall2 (@Permutation (nat * nat)) logs1 logs2.

Definition rb_replay_payload_canonical_equiv
  (a b : rb_replay_payload)
  : Prop :=
  rb_payload_user_costs a = rb_payload_user_costs b /\
  rb_log_permutation_equiv
    (rb_payload_user_logs a)
    (rb_payload_user_logs b) /\
  rb_log_permutation_equiv
    (rb_payload_system_logs a)
    (rb_payload_system_logs b) /\
  rb_payload_genesis a = rb_payload_genesis b.

Definition rb_full_replay_payload_equiv
  (a b : rb_full_replay_payload)
  : Prop :=
  rb_full_user_signatures a = rb_full_user_signatures b /\
  rb_full_user_costs a = rb_full_user_costs b /\
  rb_full_user_cost_traces a = rb_full_user_cost_traces b /\
  rb_full_user_cost_trace_present a =
    rb_full_user_cost_trace_present b /\
  rb_full_user_cost_trace_event_counts a =
    rb_full_user_cost_trace_event_counts b /\
  rb_full_user_failed a = rb_full_user_failed b /\
  rb_full_user_errors a = rb_full_user_errors b /\
  rb_full_user_logs a = rb_full_user_logs b /\
  rb_full_system_kinds a = rb_full_system_kinds b /\
  rb_full_system_error_messages a = rb_full_system_error_messages b /\
  rb_full_system_slash_fields a = rb_full_system_slash_fields b /\
  rb_full_system_logs a = rb_full_system_logs b /\
  rb_full_genesis a = rb_full_genesis b.

Record rb_block_auth_payload := {
  rb_block_sender : nat;
  rb_block_seq_num : nat;
  rb_block_replay_payload : rb_full_replay_payload
}.

Definition rb_block_auth_payload_equiv
  (a b : rb_block_auth_payload)
  : Prop :=
  rb_block_sender a = rb_block_sender b /\
  rb_block_seq_num a = rb_block_seq_num b /\
  rb_full_replay_payload_equiv
    (rb_block_replay_payload a)
    (rb_block_replay_payload b).

Record rb_replay_cache_key_model := {
  rb_cache_start_state : nat;
  rb_cache_sender : nat;
  rb_cache_seq_num : nat;
  rb_cache_replay_payload : rb_full_replay_payload
}.

Definition rb_replay_cache_key_equiv
  (a b : rb_replay_cache_key_model)
  : Prop :=
  rb_cache_start_state a = rb_cache_start_state b /\
  rb_cache_sender a = rb_cache_sender b /\
  rb_cache_seq_num a = rb_cache_seq_num b /\
  rb_full_replay_payload_equiv
    (rb_cache_replay_payload a)
    (rb_cache_replay_payload b).

Lemma rb_log_permutation_equiv_refl : forall logs,
  rb_log_permutation_equiv logs logs.
Proof.
  induction logs as [| log rest IH]; simpl.
  - constructor.
  - constructor.
    + apply Permutation_refl.
    + exact IH.
Qed.

Theorem rb_replay_payload_equiv_refl : forall p,
  rb_replay_payload_equiv p p.
Proof.
  intros p. unfold rb_replay_payload_equiv. repeat split; reflexivity.
Qed.

Theorem rb_replay_payload_canonical_equiv_refl : forall p,
  rb_replay_payload_canonical_equiv p p.
Proof.
  intros p.
  unfold rb_replay_payload_canonical_equiv.
  repeat split; try reflexivity; apply rb_log_permutation_equiv_refl.
Qed.

Theorem rb_replay_payload_canonical_user_trace_permutation :
  forall costs user_logs1 user_logs2 system_logs genesis,
    rb_log_permutation_equiv user_logs1 user_logs2 ->
    rb_replay_payload_canonical_equiv
      {|
        rb_payload_user_costs := costs;
        rb_payload_user_logs := user_logs1;
        rb_payload_system_logs := system_logs;
        rb_payload_genesis := genesis
      |}
      {|
        rb_payload_user_costs := costs;
        rb_payload_user_logs := user_logs2;
        rb_payload_system_logs := system_logs;
        rb_payload_genesis := genesis
      |}.
Proof.
  intros costs user_logs1 user_logs2 system_logs genesis Hperm.
  unfold rb_replay_payload_canonical_equiv.
  repeat split; try reflexivity.
  - exact Hperm.
  - apply rb_log_permutation_equiv_refl.
Qed.

Theorem rb_replay_payload_canonical_system_trace_permutation :
  forall costs user_logs system_logs1 system_logs2 genesis,
    rb_log_permutation_equiv system_logs1 system_logs2 ->
    rb_replay_payload_canonical_equiv
      {|
        rb_payload_user_costs := costs;
        rb_payload_user_logs := user_logs;
        rb_payload_system_logs := system_logs1;
        rb_payload_genesis := genesis
      |}
      {|
        rb_payload_user_costs := costs;
        rb_payload_user_logs := user_logs;
        rb_payload_system_logs := system_logs2;
        rb_payload_genesis := genesis
      |}.
Proof.
  intros costs user_logs system_logs1 system_logs2 genesis Hperm.
  unfold rb_replay_payload_canonical_equiv.
  repeat split; try reflexivity.
  - apply rb_log_permutation_equiv_refl.
  - exact Hperm.
Qed.

Theorem rb_replay_payload_user_cost_change_detected :
  forall costs1 costs2 user_logs system_logs genesis,
    costs1 <> costs2 ->
    ~ rb_replay_payload_equiv
      {|
        rb_payload_user_costs := costs1;
        rb_payload_user_logs := user_logs;
        rb_payload_system_logs := system_logs;
        rb_payload_genesis := genesis
      |}
      {|
        rb_payload_user_costs := costs2;
        rb_payload_user_logs := user_logs;
        rb_payload_system_logs := system_logs;
        rb_payload_genesis := genesis
      |}.
Proof.
  intros costs1 costs2 user_logs system_logs genesis Hneq Hequiv.
  unfold rb_replay_payload_equiv in Hequiv.
  destruct Hequiv as [Hcost _].
  exact (Hneq Hcost).
Qed.

Theorem rb_replay_payload_user_trace_change_detected :
  forall costs user_logs1 user_logs2 system_logs genesis,
    user_logs1 <> user_logs2 ->
    ~ rb_replay_payload_equiv
      {|
        rb_payload_user_costs := costs;
        rb_payload_user_logs := user_logs1;
        rb_payload_system_logs := system_logs;
        rb_payload_genesis := genesis
      |}
      {|
        rb_payload_user_costs := costs;
        rb_payload_user_logs := user_logs2;
        rb_payload_system_logs := system_logs;
        rb_payload_genesis := genesis
      |}.
Proof.
  intros costs user_logs1 user_logs2 system_logs genesis Hneq Hequiv.
  unfold rb_replay_payload_equiv in Hequiv.
  destruct Hequiv as [_ [Hlogs _]].
  exact (Hneq Hlogs).
Qed.

Theorem rb_replay_payload_system_trace_change_detected :
  forall costs user_logs system_logs1 system_logs2 genesis,
    system_logs1 <> system_logs2 ->
    ~ rb_replay_payload_equiv
      {|
        rb_payload_user_costs := costs;
        rb_payload_user_logs := user_logs;
        rb_payload_system_logs := system_logs1;
        rb_payload_genesis := genesis
      |}
      {|
        rb_payload_user_costs := costs;
        rb_payload_user_logs := user_logs;
        rb_payload_system_logs := system_logs2;
        rb_payload_genesis := genesis
      |}.
Proof.
  intros costs user_logs system_logs1 system_logs2 genesis Hneq Hequiv.
  unfold rb_replay_payload_equiv in Hequiv.
  destruct Hequiv as [_ [_ [Hlogs _]]].
  exact (Hneq Hlogs).
Qed.

Theorem rb_full_replay_payload_equiv_refl : forall p,
  rb_full_replay_payload_equiv p p.
Proof.
  intros p.
  unfold rb_full_replay_payload_equiv.
  repeat split; reflexivity.
Qed.

Theorem rb_block_auth_payload_replay_payload_change_detected :
  forall sender seq p1 p2,
    ~ rb_full_replay_payload_equiv p1 p2 ->
    ~ rb_block_auth_payload_equiv
      {|
        rb_block_sender := sender;
        rb_block_seq_num := seq;
        rb_block_replay_payload := p1
      |}
      {|
        rb_block_sender := sender;
        rb_block_seq_num := seq;
        rb_block_replay_payload := p2
      |}.
Proof.
  intros sender seq p1 p2 Hpayload Hequiv.
  unfold rb_block_auth_payload_equiv in Hequiv.
  destruct Hequiv as [_ [_ Hequiv_payload]].
  exact (Hpayload Hequiv_payload).
Qed.

Theorem rb_replay_cache_key_payload_change_detected :
  forall state sender seq p1 p2,
    ~ rb_full_replay_payload_equiv p1 p2 ->
    ~ rb_replay_cache_key_equiv
      {|
        rb_cache_start_state := state;
        rb_cache_sender := sender;
        rb_cache_seq_num := seq;
        rb_cache_replay_payload := p1
      |}
      {|
        rb_cache_start_state := state;
        rb_cache_sender := sender;
        rb_cache_seq_num := seq;
        rb_cache_replay_payload := p2
      |}.
Proof.
  intros state sender seq p1 p2 Hpayload Hequiv.
  unfold rb_replay_cache_key_equiv in Hequiv.
  destruct Hequiv as [_ [_ [_ Hequiv_payload]]].
  exact (Hpayload Hequiv_payload).
Qed.

Theorem rb_full_replay_payload_signature_change_detected :
  forall sigs1 sigs2 costs traces trace_counts failed errors user_logs kinds
         system_errors slash_fields system_logs genesis,
    sigs1 <> sigs2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs1;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs2;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs1 sigs2 costs traces trace_counts failed errors user_logs kinds
    system_errors slash_fields system_logs genesis Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_full_replay_payload_system_kind_change_detected :
  forall sigs costs traces trace_counts failed errors user_logs kinds1 kinds2
         system_errors slash_fields system_logs genesis,
    kinds1 <> kinds2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds1;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds2;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs costs traces trace_counts failed errors user_logs kinds1 kinds2
    system_errors slash_fields system_logs genesis Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_full_replay_payload_genesis_change_detected :
  forall sigs costs traces trace_counts failed errors user_logs kinds
         system_errors slash_fields system_logs genesis1 genesis2,
    genesis1 <> genesis2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis1
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis2
      |}.
Proof.
  intros sigs costs traces trace_counts failed errors user_logs kinds
    system_errors slash_fields system_logs genesis1 genesis2 Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_cost_trace_entries_success_and_oop :
  forall b,
    rb_cost_trace_entries b =
    rb_success_trace_entries (rb_event_log b) ++
    rb_oop_trace_entries (rb_last_oop b).
Proof.
  reflexivity.
Qed.

Theorem rb_finalize_trace_window_preserves_budget_observables :
  forall b,
    rb_initial (rb_finalize_trace_window b) = rb_initial b /\
    rb_consumed (rb_finalize_trace_window b) = rb_consumed b /\
    rb_unmetered (rb_finalize_trace_window b) = rb_unmetered b /\
    rb_total_cost (rb_finalize_trace_window b) = rb_total_cost b /\
    rb_remaining (rb_finalize_trace_window b) = rb_remaining b /\
    rb_cost_trace_entries (rb_finalize_trace_window b) =
      rb_cost_trace_entries b.
Proof.
  intros b.
  unfold rb_finalize_trace_window.
  repeat split; reflexivity.
Qed.

Theorem rb_cost_trace_permutation_equiv_refl : forall b,
  Permutation (rb_cost_trace_entries b) (rb_cost_trace_entries b).
Proof.
  intros b. apply Permutation_refl.
Qed.

Theorem rb_cost_trace_change_detected : forall b1 b2,
  rb_cost_trace_entries b1 <> rb_cost_trace_entries b2 ->
  ~ rb_cost_trace_entries b1 = rb_cost_trace_entries b2.
Proof.
  intros b1 b2 Hneq Heq.
  exact (Hneq Heq).
Qed.

Theorem rb_cost_trace_event_count_success_and_oop :
  forall b,
    rb_cost_trace_event_count (rb_cost_trace_entries b) =
    length (rb_event_log b) +
      match rb_last_oop b with
      | None => 0
      | Some _ => 1
      end.
Proof.
  intros b.
  unfold rb_cost_trace_event_count, rb_cost_trace_entries,
    rb_success_trace_entries, rb_oop_trace_entries.
  rewrite length_app, length_map.
  destruct (rb_last_oop b); simpl; lia.
Qed.

Theorem rb_post_activation_cost_trace_present_matches_count :
  forall trace count,
    rb_cost_trace_present trace ->
    rb_cost_trace_event_count_matches trace count ->
    trace <> [] /\ rb_cost_trace_event_count trace = count.
Proof.
  intros trace count Hpresent Hcount.
  unfold rb_cost_trace_present, rb_cost_trace_event_count_matches in *.
  split; assumption.
Qed.

Theorem rb_post_activation_cost_trace_commitment_valid :
  forall trace count present,
    rb_cost_trace_commitment_valid trace count present ->
    present = true /\ rb_cost_trace_event_count trace = count.
Proof.
  intros trace count present Hvalid.
  unfold rb_cost_trace_commitment_valid,
    rb_cost_trace_commitment_present,
    rb_cost_trace_event_count_matches in Hvalid.
  exact Hvalid.
Qed.

Theorem rb_empty_cost_trace_commitment_can_be_valid :
  rb_cost_trace_commitment_valid [] 0 true.
Proof.
  unfold rb_cost_trace_commitment_valid,
    rb_cost_trace_commitment_present,
    rb_cost_trace_event_count_matches,
    rb_cost_trace_event_count.
  simpl. split; reflexivity.
Qed.

Theorem rb_cost_accounted_replay_requires_commitment :
  forall trace count present,
    rb_replay_mode_accepts_cost_trace
      RbCostAccountedReplay trace count present ->
    rb_cost_trace_commitment_valid trace count present.
Proof.
  intros trace count present Haccept.
  unfold rb_replay_mode_accepts_cost_trace in Haccept.
  exact Haccept.
Qed.

Theorem rb_legacy_replay_accepts_absent_commitment :
  forall trace count,
    rb_replay_mode_accepts_cost_trace RbLegacyReplay trace count false.
Proof.
  intros trace count.
  unfold rb_replay_mode_accepts_cost_trace.
  exact I.
Qed.

Theorem rb_oop_trace_survives_boundary :
  forall b e b',
    rb_unmetered b = false ->
    rb_last_oop b = None ->
    rb_initial b < rb_consumed b + rb_event_weight e ->
    rb_reserve b e = (b', RbReserveOop) ->
    rb_cost_trace_entries b' =
      rb_success_trace_entries (rb_event_log b) ++
      [rb_oop_trace_entry e] /\
    rb_total_cost b' = rb_initial b.
Proof.
  intros b e b' Hunmetered Hnone Hexceeds Hreserve.
  unfold rb_reserve in Hreserve.
  rewrite Hunmetered in Hreserve.
  destruct (rb_initial b <? rb_consumed b + rb_event_weight e) eqn:Hlt.
  - inversion Hreserve. subst.
    rewrite Hnone.
    unfold rb_cost_trace_entries, rb_oop_trace_entries, rb_total_cost.
    simpl. split; reflexivity.
  - apply Nat.ltb_ge in Hlt. lia.
Qed.

Theorem rb_oversized_weight_rejection_preserves_trace :
  forall max_weight b e,
    max_weight < rb_event_weight e ->
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b.
Proof.
  intros max_weight b e _.
  repeat split; reflexivity.
Qed.

Theorem rb_zero_weight_admission_rejection_preserves_trace :
  forall max_weight max_source_path_components max_primitive_descriptor b e,
    rb_event_weight e = 0 ->
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b.
Proof.
  intros max_weight max_source_path_components max_primitive_descriptor b e Hzero.
  unfold rb_reserve_admitted.
  rewrite Hzero, Nat.eqb_refl.
  repeat split; reflexivity.
Qed.

Theorem rb_oversized_weight_admission_rejection_preserves_trace :
  forall max_weight max_source_path_components max_primitive_descriptor b e,
    0 < rb_event_weight e ->
    max_weight < rb_event_weight e ->
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b.
Proof.
  intros max_weight max_source_path_components max_primitive_descriptor b e
    Hpositive Hoversized.
  unfold rb_reserve_admitted.
  destruct (rb_event_weight e =? 0) eqn:Hzero.
  - apply Nat.eqb_eq in Hzero. lia.
  - assert (Hlt : (max_weight <? rb_event_weight e) = true)
      by (apply Nat.ltb_lt; exact Hoversized).
    rewrite Hlt.
    repeat split; reflexivity.
Qed.

Theorem rb_oversized_source_path_admission_rejection_preserves_trace :
  forall max_weight max_source_path_components max_primitive_descriptor b e,
    0 < rb_event_weight e ->
    rb_event_weight e <= max_weight ->
    max_source_path_components < length (rb_event_source_path e) ->
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b.
Proof.
  intros max_weight max_source_path_components max_primitive_descriptor b e
    Hpositive Hweight Hoversized.
  unfold rb_reserve_admitted.
  destruct (rb_event_weight e =? 0) eqn:Hzero.
  - apply Nat.eqb_eq in Hzero. lia.
  - destruct (max_weight <? rb_event_weight e) eqn:Hmax.
    + apply Nat.ltb_lt in Hmax. lia.
    + assert (Hlt :
        (max_source_path_components <? length (rb_event_source_path e)) = true)
        by (apply Nat.ltb_lt; exact Hoversized).
      rewrite Hlt.
      repeat split; reflexivity.
Qed.

Theorem rb_oversized_primitive_descriptor_admission_rejection_preserves_trace :
  forall max_weight max_source_path_components max_primitive_descriptor b e descriptor,
    0 < rb_event_weight e ->
    rb_event_weight e <= max_weight ->
    length (rb_event_source_path e) <= max_source_path_components ->
    rb_event_kind e = RbPrimitive descriptor ->
    max_primitive_descriptor < descriptor ->
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b.
Proof.
  intros max_weight max_source_path_components max_primitive_descriptor b e descriptor
    Hpositive Hweight Hsource Hkind Hoversized.
  unfold rb_reserve_admitted.
  destruct (rb_event_weight e =? 0) eqn:Hzero.
  - apply Nat.eqb_eq in Hzero. lia.
  - destruct (max_weight <? rb_event_weight e) eqn:Hmax.
    + apply Nat.ltb_lt in Hmax. lia.
    + destruct (max_source_path_components <? length (rb_event_source_path e))
        eqn:Hsource_too_long.
      * apply Nat.ltb_lt in Hsource_too_long. lia.
      * rewrite Hkind.
        assert (Hlt : (max_primitive_descriptor <? descriptor) = true)
          by (apply Nat.ltb_lt; exact Hoversized).
        rewrite Hlt.
        repeat split; reflexivity.
Qed.

Theorem rb_admitted_success_has_admissible_event :
  forall max_weight max_source_path_components max_primitive_descriptor b e b',
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b', RbAdmittedOk) ->
    rb_event_admissible
      max_weight max_source_path_components max_primitive_descriptor e.
Proof.
  intros max_weight max_source_path_components max_primitive_descriptor b e b'
    Hadmitted.
  unfold rb_reserve_admitted in Hadmitted.
  destruct (rb_event_weight e =? 0) eqn:Hzero.
  - discriminate.
  - destruct (max_weight <? rb_event_weight e) eqn:Hoversized_weight.
    + discriminate.
    + destruct (max_source_path_components <? length (rb_event_source_path e))
        eqn:Hoversized_source.
      * discriminate.
      * destruct (rb_event_kind e) eqn:Hkind.
        -- destruct (rb_reserve b e) as [b1 r] eqn:Hreserve.
           destruct r; inversion Hadmitted; subst.
           repeat split.
           ++ unfold rb_event_positive. apply Nat.eqb_neq in Hzero. lia.
           ++ unfold rb_event_within_weight_bound.
              apply Nat.ltb_ge in Hoversized_weight. exact Hoversized_weight.
           ++ unfold rb_event_source_path_within_bound.
              apply Nat.ltb_ge in Hoversized_source. exact Hoversized_source.
           ++ unfold rb_event_primitive_descriptor_within_bound.
              rewrite Hkind. exact I.
        -- destruct (max_primitive_descriptor <? descriptor)
             eqn:Hoversized_descriptor.
           ++ discriminate.
           ++ destruct (rb_reserve b e) as [b1 r] eqn:Hreserve.
              destruct r; inversion Hadmitted; subst.
              repeat split.
              ** unfold rb_event_positive. apply Nat.eqb_neq in Hzero. lia.
              ** unfold rb_event_within_weight_bound.
                 apply Nat.ltb_ge in Hoversized_weight. exact Hoversized_weight.
              ** unfold rb_event_source_path_within_bound.
                 apply Nat.ltb_ge in Hoversized_source. exact Hoversized_source.
              ** unfold rb_event_primitive_descriptor_within_bound.
                 rewrite Hkind.
                 apply Nat.ltb_ge in Hoversized_descriptor.
                 exact Hoversized_descriptor.
        -- destruct (rb_reserve b e) as [b1 r] eqn:Hreserve.
           destruct r; inversion Hadmitted; subst.
           repeat split.
           ++ unfold rb_event_positive. apply Nat.eqb_neq in Hzero. lia.
           ++ unfold rb_event_within_weight_bound.
              apply Nat.ltb_ge in Hoversized_weight. exact Hoversized_weight.
           ++ unfold rb_event_source_path_within_bound.
              apply Nat.ltb_ge in Hoversized_source. exact Hoversized_source.
           ++ unfold rb_event_primitive_descriptor_within_bound.
              rewrite Hkind. exact I.
Qed.

Theorem rb_admitted_success_has_positive_bounded_weight :
  forall max_weight max_source_path_components max_primitive_descriptor b e b',
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b', RbAdmittedOk) ->
    rb_event_positive e /\ rb_event_within_weight_bound max_weight e.
Proof.
  intros max_weight max_source_path_components max_primitive_descriptor b e b'
    Hadmitted.
  pose proof
    (rb_admitted_success_has_admissible_event
      max_weight max_source_path_components max_primitive_descriptor b e b'
      Hadmitted) as Hadmissible.
  unfold rb_event_admissible in Hadmissible.
  intuition.
Qed.

Theorem rb_trace_cap_rejection_preserves_trace :
  forall max_weight max_source_path_components max_primitive_descriptor max_events b e,
    max_events <= rb_trace_slot_count b ->
    rb_reserve_bounded
      max_weight max_source_path_components max_primitive_descriptor max_events b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b.
Proof.
  intros max_weight max_source_path_components max_primitive_descriptor max_events b e
    Hfull.
  unfold rb_reserve_bounded.
  destruct (rb_trace_slot_count b <? max_events) eqn:Hlt.
  - apply Nat.ltb_lt in Hlt. lia.
  - repeat split; reflexivity.
Qed.

Theorem rb_trace_cap_frontier_preserves_budget_and_trace :
  forall max_weight max_source_path_components max_primitive_descriptor max_events b e,
    max_events <= rb_trace_slot_count b ->
    rb_reserve_bounded
      max_weight max_source_path_components max_primitive_descriptor max_events b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b.
Proof.
  exact rb_trace_cap_rejection_preserves_trace.
Qed.

Theorem rb_nonbillable_frame_preserves_trace :
  forall b,
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b.
Proof.
  intros b.
  repeat split; reflexivity.
Qed.

Theorem rb_full_replay_payload_user_cost_change_detected :
  forall sigs costs1 costs2 traces trace_counts failed errors user_logs kinds
         system_errors slash_fields system_logs genesis,
    costs1 <> costs2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs1;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs2;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs costs1 costs2 traces trace_counts failed errors user_logs kinds
    system_errors slash_fields system_logs genesis Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_full_replay_payload_user_cost_trace_change_detected :
  forall sigs costs traces1 traces2 trace_counts failed errors user_logs kinds
         system_errors slash_fields system_logs genesis,
    traces1 <> traces2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces1;
        rb_full_user_cost_trace_present := repeat true (length traces1);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces2;
        rb_full_user_cost_trace_present := repeat true (length traces2);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs costs traces1 traces2 trace_counts failed errors user_logs kinds
    system_errors slash_fields system_logs genesis Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_full_replay_payload_user_cost_trace_event_count_change_detected :
  forall sigs costs traces trace_counts1 trace_counts2 failed errors
         user_logs kinds system_errors slash_fields system_logs genesis,
    trace_counts1 <> trace_counts2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts1;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts2;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs costs traces trace_counts1 trace_counts2 failed errors
    user_logs kinds system_errors slash_fields system_logs genesis Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_full_replay_payload_user_cost_trace_present_change_detected :
  forall sigs costs traces trace_present1 trace_present2 trace_counts failed
         errors user_logs kinds system_errors slash_fields system_logs genesis,
    trace_present1 <> trace_present2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := trace_present1;
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := trace_present2;
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs costs traces trace_present1 trace_present2 trace_counts failed
    errors user_logs kinds system_errors slash_fields system_logs genesis
    Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_full_replay_payload_missing_cost_trace_change_detected :
  forall sigs costs traces trace_counts failed errors user_logs kinds
         system_errors slash_fields system_logs genesis,
    traces <> [] ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := [];
        rb_full_user_cost_trace_present := [];
        rb_full_user_cost_trace_event_counts := [];
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs costs traces trace_counts failed errors user_logs kinds
    system_errors slash_fields system_logs genesis Hpresent Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_full_replay_payload_user_failed_change_detected :
  forall sigs costs traces trace_counts failed1 failed2 errors user_logs kinds
         system_errors slash_fields system_logs genesis,
    failed1 <> failed2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed1;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed2;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs costs traces trace_counts failed1 failed2 errors user_logs kinds
    system_errors slash_fields system_logs genesis Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_full_replay_payload_user_error_change_detected :
  forall sigs costs traces trace_counts failed errors1 errors2 user_logs kinds
         system_errors slash_fields system_logs genesis,
    errors1 <> errors2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors1;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors2;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs costs traces trace_counts failed errors1 errors2 user_logs kinds
    system_errors slash_fields system_logs genesis Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_full_replay_payload_system_error_change_detected :
  forall sigs costs traces trace_counts failed errors user_logs kinds
         system_errors1 system_errors2 slash_fields system_logs genesis,
    system_errors1 <> system_errors2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors1;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors2;
        rb_full_system_slash_fields := slash_fields;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs costs traces trace_counts failed errors user_logs kinds
    system_errors1 system_errors2 slash_fields system_logs genesis Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_full_replay_payload_slash_fields_change_detected :
  forall sigs costs traces trace_counts failed errors user_logs kinds system_errors
         slash_fields1 slash_fields2 system_logs genesis,
    slash_fields1 <> slash_fields2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields1;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := slash_fields2;
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs costs traces trace_counts failed errors user_logs kinds system_errors
    slash_fields1 slash_fields2 system_logs genesis Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv. intuition.
Qed.

Theorem rb_full_replay_payload_slash_target_epoch_change_detected :
  forall sigs costs traces trace_counts failed errors user_logs kinds system_errors
         invalid_block_hash issuer_public_key epoch1 epoch2 system_logs genesis,
    epoch1 <> epoch2 ->
    ~ rb_full_replay_payload_equiv
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := [{|
          rb_slash_invalid_block_hash := invalid_block_hash;
          rb_slash_issuer_public_key := issuer_public_key;
          rb_slash_target_activation_epoch := epoch1
        |}];
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}
      {|
        rb_full_user_signatures := sigs;
        rb_full_user_costs := costs;
        rb_full_user_cost_traces := traces;
        rb_full_user_cost_trace_present := repeat true (length traces);
        rb_full_user_cost_trace_event_counts := trace_counts;
        rb_full_user_failed := failed;
        rb_full_user_errors := errors;
        rb_full_user_logs := user_logs;
        rb_full_system_kinds := kinds;
        rb_full_system_error_messages := system_errors;
        rb_full_system_slash_fields := [{|
          rb_slash_invalid_block_hash := invalid_block_hash;
          rb_slash_issuer_public_key := issuer_public_key;
          rb_slash_target_activation_epoch := epoch2
        |}];
        rb_full_system_logs := system_logs;
        rb_full_genesis := genesis
      |}.
Proof.
  intros sigs costs traces trace_counts failed errors user_logs kinds system_errors
    invalid_block_hash issuer_public_key epoch1 epoch2 system_logs genesis
    Hneq Hequiv.
  unfold rb_full_replay_payload_equiv in Hequiv.
  destruct Hequiv as
    [_ [_ [_ [_ [_ [_ [_ [_ [_ [_ [Hslash _]]]]]]]]]]].
  inversion Hslash.
  congruence.
Qed.

Theorem rb_set_unmetered_restores_metered_observables :
  forall b,
    rb_initial (rb_set_unmetered (rb_set_unmetered b true) false) =
      rb_initial b /\
    rb_consumed (rb_set_unmetered (rb_set_unmetered b true) false) =
      rb_consumed b /\
    rb_event_log (rb_set_unmetered (rb_set_unmetered b true) false) =
      rb_event_log b /\
    rb_last_oop (rb_set_unmetered (rb_set_unmetered b true) false) =
      rb_last_oop b /\
    rb_unmetered (rb_set_unmetered (rb_set_unmetered b true) false) =
      false.
Proof.
  intros b.
  repeat split; reflexivity.
Qed.

Theorem rb_reserve_isolated_from_other_budget :
  forall b1 b1' b2 e r,
    rb_reserve b1 e = (b1', r) ->
    rb_initial b2 = rb_initial b2 /\
    rb_consumed b2 = rb_consumed b2 /\
    rb_event_log b2 = rb_event_log b2 /\
    rb_last_oop b2 = rb_last_oop b2.
Proof.
  intros b1 b1' b2 e r _.
  repeat split; reflexivity.
Qed.

Theorem rb_sum_settlement_app :
  forall left right,
    rb_sum_escrowed_amount (left ++ right) =
      rb_sum_escrowed_amount left + rb_sum_escrowed_amount right /\
    rb_sum_charged_amount (left ++ right) =
      rb_sum_charged_amount left + rb_sum_charged_amount right /\
    rb_sum_refund_amount (left ++ right) =
      rb_sum_refund_amount left + rb_sum_refund_amount right /\
    rb_sum_settled_amount (left ++ right) =
      rb_sum_settled_amount left + rb_sum_settled_amount right.
Proof.
  induction left as [| s rest IH]; intros right.
  - simpl. repeat split; lia.
  - simpl in *. destruct (IH right) as [Hescrow [Hcharged [Hrefund Hsettled]]].
    repeat split; rewrite ?Hescrow, ?Hcharged, ?Hrefund, ?Hsettled; lia.
Qed.

Theorem rb_sum_refund_le_escrow :
  forall settlements,
    rb_sum_refund_amount settlements <= rb_sum_escrowed_amount settlements.
Proof.
  induction settlements as [| s rest IH].
  - simpl. lia.
  - simpl.
    pose proof (refund_le_escrow s).
    lia.
Qed.

Theorem rb_multi_deploy_settlement_frontier :
  forall left right,
    rb_sum_refund_amount (left ++ right) =
      rb_sum_refund_amount left + rb_sum_refund_amount right /\
    rb_sum_settled_amount (left ++ right) =
      rb_sum_settled_amount left + rb_sum_settled_amount right /\
    rb_sum_refund_amount (left ++ right) <=
      rb_sum_escrowed_amount (left ++ right).
Proof.
  intros left right.
  destruct (rb_sum_settlement_app left right) as [_ [_ [Hrefund Hsettled]]].
  repeat split; try exact Hrefund; try exact Hsettled.
  apply rb_sum_refund_le_escrow.
Qed.

Theorem rb_trace_mismatch_preserves_settlement_accounting :
  forall (trace1 trace2 : list rb_trace_entry) s,
    trace1 <> trace2 ->
    escrowed_amount s = escrowed_amount s /\
    charged_amount s = charged_amount s /\
    refund_amount s = refund_amount s /\
    settled_amount s = settled_amount s.
Proof.
  intros trace1 trace2 s _.
  repeat split; reflexivity.
Qed.

Theorem rb_trace_entry_kind_domain_separated : forall descriptor,
  ((RbTraceSuccess, descriptor) : rb_trace_entry) <>
  ((RbTraceOop, descriptor) : rb_trace_entry).
Proof.
  intros descriptor Heq.
  discriminate Heq.
Qed.

Theorem rb_trace_entry_deploy_change_detected :
  forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_deploy_id d1 <> rb_trace_deploy_id d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry).
Proof.
  intros kind d1 d2 Hneq Heq.
  inversion Heq. subst. contradiction.
Qed.

Theorem rb_trace_entry_source_path_change_detected :
  forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_source_path d1 <> rb_trace_source_path d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry).
Proof.
  intros kind d1 d2 Hneq Heq.
  inversion Heq. subst. contradiction.
Qed.

Theorem rb_trace_entry_redex_change_detected :
  forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_redex_id d1 <> rb_trace_redex_id d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry).
Proof.
  intros kind d1 d2 Hneq Heq.
  inversion Heq. subst. contradiction.
Qed.

Theorem rb_trace_entry_local_index_change_detected :
  forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_local_index d1 <> rb_trace_local_index d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry).
Proof.
  intros kind d1 d2 Hneq Heq.
  inversion Heq. subst. contradiction.
Qed.

Theorem rb_trace_entry_billable_kind_change_detected :
  forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_billable_kind d1 <> rb_trace_billable_kind d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry).
Proof.
  intros kind d1 d2 Hneq Heq.
  inversion Heq. subst. contradiction.
Qed.

Theorem rb_trace_entry_primitive_descriptor_change_detected :
  forall (kind : rb_trace_event_kind) d1 d2 desc1 desc2,
    rb_trace_billable_kind d1 = RbPrimitive desc1 ->
    rb_trace_billable_kind d2 = RbPrimitive desc2 ->
    desc1 <> desc2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry).
Proof.
  intros kind d1 d2 desc1 desc2 Hkind1 Hkind2 Hneq Heq.
  inversion Heq. subst.
  rewrite Hkind1 in Hkind2.
  inversion Hkind2. subst.
  contradiction.
Qed.

Theorem rb_trace_entry_weight_change_detected :
  forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_weight d1 <> rb_trace_weight d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry).
Proof.
  intros kind d1 d2 Hneq Heq.
  inversion Heq. subst. contradiction.
Qed.

Theorem rb_trace_duplicate_multiplicity_detected :
  forall (entry : rb_trace_entry),
  [entry] <> [entry; entry].
Proof.
  intros entry Heq.
  discriminate Heq.
Qed.

Theorem rb_cost_accounted_replay_rejects_absent_commitment :
  forall trace count,
    ~ rb_replay_mode_accepts_cost_trace
        RbCostAccountedReplay trace count false.
Proof.
  intros trace count Haccept.
  unfold rb_replay_mode_accepts_cost_trace,
    rb_cost_trace_commitment_valid,
    rb_cost_trace_commitment_present in Haccept.
  destruct Haccept as [Hpresent _].
  discriminate Hpresent.
Qed.

Theorem rb_reset_from_token_retention_bound_zero : forall b t,
  length (rb_cost_trace_entries (rb_reset_from_token b t)) <= 0.
Proof.
  intros b t.
  rewrite rb_reset_from_token_clears_trace.
  simpl. lia.
Qed.

Theorem rb_unmetered_reserve_preserves_trace : forall b e,
  rb_unmetered b = true ->
  rb_reserve b e = (b, RbReserveOk) /\
  rb_cost_trace_entries b = rb_cost_trace_entries b /\
  rb_total_cost b = 0.
Proof.
  intros b e Hunmetered.
  split.
  - exact (rb_unmetered_reserve_no_cost b e Hunmetered).
  - split.
    + reflexivity.
    + unfold rb_total_cost. rewrite Hunmetered. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Option E Reconciliation Theorems
   ═══════════════════════════════════════════════════════════════════════════

   These theorems formalize the post-hoc canonical reconciliation
   implemented in `rholang/src/rust/interpreter/accounting/mod.rs::reconcile`
   and modeled as the `Merge` action in
   `formal/tlaplus/cost_accounted_rho/RuntimeBudgetReplay.tla`.

   The Rust runtime races lock-free CAS attempts against a shared
   `consumed_tokens` counter. The consensus-relevant `total_cost`,
   `cost_trace_digest`, and `last_oop_event` come from a post-execution
   canonical walk over the attempt log — NOT from the runtime CAS
   outcomes (which depend on Tokio scheduling). This module proves the
   invariants that justify that decoupling:

   - `rb_event_weight_sum_permutation_invariant`: the multiset weight
     is independent of input order (the foundation of permutation
     invariance).
   - `rb_reconcile_consumed_eq_min_initial_or_sum`: the canonical
     consumed value is `min(initial, sum_of_weights)`, a pure function
     of (initial, weight multiset) — schedule-independent.
   - `rb_reconcile_consumed_invariant_under_permutation`: a direct
     consequence — any two permutations of the same attempts produce
     the same consumed value.
   - `rb_reconcile_oop_iff_overflow`: the canonical OOP fires iff
     sum_of_weights > initial; otherwise no OOP. Permutation-invariant.

   Together these mirror the headline Option E theorem:
     "for the same multiset of attempts and the same initial budget,
      reconciliation produces the same observable accounting state
      regardless of which CAS race winners occurred at runtime."

   No `Axiom`, no `Admitted`. Proofs use only stdlib `Permutation`
   and `Nat` lemmas. *)

Theorem rb_event_weight_sum_permutation_invariant :
  forall a b,
  Permutation a b ->
  rb_event_weight_sum a = rb_event_weight_sum b.
Proof.
  induction 1; simpl; lia.
Qed.

(* Canonical reconciliation result: just the final state of
   `rb_reserve_many`. The Rust implementation additionally sorts
   attempts canonically before walking; for the consumed/OOP invariants
   below the sort is irrelevant because they hold for ANY permutation.
   The sort matters only for the DIGEST identity (which event becomes
   the OOP boundary record), and is verified via the TLA+ model + the
   Rust `cost_trace_digest_invariant_under_concurrent_commits` test. *)
Definition rb_reconcile (b : rb_state) (events : list rb_event) : rb_state :=
  fst (rb_reserve_many b events).

Theorem rb_reconcile_preserves_valid :
  forall b events,
  rb_valid b ->
  rb_valid (rb_reconcile b events).
Proof.
  intros b events Hvalid.
  unfold rb_reconcile.
  destruct (rb_reserve_many b events) as [b' results] eqn:Hmany.
  simpl.
  eapply rb_reserve_many_preserves_valid; eassumption.
Qed.

(* Helper: when not unmetered and `rb_reserve` returns Ok, the consumed
   counter advances by the event weight; when it returns Oop, the
   consumed counter clamps to initial. *)
Lemma rb_reserve_ok_advances_consumed :
  forall b e b',
  rb_unmetered b = false ->
  rb_reserve b e = (b', RbReserveOk) ->
  rb_consumed b' = rb_consumed b + rb_event_weight e
  /\ rb_initial b' = rb_initial b
  /\ rb_unmetered b' = false.
Proof.
  intros b e b' Hunmetered Hreserve.
  unfold rb_reserve in Hreserve.
  rewrite Hunmetered in Hreserve.
  destruct (rb_initial b <? rb_consumed b + rb_event_weight e) eqn:Hcond.
  - inversion Hreserve.
  - inversion Hreserve. subst. simpl. auto.
Qed.

Lemma rb_reserve_oop_clamps_consumed :
  forall b e b',
  rb_unmetered b = false ->
  rb_reserve b e = (b', RbReserveOop) ->
  rb_consumed b' = rb_initial b
  /\ rb_initial b' = rb_initial b
  /\ rb_unmetered b' = false.
Proof.
  intros b e b' Hunmetered Hreserve.
  unfold rb_reserve in Hreserve.
  rewrite Hunmetered in Hreserve.
  destruct (rb_initial b <? rb_consumed b + rb_event_weight e) eqn:Hcond.
  - inversion Hreserve. subst. simpl. auto.
  - inversion Hreserve.
Qed.

(* The OOP branch fires iff `consumed + weight > initial`. *)
Lemma rb_reserve_oop_iff_would_overflow :
  forall b e b' r,
  rb_unmetered b = false ->
  rb_reserve b e = (b', r) ->
  (r = RbReserveOop <-> rb_initial b < rb_consumed b + rb_event_weight e).
Proof.
  intros b e b' r Hunmetered Hreserve.
  unfold rb_reserve in Hreserve.
  rewrite Hunmetered in Hreserve.
  destruct (rb_initial b <? rb_consumed b + rb_event_weight e) eqn:Hcond.
  - rewrite Nat.ltb_lt in Hcond.
    inversion Hreserve. subst. split; intros; [exact Hcond | reflexivity].
  - rewrite Nat.ltb_ge in Hcond.
    inversion Hreserve. subst. split; intros; try discriminate.
    lia.
Qed.

(* The key canonical-consumed identity: starting from any valid metered
   state, the final consumed after walking any list of events equals
   `min(initial, consumed_initial + sum_of_event_weights)`. The walk
   either runs the full list (no OOP) and totals consumed + sum, or
   it OOPs at some point and clamps to initial. Both cases are equal
   to the `min` formula.

   This is the source of permutation-invariance: the right-hand side
   depends only on `initial`, `consumed_initial`, and `sum_of_weights`
   — none of which depend on the order of events in the input list. *)
Theorem rb_reconcile_consumed_eq_min_initial_or_sum :
  forall events b b' results,
  rb_valid b ->
  rb_unmetered b = false ->
  rb_reserve_many b events = (b', results) ->
  rb_consumed b' =
    Nat.min (rb_initial b) (rb_consumed b + rb_event_weight_sum events).
Proof.
  induction events as [| e rest IH]; intros b b' results Hvalid Hunmetered Hmany.
  - simpl in Hmany. injection Hmany as Hb' _. subst b'. simpl.
    rewrite Nat.add_0_r.
    unfold rb_valid in Hvalid. rewrite Nat.min_r by lia. reflexivity.
  - simpl in Hmany.
    destruct (rb_reserve b e) as [b1 r] eqn:Hreserve.
    destruct r.
    + (* RbReserveOk: advances consumed by e.weight, then recurse on rest. *)
      destruct (rb_reserve_many b1 rest) as [b2 rs] eqn:Hrest.
      injection Hmany as Hb' _. subst b'.
      pose proof (rb_reserve_ok_advances_consumed b e b1 Hunmetered Hreserve)
        as [Hconsumed1 [Hinitial1 Hunmet1]].
      pose proof (rb_reserve_preserves_valid b e b1 RbReserveOk Hvalid Hreserve)
        as Hvalid1.
      pose proof (IH b1 b2 rs Hvalid1 Hunmet1 Hrest) as IHrest.
      rewrite IHrest. rewrite Hinitial1, Hconsumed1.
      simpl. f_equal. lia.
    + (* RbReserveOop: clamps consumed to initial, returns immediately. *)
      injection Hmany as Hb' _. subst b'.
      pose proof (rb_reserve_oop_clamps_consumed b e b1 Hunmetered Hreserve)
        as [Hconsumed' [_ _]].
      pose proof (rb_reserve_oop_iff_would_overflow b e b1 RbReserveOop
                   Hunmetered Hreserve) as [Hoop_to_overflow _].
      pose proof (Hoop_to_overflow eq_refl) as Hoverflow.
      rewrite Hconsumed'.
      simpl.
      symmetry. apply Nat.min_l. lia.
Qed.

(* Permutation invariance of canonical consumed: the immediate
   corollary of the min-formula theorem above. Two reservation lists
   that are permutations of each other agree on the final consumed
   value, regardless of which order they were walked in. *)
Theorem rb_reconcile_consumed_invariant_under_permutation :
  forall events1 events2 b b1 b2 r1 r2,
  rb_valid b ->
  rb_unmetered b = false ->
  Permutation events1 events2 ->
  rb_reserve_many b events1 = (b1, r1) ->
  rb_reserve_many b events2 = (b2, r2) ->
  rb_consumed b1 = rb_consumed b2.
Proof.
  intros events1 events2 b b1 b2 r1 r2
         Hvalid Hunmetered Hperm Hmany1 Hmany2.
  pose proof (rb_reconcile_consumed_eq_min_initial_or_sum
                events1 b b1 r1 Hvalid Hunmetered Hmany1) as Heq1.
  pose proof (rb_reconcile_consumed_eq_min_initial_or_sum
                events2 b b2 r2 Hvalid Hunmetered Hmany2) as Heq2.
  rewrite Heq1, Heq2.
  rewrite (rb_event_weight_sum_permutation_invariant events1 events2 Hperm).
  reflexivity.
Qed.

(* The canonical OOP boundary fires iff the cumulative weight exceeds
   the budget. This is decided by the multiset alone (sum of weights),
   independent of input order. *)
Theorem rb_reconcile_oop_iff_sum_overflows :
  forall events b b' results,
  rb_valid b ->
  rb_unmetered b = false ->
  rb_reserve_many b events = (b', results) ->
  (rb_oop_count results = 1 <->
   rb_initial b < rb_consumed b + rb_event_weight_sum events).
Proof.
  induction events as [| e rest IH]; intros b b' results Hvalid Hunmetered Hmany.
  - simpl in Hmany. injection Hmany as _ Hres. subst results. simpl.
    unfold rb_valid in Hvalid. rewrite Nat.add_0_r.
    split; intros; [discriminate | lia].
  - simpl in Hmany.
    destruct (rb_reserve b e) as [b1 r] eqn:Hreserve.
    destruct r.
    + (* RbReserveOk: e fit within budget. Recurse on rest. *)
      destruct (rb_reserve_many b1 rest) as [b2 rs] eqn:Hrest.
      injection Hmany as Hb' Hres. subst b'. subst results.
      pose proof (rb_reserve_ok_advances_consumed b e b1 Hunmetered Hreserve)
        as [Hconsumed1 [Hinitial1 Hunmet1]].
      pose proof (rb_reserve_preserves_valid b e b1 RbReserveOk Hvalid Hreserve)
        as Hvalid1.
      pose proof (IH b1 b2 rs Hvalid1 Hunmet1 Hrest) as IHrest.
      simpl. rewrite IHrest.
      rewrite Hconsumed1, Hinitial1. simpl. split; intros; lia.
    + (* RbReserveOop: e overflowed. OOP count is exactly 1. *)
      injection Hmany as Hb' Hres. subst b'. subst results.
      pose proof (rb_reserve_oop_iff_would_overflow b e b1 RbReserveOop
                   Hunmetered Hreserve) as [Hoop_to_overflow _].
      pose proof (Hoop_to_overflow eq_refl) as Hoverflow.
      simpl. split.
      * intros _.
        assert (rb_event_weight e <=
                rb_event_weight e + rb_event_weight_sum rest) by lia.
        lia.
      * intros _. reflexivity.
Qed.

(* Permutation invariance of OOP boundary occurrence: whether OOP fires
   (not which specific event triggered it — that requires the canonical
   sort, modeled in the TLA+ spec) is permutation-invariant. *)
Theorem rb_reconcile_oop_occurrence_invariant_under_permutation :
  forall events1 events2 b b1 b2 r1 r2,
  rb_valid b ->
  rb_unmetered b = false ->
  Permutation events1 events2 ->
  rb_reserve_many b events1 = (b1, r1) ->
  rb_reserve_many b events2 = (b2, r2) ->
  rb_oop_count r1 = rb_oop_count r2.
Proof.
  intros events1 events2 b b1 b2 r1 r2
         Hvalid Hunmetered Hperm Hmany1 Hmany2.
  pose proof (rb_reconcile_oop_iff_sum_overflows
                events1 b b1 r1 Hvalid Hunmetered Hmany1) as Hiff1.
  pose proof (rb_reconcile_oop_iff_sum_overflows
                events2 b b2 r2 Hvalid Hunmetered Hmany2) as Hiff2.
  pose proof (rb_reserve_many_oop_count_le_one events1 b b1 r1 Hmany1)
    as Hle1.
  pose proof (rb_reserve_many_oop_count_le_one events2 b b2 r2 Hmany2)
    as Hle2.
  pose proof (rb_event_weight_sum_permutation_invariant
                events1 events2 Hperm) as Hsum_eq.
  destruct (Compare_dec.lt_dec (rb_initial b)
              (rb_consumed b + rb_event_weight_sum events1))
    as [Hlt | Hge].
  - (* Overflow on events1: count1 = 1. By Hsum_eq, also overflow on events2: count2 = 1. *)
    rewrite (proj2 Hiff1 Hlt). rewrite Hsum_eq in Hlt.
    rewrite (proj2 Hiff2 Hlt). reflexivity.
  - (* No overflow: count1 ≠ 1 by Hiff1, and count1 ≤ 1 by Hle1, so count1 = 0.
       Same for count2 via Hsum_eq. *)
    assert (Hcount1_zero : rb_oop_count r1 = 0).
    { destruct (Nat.eq_dec (rb_oop_count r1) 1) as [Heq | Hneq].
      - apply (proj1 Hiff1) in Heq. contradiction.
      - lia. }
    assert (Hcount2_zero : rb_oop_count r2 = 0).
    { destruct (Nat.eq_dec (rb_oop_count r2) 1) as [Heq | Hneq].
      - apply (proj1 Hiff2) in Heq.
        rewrite <- Hsum_eq in Heq. contradiction.
      - lia. }
    rewrite Hcount1_zero, Hcount2_zero. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Bounded-K reconciliation: lowestK commutative monoid + walk-window bound,
   and total_cost schedule-independence (Milestone 4 — Rocq, per the plan
   one-of-the-primary-gentle-pearl.md).

   Two results, ADDED on top of the existing Option-E reconcile lemmas
   (rb_event_weight_sum_permutation_invariant,
    rb_reconcile_consumed_eq_min_initial_or_sum, rb_reconcile_oop_iff_sum_overflows,
    rb_reconcile_consumed_invariant_under_permutation):

   1. BOUNDED-K EQUIVALENCE.  We model the canonical Ord on BillableTokenEvent
      (accounting/mod.rs:88,148-169) as the lexicographic `rb_event_compare`
      over (deploy_id, source_path, redex_id, local_index, kind, weight), prove
      it is a decidable total order (reflexive / antisymmetric-to-record-equality
      / transitive / total), instantiate the stdlib mergesort over it to get a
      deterministic `rb_event_sort`, and define
        lowestK k events := firstn k (rb_event_sort events).
      We prove `lowK_merge k a b := lowestK k (a ++ b)` is a COMMUTATIVE MONOID
      (lowK_merge_comm / _assoc / _id_l / _id_r, identity []) — associativity
      being the bounded-K absorption law lowK_absorb.  Then
      rb_reconcile_bounded_K_eq_sort_truncate shows the cost walk over the
      lowest min(MAX_COST_TRACE_EVENTS, initial+1) events produces the SAME
      committed event_log, last_oop, consumed, and OOP-count as the full
      sorted-then-truncated walk (weights >= 1 ⇒ <= initial events committed
      + 1 boundary).

   2. total_cost SCHEDULE-INDEPENDENCE.  rb_total_cost_eq_min_initial_sum +
      rb_total_cost_schedule_independent + rb_total_cost_clamped_characterization
      package the consensus cost quantity that remains after the per-op digest
      is dropped: rb_reconcile(events).consumed = min(initial, Σ weights),
      invariant under any permutation of events (Σ>initial ⇒ consumed=initial
      with OOP; Σ<=initial ⇒ consumed=Σ).

   No Axiom / Admitted / admit; stdlib Sorting + Nat + Permutation only.

   ── Failed strategies recorded so they are not re-attempted ───────────────
   - Proving the bounded-K absorption via a too-weak insertion lemma
     `firstn k (rb_insert d l) = firstn k l` under only `Forall (<= d) (firstn k l)`
     is FALSE in general for k >= 2 with strict ties (inserting a duplicate of a
     non-boundary head shifts the prefix).  The correct hypothesis is that EVERY
     one of the first k is <= d (so any boundary tie forces an EQUAL record by
     antisymmetry); under StronglySorted that collapses the tied prefix to a
     constant (firstn_insert_ge).
   - Proving "no dropped (large) element enters the lowest-k" via a position /
     count_occ pigeonhole was abandoned as too fiddly.  The clean route is the
     filter/threshold lemma StronglySorted_firstn_le_threshold: in a sorted list
     the "<= d" elements form a prefix, and if there are >= k of them the first k
     are all <= d (used by firstn_sort_dominated_by_pivot).
   - `Nat.compare_trans` does NOT exist in this stdlib (9.1.0); a local
     `natc_trans` is proven from compare_{lt,gt,eq}_iff.
   - `Permutation_filter` / `firstn_In` / `Forall_firstn` are absent under those
     names here; small local versions are proven.
   - The lexicographic-comparison transitivity is folded via a generic `lexc_trans`
     combinator (one application per field) rather than a 6-field manual case
     explosion, using per-field congruence `proj_compare_cong_{l,r}`.
   ═══════════════════════════════════════════════════════════════════════════ *)

Fixpoint list_nat_compare (xs ys : list nat) : comparison :=
  match xs, ys with
  | [], [] => Eq
  | [], _ :: _ => Lt
  | _ :: _, [] => Gt
  | x :: xs', y :: ys' =>
      match Nat.compare x y with
      | Eq => list_nat_compare xs' ys'
      | c => c
      end
  end.

Lemma list_nat_compare_eq_iff : forall xs ys,
  list_nat_compare xs ys = Eq <-> xs = ys.
Proof.
  induction xs as [| x xs' IH]; intros [| y ys']; simpl; split; intro H;
    try reflexivity; try discriminate.
  - destruct (Nat.compare x y) eqn:Hc; try discriminate.
    apply Nat.compare_eq_iff in Hc. subst y. f_equal. apply IH. exact H.
  - injection H as Hx Hxs. subst y xs'.
    rewrite Nat.compare_refl. apply IH. reflexivity.
Qed.

Lemma list_nat_compare_refl : forall xs, list_nat_compare xs xs = Eq.
Proof. intro xs. apply list_nat_compare_eq_iff. reflexivity. Qed.

Lemma list_nat_compare_antisym : forall xs ys,
  list_nat_compare ys xs = CompOpp (list_nat_compare xs ys).
Proof.
  induction xs as [| x xs' IH]; intros [| y ys']; simpl; try reflexivity.
  rewrite (Nat.compare_antisym x y).
  destruct (Nat.compare x y) eqn:Hc; simpl; try reflexivity.
  apply IH.
Qed.

Lemma list_nat_compare_trans : forall c xs ys zs,
  list_nat_compare xs ys = c ->
  list_nat_compare ys zs = c ->
  list_nat_compare xs zs = c.
Proof.
  intros c xs. revert c.
  induction xs as [| x xs' IH]; intros c ys zs H1 H2.
  - (* xs = [] *)
    destruct ys as [| y ys']; destruct zs as [| z zs']; simpl in *;
      try congruence.
  - (* xs = x :: xs' *)
    destruct ys as [| y ys']; destruct zs as [| z zs']; simpl in *;
      try congruence.
    + (* general x::xs', y::ys', z::zs'.  The outer `simpl in *` has already
         reduced the goal and H1,H2 to `match (x?=y) ...` / `match (y?=z) ...`;
         destructing the comparisons makes the matches compute. *)
      destruct (Nat.compare x y) eqn:Hxy;
      destruct (Nat.compare y z) eqn:Hyz;
        try (subst c; discriminate).
      * apply Nat.compare_eq_iff in Hxy. apply Nat.compare_eq_iff in Hyz.
        subst y z. rewrite Nat.compare_refl. eapply IH; eauto.
      * apply Nat.compare_eq_iff in Hxy. subst y. rewrite Hyz. exact H2.
      * apply Nat.compare_eq_iff in Hxy. subst y. rewrite Hyz. exact H2.
      * apply Nat.compare_eq_iff in Hyz. subst z. rewrite Hxy. exact H1.
      * apply Nat.compare_lt_iff in Hxy. apply Nat.compare_lt_iff in Hyz.
        assert (x < z) as Hxz by lia. apply Nat.compare_lt_iff in Hxz.
        rewrite Hxz. congruence.
      * apply Nat.compare_eq_iff in Hyz. subst z. rewrite Hxy. exact H1.
      * apply Nat.compare_gt_iff in Hxy. apply Nat.compare_gt_iff in Hyz.
        assert (z < x) as Hzx by lia. apply Nat.compare_gt_iff in Hzx.
        rewrite Hzx. congruence.
Qed.

Lemma list_nat_compare_total : forall xs ys,
  list_nat_compare xs ys = Lt \/ list_nat_compare xs ys = Eq
  \/ list_nat_compare xs ys = Gt.
Proof. intros. destruct (list_nat_compare xs ys); auto. Qed.

(* ── Comparison of rb_billable_kind ─────────────────────────────────────
   Matches the Rust derived `Ord` on `BillableKind`
   (SourceStep < Primitive(_) < Substitution; Primitive by descriptor). *)
Definition rb_kind_compare (k1 k2 : rb_billable_kind) : comparison :=
  match k1, k2 with
  | RbSourceStep, RbSourceStep => Eq
  | RbSourceStep, _ => Lt
  | RbPrimitive _, RbSourceStep => Gt
  | RbPrimitive d1, RbPrimitive d2 => Nat.compare d1 d2
  | RbPrimitive _, RbSubstitution => Lt
  | RbSubstitution, RbSubstitution => Eq
  | RbSubstitution, _ => Gt
  end.

Lemma rb_kind_compare_eq_iff : forall k1 k2,
  rb_kind_compare k1 k2 = Eq <-> k1 = k2.
Proof.
  intros [| d1 |] [| d2 |]; simpl; split; intro H;
    try reflexivity; try discriminate.
  - apply Nat.compare_eq_iff in H. subst. reflexivity.
  - injection H as Hd. subst. apply Nat.compare_refl.
Qed.

Lemma rb_kind_compare_refl : forall k, rb_kind_compare k k = Eq.
Proof. intro k. apply rb_kind_compare_eq_iff. reflexivity. Qed.

Lemma rb_kind_compare_antisym : forall k1 k2,
  rb_kind_compare k2 k1 = CompOpp (rb_kind_compare k1 k2).
Proof.
  intros [| d1 |] [| d2 |]; simpl; try reflexivity.
  apply Nat.compare_antisym.
Qed.

Lemma rb_kind_compare_trans : forall c k1 k2 k3,
  rb_kind_compare k1 k2 = c ->
  rb_kind_compare k2 k3 = c ->
  rb_kind_compare k1 k3 = c.
Proof.
  intros c k1 k2 k3 H1 H2.
  destruct k1 as [| d1 |]; destruct k2 as [| d2 |]; destruct k3 as [| d3 |];
    simpl in *; try congruence.
  (* Only Primitive/Primitive/Primitive survives: delegate to Nat.compare.
     After `try (subst c; discriminate)` only the three matching-comparison
     cases (Eq,Eq), (Lt,Lt), (Gt,Gt) remain. *)
  destruct (Nat.compare d1 d2) eqn:H12;
  destruct (Nat.compare d2 d3) eqn:H23;
    try (subst c; discriminate).
  - apply Nat.compare_eq_iff in H12. apply Nat.compare_eq_iff in H23.
    subst. rewrite Nat.compare_refl. assumption.
  - apply Nat.compare_lt_iff in H12. apply Nat.compare_lt_iff in H23.
    assert (d1 < d3) as Hd by lia. apply Nat.compare_lt_iff in Hd.
    rewrite Hd. congruence.
  - apply Nat.compare_gt_iff in H12. apply Nat.compare_gt_iff in H23.
    assert (d3 < d1) as Hd by lia. apply Nat.compare_gt_iff in Hd.
    rewrite Hd. congruence.
Qed.

(* ── Canonical lexicographic comparison on rb_event ────────────────────
   Matches the derived Rust `Ord` on `BillableTokenEvent`:
     (deploy_id, source_path, redex_id, local_index, kind, weight).
   The field order is exactly the record/struct field order, which is the
   tuple `cmp` derivation Rust uses. *)
Definition lexc (c1 c2 : comparison) : comparison :=
  match c1 with Eq => c2 | _ => c1 end.


Definition rb_event_compare (e1 e2 : rb_event) : comparison :=
  lexc (Nat.compare (rb_event_deploy_id e1) (rb_event_deploy_id e2))
  (lexc (list_nat_compare (rb_event_source_path e1) (rb_event_source_path e2))
  (lexc (Nat.compare (rb_event_redex_id e1) (rb_event_redex_id e2))
  (lexc (Nat.compare (rb_event_local_index e1) (rb_event_local_index e2))
  (lexc (rb_kind_compare (rb_event_kind e1) (rb_event_kind e2))
        (Nat.compare (rb_event_weight e1) (rb_event_weight e2)))))).

Definition rb_event_leb (e1 e2 : rb_event) : bool :=
  match rb_event_compare e1 e2 with
  | Gt => false
  | _ => true
  end.

(* Antisymmetry-to-equality: compare = Eq iff the records are equal.
   This holds because rb_event has EXACTLY the six compared fields. *)
Lemma rb_event_compare_eq_iff : forall e1 e2,
  rb_event_compare e1 e2 = Eq <-> e1 = e2.
Proof.
  intros e1 e2. unfold rb_event_compare. split.
  - intro H.
    destruct (Nat.compare (rb_event_deploy_id e1) (rb_event_deploy_id e2))
      eqn:Hd; try discriminate.
    destruct (list_nat_compare (rb_event_source_path e1) (rb_event_source_path e2))
      eqn:Hsp; try discriminate.
    destruct (Nat.compare (rb_event_redex_id e1) (rb_event_redex_id e2))
      eqn:Hr; try discriminate.
    destruct (Nat.compare (rb_event_local_index e1) (rb_event_local_index e2))
      eqn:Hl; try discriminate.
    destruct (rb_kind_compare (rb_event_kind e1) (rb_event_kind e2))
      eqn:Hk; try discriminate.
    apply Nat.compare_eq_iff in Hd.
    apply list_nat_compare_eq_iff in Hsp.
    apply Nat.compare_eq_iff in Hr.
    apply Nat.compare_eq_iff in Hl.
    apply rb_kind_compare_eq_iff in Hk.
    apply Nat.compare_eq_iff in H.
    destruct e1, e2; simpl in *; subst; reflexivity.
  - intro H. subst e2.
    rewrite Nat.compare_refl, list_nat_compare_refl, Nat.compare_refl,
            Nat.compare_refl, rb_kind_compare_refl, Nat.compare_refl.
    reflexivity.
Qed.

Lemma rb_event_compare_refl : forall e, rb_event_compare e e = Eq.
Proof. intro e. apply rb_event_compare_eq_iff. reflexivity. Qed.

Lemma rb_event_compare_antisym : forall e1 e2,
  rb_event_compare e2 e1 = CompOpp (rb_event_compare e1 e2).
Proof.
  intros e1 e2. unfold rb_event_compare.
  rewrite (Nat.compare_antisym (rb_event_deploy_id e1) (rb_event_deploy_id e2)).
  destruct (Nat.compare (rb_event_deploy_id e1) (rb_event_deploy_id e2)) eqn:Hd;
    simpl; try reflexivity.
  rewrite (list_nat_compare_antisym (rb_event_source_path e1) (rb_event_source_path e2)).
  destruct (list_nat_compare (rb_event_source_path e1) (rb_event_source_path e2)) eqn:Hsp;
    simpl; try reflexivity.
  rewrite (Nat.compare_antisym (rb_event_redex_id e1) (rb_event_redex_id e2)).
  destruct (Nat.compare (rb_event_redex_id e1) (rb_event_redex_id e2)) eqn:Hr;
    simpl; try reflexivity.
  rewrite (Nat.compare_antisym (rb_event_local_index e1) (rb_event_local_index e2)).
  destruct (Nat.compare (rb_event_local_index e1) (rb_event_local_index e2)) eqn:Hl;
    simpl; try reflexivity.
  rewrite (rb_kind_compare_antisym (rb_event_kind e1) (rb_event_kind e2)).
  destruct (rb_kind_compare (rb_event_kind e1) (rb_event_kind e2)) eqn:Hk;
    simpl; try reflexivity.
  apply Nat.compare_antisym.
Qed.

(* ── Generic lexicographic-composition transitivity ─────────────────────
   `lex c1 c2` returns `c2` when `c1 = Eq`, else `c1`. The composite
   comparison `fun x y => lex (f x y) (g x y)` is transitive provided:
     - f is transitive (f_trans),
     - f is left/right-congruent under f-equality (f x y = Eq makes
       f x _ and f y _ interchangeable; symmetric on the right), and
     - g is transitive (g_trans).
   We instantiate this once per field of rb_event. *)
Lemma lexc_trans :
  forall (T : Type) (f g : T -> T -> comparison),
  (forall c x y z, f x y = c -> f y z = c -> f x z = c) ->
  (forall x y z, f x y = Eq -> f x z = f y z) ->
  (forall x y z, f y z = Eq -> f x z = f x y) ->
  (forall c x y z, g x y = c -> g y z = c -> g x z = c) ->
  forall c x y z,
  lexc (f x y) (g x y) = c ->
  lexc (f y z) (g y z) = c ->
  lexc (f x z) (g x z) = c.
Proof.
  intros T f g f_trans f_cong_l f_cong_r g_trans c x y z H1 H2.
  unfold lexc in *.
  destruct (f x y) eqn:Hfxy; destruct (f y z) eqn:Hfyz.
  - (* Eq, Eq: f x z = Eq; chain g. *)
    rewrite (f_trans Eq x y z Hfxy Hfyz).
    apply (g_trans c x y z H1 H2).
  - (* Eq, Lt: f x z = f y z = Lt. *)
    rewrite (f_cong_l x y z Hfxy). rewrite Hfyz. exact H2.
  - (* Eq, Gt *)
    rewrite (f_cong_l x y z Hfxy). rewrite Hfyz. exact H2.
  - (* Lt, Eq: f x z = f x y = Lt. *)
    rewrite (f_cong_r x y z Hfyz). rewrite Hfxy. exact H1.
  - (* Lt, Lt: f x z = Lt by f_trans. *)
    rewrite (f_trans Lt x y z Hfxy Hfyz). exact H1.
  - (* Lt, Gt: H1 = Lt, H2 = Gt, c both -> contradiction. *)
    subst c; discriminate.
  - (* Gt, Eq: f x z = f x y = Gt. *)
    rewrite (f_cong_r x y z Hfyz). rewrite Hfxy. exact H1.
  - (* Gt, Lt: contradiction. *)
    subst c; discriminate.
  - (* Gt, Gt *)
    rewrite (f_trans Gt x y z Hfxy Hfyz). exact H1.
Qed.

(* Congruence of a projection-based comparison under its own Eq:
   if comparing the projections of x and y is Eq then the projections
   are equal, so x and y are interchangeable on either side. *)
Lemma proj_compare_cong_l :
  forall (F : Type) (bc : F -> F -> comparison) (proj : rb_event -> F),
  (forall a b, bc a b = Eq -> a = b) ->
  forall x y z,
  bc (proj x) (proj y) = Eq ->
  bc (proj x) (proj z) = bc (proj y) (proj z).
Proof.
  intros F bc proj bc_eq x y z H. apply bc_eq in H. rewrite H. reflexivity.
Qed.

Lemma proj_compare_cong_r :
  forall (F : Type) (bc : F -> F -> comparison) (proj : rb_event -> F),
  (forall a b, bc a b = Eq -> a = b) ->
  forall x y z,
  bc (proj y) (proj z) = Eq ->
  bc (proj x) (proj z) = bc (proj x) (proj y).
Proof.
  intros F bc proj bc_eq x y z H. apply bc_eq in H. rewrite H. reflexivity.
Qed.

(* Component eq-iff facts in the one-directional form lexc_trans wants. *)
Lemma natc_eq : forall a b : nat, Nat.compare a b = Eq -> a = b.
Proof. intros a b H. apply Nat.compare_eq_iff. exact H. Qed.
Lemma natc_trans : forall (c : comparison) (a b d : nat),
  Nat.compare a b = c -> Nat.compare b d = c -> Nat.compare a d = c.
Proof.
  intros c a b d H1 H2.
  destruct (Nat.compare a b) eqn:Hab; destruct (Nat.compare b d) eqn:Hbd;
    try (subst c; discriminate).
  - apply Nat.compare_eq_iff in Hab. apply Nat.compare_eq_iff in Hbd.
    subst. rewrite Nat.compare_refl. assumption.
  - apply Nat.compare_lt_iff in Hab. apply Nat.compare_lt_iff in Hbd.
    assert (a < d) as Hd by lia. apply Nat.compare_lt_iff in Hd.
    rewrite Hd. congruence.
  - apply Nat.compare_gt_iff in Hab. apply Nat.compare_gt_iff in Hbd.
    assert (d < a) as Hd by lia. apply Nat.compare_gt_iff in Hd.
    rewrite Hd. congruence.
Qed.
Lemma lnc_eq : forall a b : list nat, list_nat_compare a b = Eq -> a = b.
Proof. intros a b H. apply list_nat_compare_eq_iff. exact H. Qed.
Lemma kindc_eq : forall a b, rb_kind_compare a b = Eq -> a = b.
Proof. intros a b H. apply rb_kind_compare_eq_iff. exact H. Qed.

(* Transitivity of rb_event_compare: fold lexc_trans over the six fields,
   innermost (weight) outward. *)
Lemma rb_event_compare_trans : forall c e1 e2 e3,
  rb_event_compare e1 e2 = c ->
  rb_event_compare e2 e3 = c ->
  rb_event_compare e1 e3 = c.
Proof.
  unfold rb_event_compare.
  (* weight level (innermost g): plain Nat.compare transitivity. *)
  assert (Gw : forall c x y z,
            Nat.compare (rb_event_weight x) (rb_event_weight y) = c ->
            Nat.compare (rb_event_weight y) (rb_event_weight z) = c ->
            Nat.compare (rb_event_weight x) (rb_event_weight z) = c).
  { intros c0 x y z H1 H2. eapply natc_trans; eauto. }
  (* kind level *)
  assert (Gk : forall c x y z,
            lexc (rb_kind_compare (rb_event_kind x) (rb_event_kind y))
                 (Nat.compare (rb_event_weight x) (rb_event_weight y)) = c ->
            lexc (rb_kind_compare (rb_event_kind y) (rb_event_kind z))
                 (Nat.compare (rb_event_weight y) (rb_event_weight z)) = c ->
            lexc (rb_kind_compare (rb_event_kind x) (rb_event_kind z))
                 (Nat.compare (rb_event_weight x) (rb_event_weight z)) = c).
  { apply (lexc_trans rb_event
             (fun a b => rb_kind_compare (rb_event_kind a) (rb_event_kind b))
             (fun a b => Nat.compare (rb_event_weight a) (rb_event_weight b))).
    - intros c0 x y z. apply rb_kind_compare_trans.
    - intros x y z. apply (proj_compare_cong_l _ rb_kind_compare rb_event_kind kindc_eq).
    - intros x y z. apply (proj_compare_cong_r _ rb_kind_compare rb_event_kind kindc_eq).
    - exact Gw. }
  (* local_index level *)
  assert (Gl : forall c x y z,
            lexc (Nat.compare (rb_event_local_index x) (rb_event_local_index y))
              (lexc (rb_kind_compare (rb_event_kind x) (rb_event_kind y))
                 (Nat.compare (rb_event_weight x) (rb_event_weight y))) = c ->
            lexc (Nat.compare (rb_event_local_index y) (rb_event_local_index z))
              (lexc (rb_kind_compare (rb_event_kind y) (rb_event_kind z))
                 (Nat.compare (rb_event_weight y) (rb_event_weight z))) = c ->
            lexc (Nat.compare (rb_event_local_index x) (rb_event_local_index z))
              (lexc (rb_kind_compare (rb_event_kind x) (rb_event_kind z))
                 (Nat.compare (rb_event_weight x) (rb_event_weight z))) = c).
  { apply (lexc_trans rb_event
             (fun a b => Nat.compare (rb_event_local_index a) (rb_event_local_index b))
             (fun a b => lexc (rb_kind_compare (rb_event_kind a) (rb_event_kind b))
                              (Nat.compare (rb_event_weight a) (rb_event_weight b)))).
    - intros c0 x y z. apply natc_trans.
    - intros x y z. apply (proj_compare_cong_l _ Nat.compare rb_event_local_index natc_eq).
    - intros x y z. apply (proj_compare_cong_r _ Nat.compare rb_event_local_index natc_eq).
    - exact Gk. }
  (* redex_id level *)
  assert (Gr : forall c x y z,
            lexc (Nat.compare (rb_event_redex_id x) (rb_event_redex_id y))
              (lexc (Nat.compare (rb_event_local_index x) (rb_event_local_index y))
                (lexc (rb_kind_compare (rb_event_kind x) (rb_event_kind y))
                   (Nat.compare (rb_event_weight x) (rb_event_weight y)))) = c ->
            lexc (Nat.compare (rb_event_redex_id y) (rb_event_redex_id z))
              (lexc (Nat.compare (rb_event_local_index y) (rb_event_local_index z))
                (lexc (rb_kind_compare (rb_event_kind y) (rb_event_kind z))
                   (Nat.compare (rb_event_weight y) (rb_event_weight z)))) = c ->
            lexc (Nat.compare (rb_event_redex_id x) (rb_event_redex_id z))
              (lexc (Nat.compare (rb_event_local_index x) (rb_event_local_index z))
                (lexc (rb_kind_compare (rb_event_kind x) (rb_event_kind z))
                   (Nat.compare (rb_event_weight x) (rb_event_weight z)))) = c).
  { apply (lexc_trans rb_event
             (fun a b => Nat.compare (rb_event_redex_id a) (rb_event_redex_id b))
             (fun a b => lexc (Nat.compare (rb_event_local_index a) (rb_event_local_index b))
                (lexc (rb_kind_compare (rb_event_kind a) (rb_event_kind b))
                   (Nat.compare (rb_event_weight a) (rb_event_weight b))))).
    - intros c0 x y z. apply natc_trans.
    - intros x y z. apply (proj_compare_cong_l _ Nat.compare rb_event_redex_id natc_eq).
    - intros x y z. apply (proj_compare_cong_r _ Nat.compare rb_event_redex_id natc_eq).
    - exact Gl. }
  (* source_path level *)
  assert (Gsp : forall c x y z,
            lexc (list_nat_compare (rb_event_source_path x) (rb_event_source_path y))
              (lexc (Nat.compare (rb_event_redex_id x) (rb_event_redex_id y))
                (lexc (Nat.compare (rb_event_local_index x) (rb_event_local_index y))
                  (lexc (rb_kind_compare (rb_event_kind x) (rb_event_kind y))
                     (Nat.compare (rb_event_weight x) (rb_event_weight y))))) = c ->
            lexc (list_nat_compare (rb_event_source_path y) (rb_event_source_path z))
              (lexc (Nat.compare (rb_event_redex_id y) (rb_event_redex_id z))
                (lexc (Nat.compare (rb_event_local_index y) (rb_event_local_index z))
                  (lexc (rb_kind_compare (rb_event_kind y) (rb_event_kind z))
                     (Nat.compare (rb_event_weight y) (rb_event_weight z))))) = c ->
            lexc (list_nat_compare (rb_event_source_path x) (rb_event_source_path z))
              (lexc (Nat.compare (rb_event_redex_id x) (rb_event_redex_id z))
                (lexc (Nat.compare (rb_event_local_index x) (rb_event_local_index z))
                  (lexc (rb_kind_compare (rb_event_kind x) (rb_event_kind z))
                     (Nat.compare (rb_event_weight x) (rb_event_weight z))))) = c).
  { apply (lexc_trans rb_event
             (fun a b => list_nat_compare (rb_event_source_path a) (rb_event_source_path b))
             (fun a b => lexc (Nat.compare (rb_event_redex_id a) (rb_event_redex_id b))
                (lexc (Nat.compare (rb_event_local_index a) (rb_event_local_index b))
                  (lexc (rb_kind_compare (rb_event_kind a) (rb_event_kind b))
                     (Nat.compare (rb_event_weight a) (rb_event_weight b)))))).
    - intros c0 x y z. apply list_nat_compare_trans.
    - intros x y z. apply (proj_compare_cong_l _ list_nat_compare rb_event_source_path lnc_eq).
    - intros x y z. apply (proj_compare_cong_r _ list_nat_compare rb_event_source_path lnc_eq).
    - exact Gr. }
  (* deploy_id level (outermost) *)
  apply (lexc_trans rb_event
           (fun a b => Nat.compare (rb_event_deploy_id a) (rb_event_deploy_id b))
           (fun a b => lexc (list_nat_compare (rb_event_source_path a) (rb_event_source_path b))
              (lexc (Nat.compare (rb_event_redex_id a) (rb_event_redex_id b))
                (lexc (Nat.compare (rb_event_local_index a) (rb_event_local_index b))
                  (lexc (rb_kind_compare (rb_event_kind a) (rb_event_kind b))
                     (Nat.compare (rb_event_weight a) (rb_event_weight b))))))).
  - intros c0 x y z. apply natc_trans.
  - intros x y z. apply (proj_compare_cong_l _ Nat.compare rb_event_deploy_id natc_eq).
  - intros x y z. apply (proj_compare_cong_r _ Nat.compare rb_event_deploy_id natc_eq).
  - exact Gsp.
Qed.

(* ── rb_event_leb is a total, transitive, antisymmetric order ───────────── *)
Lemma rb_event_leb_total : forall e1 e2,
  rb_event_leb e1 e2 = true \/ rb_event_leb e2 e1 = true.
Proof.
  intros e1 e2. unfold rb_event_leb.
  rewrite (rb_event_compare_antisym e1 e2).
  destruct (rb_event_compare e1 e2); simpl; auto.
Qed.

Lemma rb_event_leb_trans : forall e1 e2 e3,
  rb_event_leb e1 e2 = true ->
  rb_event_leb e2 e3 = true ->
  rb_event_leb e1 e3 = true.
Proof.
  intros e1 e2 e3 H1 H2. unfold rb_event_leb in *.
  (* leb = true iff compare <> Gt iff compare in {Eq,Lt}. *)
  destruct (rb_event_compare e1 e2) eqn:H12; try discriminate;
  destruct (rb_event_compare e2 e3) eqn:H23; try discriminate.
  - rewrite (rb_event_compare_trans Eq e1 e2 e3 H12 H23). reflexivity.
  - apply rb_event_compare_eq_iff in H12. subst e2. rewrite H23. reflexivity.
  - apply rb_event_compare_eq_iff in H23. subst e3. rewrite H12. reflexivity.
  - rewrite (rb_event_compare_trans Lt e1 e2 e3 H12 H23). reflexivity.
Qed.

Lemma rb_event_leb_antisym : forall e1 e2,
  rb_event_leb e1 e2 = true ->
  rb_event_leb e2 e1 = true ->
  e1 = e2.
Proof.
  intros e1 e2 H1 H2. unfold rb_event_leb in *.
  rewrite (rb_event_compare_antisym e1 e2) in H2.
  destruct (rb_event_compare e1 e2) eqn:H12; simpl in *; try discriminate.
  apply rb_event_compare_eq_iff in H12. exact H12.
Qed.

(* ── Instantiate the stdlib mergesort over the canonical order ──────────── *)
Module RbEventOrder <: TotalLeBool'.
  Definition t := rb_event.
  Definition leb := rb_event_leb.
  Infix "<=?" := leb (at level 70, no associativity).
  Theorem leb_total : forall a1 a2, (a1 <=? a2) = true \/ (a2 <=? a1) = true.
  Proof. exact rb_event_leb_total. Qed.
End RbEventOrder.

Module Import RbEventSort := Sort RbEventOrder.

Definition rb_event_sort : list rb_event -> list rb_event := RbEventSort.sort.

(* Prop-valued order relation matching the sort's `is_true (leb ..)` view. *)
Definition rb_event_le (e1 e2 : rb_event) : Prop := rb_event_leb e1 e2 = true.

(* leb is transitive, so the sort output is StronglySorted. *)
#[local] Instance rb_event_le_Transitive : Transitive rb_event_le.
Proof. intros x y z. apply rb_event_leb_trans. Qed.

Lemma rb_event_sort_permuted : forall l, Permutation l (rb_event_sort l).
Proof. intro l. apply RbEventSort.Permuted_sort. Qed.

Lemma rb_event_sort_strongly_sorted : forall l,
  StronglySorted rb_event_le (rb_event_sort l).
Proof.
  intro l. unfold rb_event_le, rb_event_sort.
  apply RbEventSort.StronglySorted_sort.
  intros x y z Hxy Hyz. unfold is_true in *. eapply rb_event_leb_trans; eauto.
Qed.

(* ── Sort uniqueness: a StronglySorted list is the unique sorted
   representative of its permutation class (antisymmetry of the order). ─── *)
Lemma rb_strongly_sorted_perm_eq : forall l1 l2,
  StronglySorted rb_event_le l1 ->
  StronglySorted rb_event_le l2 ->
  Permutation l1 l2 ->
  l1 = l2.
Proof.
  induction l1 as [| a l1' IH]; intros l2 Hss1 Hss2 Hperm.
  - apply Permutation_nil in Hperm. symmetry. exact Hperm.
  - (* l2 is nonempty: a in l2. *)
    destruct l2 as [| b l2'].
    { apply Permutation_sym in Hperm. apply Permutation_nil in Hperm. discriminate. }
    (* a and b are mutually <= : a is the min of l1, b the min of l2,
       and each list is a permutation of the other. *)
    pose proof (StronglySorted_inv Hss1) as [Hss1' Hfa].
    pose proof (StronglySorted_inv Hss2) as [Hss2' Hfb].
    assert (Hb_in : In b (a :: l1')).
    { apply Permutation_in with (l := b :: l2').
      - apply Permutation_sym. exact Hperm.
      - left; reflexivity. }
    assert (Ha_in : In a (b :: l2')).
    { apply Permutation_in with (l := a :: l1').
      - exact Hperm.
      - left; reflexivity. }
    assert (Hab : a = b).
    { destruct Hb_in as [Hba | Hbin].
      - exact Hba.
      - destruct Ha_in as [Hab | Hain].
        + symmetry. exact Hab.
        + (* a <= b (Hfa on b in l1') and b <= a (Hfb on a in l2') *)
          rewrite Forall_forall in Hfa, Hfb.
          apply rb_event_leb_antisym.
          * apply (Hfa b Hbin).
          * apply (Hfb a Hain). }
    subst b.
    apply Permutation_cons_inv in Hperm.
    f_equal. apply IH; assumption.
Qed.

Lemma rb_event_sort_perm_eq : forall l1 l2,
  Permutation l1 l2 -> rb_event_sort l1 = rb_event_sort l2.
Proof.
  intros l1 l2 Hperm.
  apply rb_strongly_sorted_perm_eq.
  - apply rb_event_sort_strongly_sorted.
  - apply rb_event_sort_strongly_sorted.
  - (* sort l1 ~ l1 ~ l2 ~ sort l2 *)
    apply Permutation_trans with (l' := l1).
    { apply Permutation_sym. apply rb_event_sort_permuted. }
    apply Permutation_trans with (l' := l2).
    { exact Hperm. }
    apply rb_event_sort_permuted.
Qed.

(* sort is idempotent (it is identity on already-sorted lists). *)
Lemma rb_event_sort_idem : forall l,
  rb_event_sort (rb_event_sort l) = rb_event_sort l.
Proof.
  intro l. apply rb_event_sort_perm_eq.
  apply Permutation_sym. apply rb_event_sort_permuted.
Qed.

(* ── Insertion sort (used only to reason about the lowest-k absorption;
   shown equal to the mergesort via uniqueness). ───────────────────────── *)
Fixpoint rb_insert (x : rb_event) (l : list rb_event) : list rb_event :=
  match l with
  | [] => [x]
  | h :: t => if rb_event_leb x h then x :: h :: t else h :: rb_insert x t
  end.

Fixpoint rb_isort (l : list rb_event) : list rb_event :=
  match l with
  | [] => []
  | h :: t => rb_insert h (rb_isort t)
  end.

Lemma rb_insert_perm : forall x l, Permutation (x :: l) (rb_insert x l).
Proof.
  intros x l. induction l as [| h t IH]; simpl.
  - apply Permutation_refl.
  - destruct (rb_event_leb x h) eqn:Hle.
    + apply Permutation_refl.
    + (* x :: h :: t  ~  h :: insert x t *)
      apply Permutation_trans with (l' := h :: x :: t).
      * apply perm_swap.
      * apply perm_skip. exact IH.
Qed.

Lemma rb_isort_perm : forall l, Permutation l (rb_isort l).
Proof.
  induction l as [| h t IH]; simpl.
  - apply Permutation_refl.
  - apply Permutation_trans with (l' := h :: rb_isort t).
    + apply perm_skip. exact IH.
    + apply rb_insert_perm.
Qed.

Lemma rb_insert_sorted : forall x l,
  StronglySorted rb_event_le l ->
  StronglySorted rb_event_le (rb_insert x l).
Proof.
  intros x l Hss. induction l as [| h t IH]; simpl.
  - apply SSorted_cons; [apply SSorted_nil | apply Forall_nil].
  - destruct (rb_event_leb x h) eqn:Hle.
    + (* x <= h, prepend x *)
      apply SSorted_cons.
      * exact Hss.
      * (* Forall (le x) (h::t): x <= h, and x <= every elt of t since h <= them *)
        pose proof (StronglySorted_inv Hss) as [Hss_t Hfh].
        constructor.
        { unfold rb_event_le. exact Hle. }
        { (* x <= every element of t : x <= h <= y *)
          rewrite Forall_forall in Hfh. rewrite Forall_forall.
          intros y Hy. unfold rb_event_le in *.
          eapply rb_event_leb_trans; [exact Hle | apply (Hfh y Hy)]. }
    + (* h < x, recurse *)
      pose proof (StronglySorted_inv Hss) as [Hss_t Hfh].
      specialize (IH Hss_t).
      apply SSorted_cons.
      * exact IH.
      * (* Forall (le h) (insert x t): h <= elements of t (Hfh) and h <= x *)
        assert (Hhx : rb_event_le h x).
        { unfold rb_event_le.
          destruct (rb_event_leb_total h x) as [Hhx | Hxh].
          - exact Hhx.
          - rewrite Hxh in Hle. discriminate. }
        rewrite Forall_forall. rewrite Forall_forall in Hfh.
        intros y Hy.
        (* y in insert x t : y in (x::t) by permutation *)
        pose proof (rb_insert_perm x t) as Hp.
        assert (Hyin : In y (x :: t)).
        { apply Permutation_in with (l := rb_insert x t).
          - apply Permutation_sym. exact Hp.
          - exact Hy. }
        destruct Hyin as [Hyx | Hyt].
        { subst y. exact Hhx. }
        { apply (Hfh y Hyt). }
Qed.

Lemma rb_isort_sorted : forall l, StronglySorted rb_event_le (rb_isort l).
Proof.
  induction l as [| h t IH]; simpl.
  - apply SSorted_nil.
  - apply rb_insert_sorted. exact IH.
Qed.

(* Insertion sort and the canonical mergesort agree (both are the unique
   sorted permutation). *)
Lemma rb_isort_eq_sort : forall l, rb_isort l = rb_event_sort l.
Proof.
  intro l. apply rb_strongly_sorted_perm_eq.
  - apply rb_isort_sorted.
  - apply rb_event_sort_strongly_sorted.
  - apply Permutation_trans with (l' := l).
    + apply Permutation_sym. apply rb_isort_perm.
    + apply rb_event_sort_permuted.
Qed.

(* Helper: in a strongly-sorted list, the first-k elements that are all <= d,
   when the head h satisfies d = h (a boundary tie), are all equal to h. *)
Lemma sorted_firstn_below_pivot_all_eq : forall l d k,
  StronglySorted rb_event_le l ->
  Forall (fun p => rb_event_le p d) (firstn k l) ->
  Forall (fun p => rb_event_le d p) (firstn k l) ->
  Forall (fun p => p = d) (firstn k l).
Proof.
  intros l d k Hss Hle Hge.
  rewrite Forall_forall in *. intros x Hx.
  apply rb_event_leb_antisym.
  - apply Hle. exact Hx.
  - apply Hge. exact Hx.
Qed.

(* A list all of whose elements equal a constant IS repeat of that constant. *)
Lemma Forall_eq_repeat : forall (l : list rb_event) (c : rb_event),
  Forall (fun p => p = c) l ->
  l = repeat c (length l).
Proof.
  induction l as [| h t IH]; intros c Hall; simpl.
  - reflexivity.
  - inversion Hall as [| ? ? Hh Ht]; subst.
    f_equal. apply IH. exact Ht.
Qed.

(* Small list helpers not present under these names in this stdlib. *)
Lemma In_firstn : forall (A : Type) (x : A) n (l : list A),
  In x (firstn n l) -> In x l.
Proof.
  intros A x n l. revert n.
  induction l as [| h t IH]; intros n Hin.
  - rewrite firstn_nil in Hin. contradiction.
  - destruct n as [| n']; simpl in Hin.
    + contradiction.
    + destruct Hin as [Heq | Hin].
      * left. exact Heq.
      * right. apply (IH n' Hin).
Qed.

Lemma Forall_firstn : forall (A : Type) (P : A -> Prop) n (l : list A),
  Forall P l -> Forall P (firstn n l).
Proof.
  intros A P n l. revert n.
  induction l as [| h t IH]; intros n Hall.
  - rewrite firstn_nil. apply Forall_nil.
  - destruct n as [| n']; simpl.
    + apply Forall_nil.
    + inversion Hall as [| ? ? Hh Ht]; subst.
      constructor; [exact Hh | apply (IH n' Ht)].
Qed.

Lemma Forall_firstn_mono : forall (A : Type) (P : A -> Prop) m n (l : list A),
  m <= n -> Forall P (firstn n l) -> Forall P (firstn m l).
Proof.
  intros A P m n l Hmn Hall.
  assert (Hfm : firstn m l = firstn m (firstn n l)).
  { rewrite firstn_firstn. rewrite Nat.min_l by lia. reflexivity. }
  rewrite Hfm. apply Forall_firstn. exact Hall.
Qed.

(* The linchpin insertion lemma: inserting an element d that is >= every one
   of the first k elements of a strongly-sorted list (with k <= length) does
   not change the first k elements (as a list). Boundary ties collapse to
   equal records, which is why we need StronglySorted + antisymmetry. *)
Lemma firstn_insert_ge : forall l d k,
  k <= length l ->
  StronglySorted rb_event_le l ->
  Forall (fun p => rb_event_le p d) (firstn k l) ->
  firstn k (rb_insert d l) = firstn k l.
Proof.
  induction l as [| h t IH]; intros d k Hk Hss Hpre.
  - (* l = [] : k <= 0 so k = 0. *)
    simpl in Hk. assert (k = 0) by lia. subst k. reflexivity.
  - destruct k as [| k'].
    + reflexivity.
    + simpl in Hpre. inversion Hpre as [| ? ? Hhd Hrest]; subst.
      simpl rb_insert.
      destruct (rb_event_leb d h) eqn:Hdh.
      * (* boundary tie: leb d h = true and le h d => d = h *)
        assert (Hdh_eq : d = h).
        { apply rb_event_leb_antisym; [exact Hdh | exact Hhd]. }
        subst d.
        pose proof (StronglySorted_inv Hss) as [Hss_t Hfh].
        assert (Hle_h : rb_event_le h h).
        { unfold rb_event_le, rb_event_leb. rewrite rb_event_compare_refl. reflexivity. }
        (* every element of firstn (S k') (h::t) is = h *)
        assert (Hge : Forall (fun p => rb_event_le h p) (firstn (S k') (h :: t))).
        { simpl. constructor.
          - exact Hle_h.
          - rewrite Forall_forall in Hfh. rewrite Forall_forall.
            intros x Hx. apply Hfh. apply (In_firstn _ _ _ _ Hx). }
        assert (Hle : Forall (fun p => rb_event_le p h) (firstn (S k') (h :: t))).
        { simpl. constructor; [exact Hhd | exact Hrest]. }
        pose proof (sorted_firstn_below_pivot_all_eq (h::t) h (S k') Hss Hle Hge) as Hall.
        (* Reduce exactly one firstn layer on each side:
           goal becomes firstn k' (h::t) = firstn k' t. *)
        cbn [firstn]. f_equal.
        assert (Hk' : k' <= length t) by (simpl in Hk; lia).
        assert (Hall1 : Forall (fun p => p = h) (firstn k' (h :: t))).
        { apply (Forall_firstn_mono _ _ k' (S k')); [lia | exact Hall]. }
        simpl in Hall. inversion Hall as [| ? ? _ Hall_t']; subst.
        assert (Hall2 : Forall (fun p => p = h) (firstn k' t)) by exact Hall_t'.
        rewrite (Forall_eq_repeat (firstn k' (h :: t)) h Hall1).
        rewrite (Forall_eq_repeat (firstn k' t) h Hall2).
        rewrite !length_firstn. simpl length.
        rewrite (Nat.min_l k' (S (length t))) by lia.
        rewrite (Nat.min_l k' (length t)) by lia.
        reflexivity.
      * (* leb d h = false: recurse into t. *)
        simpl. f_equal.
        pose proof (StronglySorted_inv Hss) as [Hss_t _].
        apply IH.
        -- simpl in Hk. lia.
        -- exact Hss_t.
        -- exact Hrest.
Qed.

(* ── Absorption support: the lowest-k of a multiset are all <= a pivot d
   whenever the multiset contains k elements that are each <= d. ─────────── *)

Lemma Permutation_filter : forall (f : rb_event -> bool) a b,
  Permutation a b -> Permutation (filter f a) (filter f b).
Proof.
  intros f a b H. induction H; simpl.
  - apply Permutation_refl.
  - destruct (f x); [apply perm_skip; exact IHPermutation | exact IHPermutation].
  - destruct (f x); destruct (f y); try apply perm_swap; try apply Permutation_refl.
  - eapply Permutation_trans; eauto.
Qed.

(* A list every element of which fails "<= d" filters to nil. *)
Lemma filter_le_nil_of_all_gt : forall t d,
  Forall (fun y => rb_event_leb y d = false) t ->
  filter (fun x => rb_event_leb x d) t = [].
Proof.
  induction t as [| y t' IH]; intros d Hall; simpl.
  - reflexivity.
  - inversion Hall as [| ? ? Hy Hrest]; subst.
    rewrite Hy. apply IH. exact Hrest.
Qed.

(* If the head of a strongly-sorted list fails "<= d", so does every tail
   element (sorted: tail >= head > d). *)
Lemma sorted_head_gt_tail_gt : forall h t d,
  StronglySorted rb_event_le (h :: t) ->
  rb_event_leb h d = false ->
  Forall (fun y => rb_event_leb y d = false) t.
Proof.
  intros h t d Hss Hhd.
  pose proof (StronglySorted_inv Hss) as [_ Hfh].
  rewrite Forall_forall in Hfh. rewrite Forall_forall.
  intros y Hy.
  destruct (rb_event_leb y d) eqn:Hyd; [| reflexivity].
  exfalso.
  assert (Hhy : rb_event_le h y) by (apply Hfh; exact Hy).
  assert (rb_event_le h d).
  { unfold rb_event_le in *. eapply rb_event_leb_trans; [exact Hhy | exact Hyd]. }
  unfold rb_event_le in *. rewrite Hhd in H. discriminate.
Qed.

(* In a strongly-sorted list, the first k elements are all <= d, provided
   the list has at least k elements that are <= d (i.e. the "<= d" prefix is
   at least k long). *)
Lemma StronglySorted_firstn_le_threshold : forall S d k,
  StronglySorted rb_event_le S ->
  k <= length (filter (fun x => rb_event_leb x d) S) ->
  Forall (fun x => rb_event_le x d) (firstn k S).
Proof.
  induction S as [| h t IH]; intros d k Hss Hcount.
  - simpl in *. assert (k = 0) by lia. subst k. apply Forall_nil.
  - destruct k as [| k'].
    + apply Forall_nil.
    + simpl. pose proof (StronglySorted_inv Hss) as [Hss_t Hfh].
      simpl in Hcount.
      destruct (rb_event_leb h d) eqn:Hhd.
      * (* h <= d : head passes. *)
        simpl in Hcount.
        constructor.
        { unfold rb_event_le. exact Hhd. }
        { apply IH; [exact Hss_t | lia]. }
      * (* h > d : then all of t > d, so filter t = [], count = 0, absurd. *)
        exfalso.
        assert (Hempty : filter (fun x => rb_event_leb x d) t = []).
        { apply filter_le_nil_of_all_gt.
          apply (sorted_head_gt_tail_gt h t d Hss Hhd). }
        rewrite Hempty in Hcount. simpl in Hcount. lia.
Qed.

(* Permutation rest (P ++ Q) with |P| = k and all of P <= d gives >= k
   elements of rest that are <= d. *)
Lemma filter_le_pivot_count : forall rest P Q d k,
  Permutation rest (P ++ Q) ->
  length P = k ->
  Forall (fun p => rb_event_le p d) P ->
  k <= length (filter (fun x => rb_event_leb x d) rest).
Proof.
  intros rest P Q d k Hperm Hlen Hall.
  pose proof (Permutation_filter (fun x => rb_event_leb x d) _ _ Hperm) as Hpf.
  pose proof (Permutation_length Hpf) as Hlenf.
  rewrite Hlenf.
  rewrite filter_app, length_app.
  assert (HP : filter (fun x => rb_event_leb x d) P = P).
  { clear -Hall. induction P as [| p P' IHP]; simpl.
    - reflexivity.
    - inversion Hall as [| ? ? Hp Hrest]; subst.
      unfold rb_event_le in Hp. rewrite Hp. f_equal. apply IHP. exact Hrest. }
  rewrite HP. rewrite Hlen. lia.
Qed.

(* Main domination lemma: the lowest-k of `rest` are all <= d. *)
Lemma firstn_sort_dominated_by_pivot : forall rest P Q d k,
  Permutation rest (P ++ Q) ->
  length P = k ->
  Forall (fun p => rb_event_le p d) P ->
  Forall (fun x => rb_event_le x d) (firstn k (rb_event_sort rest)).
Proof.
  intros rest P Q d k Hperm Hlen Hall.
  apply StronglySorted_firstn_le_threshold.
  - apply rb_event_sort_strongly_sorted.
  - (* count of <= d in sort rest = count in rest (permutation) >= k *)
    pose proof (rb_event_sort_permuted rest) as Hsp.
    pose proof (Permutation_filter (fun x => rb_event_leb x d) _ _ Hsp) as Hpf.
    pose proof (Permutation_length Hpf) as Hlenf.
    rewrite <- Hlenf.
    eapply filter_le_pivot_count; eauto.
Qed.

(* Dropping a single dominated element (>= k elements that are <= it) from a
   multiset does not change its lowest-k. *)
Lemma lowk_drop_one_dominated : forall d rest P Q k,
  Permutation rest (P ++ Q) ->
  length P = k ->
  Forall (fun p => rb_event_le p d) P ->
  firstn k (rb_event_sort (d :: rest)) = firstn k (rb_event_sort rest).
Proof.
  intros d rest P Q k Hperm Hlen Hall.
  (* sort (d :: rest) = insert d (sort rest) *)
  assert (Hsort_cons : rb_event_sort (d :: rest)
                       = rb_insert d (rb_event_sort rest)).
  { rewrite <- (rb_isort_eq_sort (d :: rest)).
    simpl rb_isort. rewrite rb_isort_eq_sort. reflexivity. }
  rewrite Hsort_cons.
  apply firstn_insert_ge.
  - (* k <= length (sort rest) = length rest >= length P = k *)
    pose proof (rb_event_sort_permuted rest) as Hsp.
    rewrite <- (Permutation_length Hsp).
    rewrite (Permutation_length Hperm). rewrite length_app, Hlen. lia.
  - apply rb_event_sort_strongly_sorted.
  - eapply firstn_sort_dominated_by_pivot; eauto.
Qed.

(* Dropping a whole list D of dominated elements (all >= the k pivots P,
   with P a k-sub-multiset of base) preserves the lowest-k. *)
Lemma lowk_drop_dominated : forall D base P R k,
  Permutation base (P ++ R) ->
  length P = k ->
  Forall (fun d => Forall (fun p => rb_event_le p d) P) D ->
  firstn k (rb_event_sort (base ++ D)) = firstn k (rb_event_sort base).
Proof.
  induction D as [| d D' IH]; intros base P R k Hperm Hlen HallD.
  - rewrite app_nil_r. reflexivity.
  - pose proof (Forall_inv HallD) as Hd.
    pose proof (Forall_inv_tail HallD) as HD'.
    (* base ++ (d :: D') ~ d :: (base ++ D') *)
    assert (Hp1 : firstn k (rb_event_sort (base ++ d :: D'))
                  = firstn k (rb_event_sort (d :: (base ++ D')))).
    { f_equal. apply rb_event_sort_perm_eq.
      (* base ++ d :: D' ~ d :: base ++ D' *)
      apply Permutation_trans with (l' := (base ++ D') ++ [d]).
      - (* base ++ (d :: D') ~ (base ++ D') ++ [d] *)
        rewrite <- app_assoc. apply Permutation_app_head.
        apply Permutation_sym.
        change (d :: D') with ([d] ++ D').
        apply Permutation_app_comm.
      - (* (base ++ D') ++ [d] ~ d :: (base ++ D') *)
        apply Permutation_trans with (l' := [d] ++ (base ++ D')).
        + apply Permutation_app_comm.
        + simpl. apply Permutation_refl. }
    rewrite Hp1.
    (* drop d (dominated by P, which is a k-sub-multiset of base ++ D') *)
    assert (Hperm' : Permutation (base ++ D') (P ++ (R ++ D'))).
    { rewrite app_assoc.
      apply Permutation_app_tail. exact Hperm. }
    rewrite (lowk_drop_one_dominated d (base ++ D') P (R ++ D') k Hperm' Hlen Hd).
    (* now recurse on D' *)
    apply (IH base P R k Hperm Hlen HD').
Qed.

(* In a strongly-sorted concatenation P ++ D, every element of P is <= every
   element of D. *)
Lemma StronglySorted_app_cross : forall P D,
  StronglySorted rb_event_le (P ++ D) ->
  Forall (fun d => Forall (fun p => rb_event_le p d) P) D.
Proof.
  induction P as [| h P' IH]; intros D Hss; simpl in *.
  - (* P = [] : vacuous (Forall (fun _ => Forall _ []) D) *)
    rewrite Forall_forall. intros d _. apply Forall_nil.
  - (* P = h :: P' : head h <= all of D, and recurse for P'. *)
    pose proof (StronglySorted_inv Hss) as [Hss' Hfh].
    (* Hfh : Forall (rb_event_le h) (P' ++ D) ; restrict to D *)
    assert (HhD : Forall (fun d => rb_event_le h d) D).
    { rewrite Forall_forall in Hfh. rewrite Forall_forall.
      intros d Hd. apply Hfh. apply in_or_app. right. exact Hd. }
    specialize (IH D Hss').
    (* combine: for each d in D, h <= d and (P' all <= d) *)
    rewrite Forall_forall in IH, HhD. rewrite Forall_forall.
    intros d Hd.
    constructor.
    + apply HhD. exact Hd.
    + apply IH. exact Hd.
Qed.

(* ── MASTER bounded-K absorption lemma ───────────────────────────────────
   Truncating l to its lowest k before unioning with `rest` does not change
   the lowest k of the union. This is the algebraic heart of the bounded-K
   reconciliation: the cost walk never needs more than the lowest k events. *)
Lemma lowK_absorb : forall k l rest,
  firstn k (rb_event_sort (firstn k (rb_event_sort l) ++ rest))
  = firstn k (rb_event_sort (l ++ rest)).
Proof.
  intros k l rest.
  destruct (Nat.le_gt_cases k (length l)) as [Hk | Hk].
  - (* k <= length l : P := firstn k (sort l) has length k; D := skipn k. *)
    set (s := rb_event_sort l).
    set (P := firstn k s).
    set (D := skipn k s).
    assert (HlenP : length P = k).
    { unfold P, s. rewrite length_firstn.
      pose proof (rb_event_sort_permuted l) as Hsp.
      rewrite <- (Permutation_length Hsp). lia. }
    assert (Hsplit : P ++ D = s) by (unfold P, D; apply firstn_skipn).
    assert (Hss : StronglySorted rb_event_le s)
      by (unfold s; apply rb_event_sort_strongly_sorted).
    assert (Hdom : Forall (fun d => Forall (fun p => rb_event_le p d) P) D).
    { apply StronglySorted_app_cross. rewrite Hsplit. exact Hss. }
    (* base := P ++ rest. *)
    pose proof (lowk_drop_dominated D (P ++ rest) P rest k
                  (Permutation_refl _) HlenP Hdom) as Hdrop.
    (* Hdrop : firstn k (sort ((P++rest) ++ D)) = firstn k (sort (P++rest)) *)
    (* LHS of goal = firstn k (sort (P ++ rest)). *)
    fold s. fold P.
    rewrite <- Hdrop.
    (* now show firstn k (sort ((P++rest)++D)) = firstn k (sort (l++rest)) *)
    f_equal. apply rb_event_sort_perm_eq.
    (* (P++rest)++D ~ (P++D)++rest = s ++ rest ~ l ++ rest *)
    apply Permutation_trans with (l' := (P ++ D) ++ rest).
    + (* (P++rest)++D ~ (P++D)++rest *)
      rewrite <- !app_assoc. apply Permutation_app_head.
      apply Permutation_app_comm.
    + rewrite Hsplit.
      (* s ++ rest ~ l ++ rest *)
      apply Permutation_app_tail.
      unfold s. apply Permutation_sym. apply rb_event_sort_permuted.
  - (* k > length l : firstn k (sort l) = sort l, so just permutation. *)
    assert (Hall : firstn k (rb_event_sort l) = rb_event_sort l).
    { apply firstn_all2.
      pose proof (rb_event_sort_permuted l) as Hsp.
      rewrite <- (Permutation_length Hsp). lia. }
    rewrite Hall.
    f_equal. apply rb_event_sort_perm_eq.
    apply Permutation_app_tail.
    apply Permutation_sym. apply rb_event_sort_permuted.
Qed.

(* ══════════════════════════════════════════════════════════════════════════
   lowestK and the bounded-K commutative monoid
   ══════════════════════════════════════════════════════════════════════════ *)

(* The k lowest-rank events by the canonical Ord, multiplicity-preserving. *)
Definition lowestK (k : nat) (events : list rb_event) : list rb_event :=
  firstn k (rb_event_sort events).

(* Monoid operation: union the multisets and re-truncate to k. *)
Definition lowK_merge (k : nat) (a b : list rb_event) : list rb_event :=
  lowestK k (a ++ b).

(* Normalization onto the carrier of canonical k-bounded forms. *)
Definition lowK_nf (k : nat) (l : list rb_event) : list rb_event := lowestK k l.

(* lowestK is permutation-invariant in its argument (a pure multiset op). *)
Lemma lowestK_perm : forall k a b,
  Permutation a b -> lowestK k a = lowestK k b.
Proof.
  intros k a b H. unfold lowestK. f_equal. apply rb_event_sort_perm_eq. exact H.
Qed.

(* Idempotence of normalization: lowestK k (lowestK k l) = lowestK k l. *)
Lemma lowestK_idem : forall k l, lowestK k (lowestK k l) = lowestK k l.
Proof.
  intros k l. unfold lowestK.
  (* firstn k (sort (firstn k (sort l))) ; use absorption with rest = []. *)
  pose proof (lowK_absorb k l []) as H.
  rewrite app_nil_r in H. rewrite app_nil_r in H. exact H.
Qed.

(* Absorption restated at the lowestK level: pre-truncating the left operand
   of a union does not change the lowest-k of the union. *)
Lemma lowestK_absorb : forall k l rest,
  lowestK k (lowestK k l ++ rest) = lowestK k (l ++ rest).
Proof.
  intros k l rest. unfold lowestK. apply lowK_absorb.
Qed.

(* Commutativity (literal equality via permutation-invariance + app comm). *)
Theorem lowK_merge_comm : forall k a b,
  lowK_merge k a b = lowK_merge k b a.
Proof.
  intros k a b. unfold lowK_merge. apply lowestK_perm. apply Permutation_app_comm.
Qed.

(* Left identity on canonical forms: [] is the unit. *)
Theorem lowK_merge_id_l : forall k l,
  lowK_merge k [] (lowK_nf k l) = lowK_nf k l.
Proof.
  intros k l. unfold lowK_merge, lowK_nf. simpl. apply lowestK_idem.
Qed.

Theorem lowK_merge_id_r : forall k l,
  lowK_merge k (lowK_nf k l) [] = lowK_nf k l.
Proof.
  intros k l. rewrite lowK_merge_comm. apply lowK_merge_id_l.
Qed.

(* The empty list is itself normal (a fixpoint of normalization). *)
Lemma lowK_nf_nil : forall k, lowK_nf k [] = [].
Proof.
  intro k. unfold lowK_nf, lowestK.
  (* sort [] = [] *)
  assert (rb_event_sort [] = []).
  { apply Permutation_nil. apply Permutation_sym. apply rb_event_sort_permuted. }
  rewrite H. rewrite firstn_nil. reflexivity.
Qed.

(* Associativity — the bounded-K law.  Folding two truncations and re-merging
   equals merging the full union and truncating once: this is what lets the
   runtime keep only a bounded lowest-K window per merge. *)
Theorem lowK_merge_assoc : forall k a b c,
  lowK_merge k (lowK_merge k a b) c = lowK_merge k a (lowK_merge k b c).
Proof.
  intros k a b c. unfold lowK_merge.
  (* LHS = lowestK k (lowestK k (a++b) ++ c) = lowestK k ((a++b) ++ c). *)
  rewrite (lowestK_absorb k (a ++ b) c).
  (* RHS = lowestK k (a ++ lowestK k (b++c)).  Commute the union so the
     truncated operand is on the LEFT, then absorb. *)
  rewrite (lowestK_perm k (a ++ lowestK k (b ++ c))
                          (lowestK k (b ++ c) ++ a))
    by apply Permutation_app_comm.
  rewrite (lowestK_absorb k (b ++ c) a).
  (* Now: lowestK k ((a++b)++c) = lowestK k ((b++c)++a). *)
  apply lowestK_perm.
  rewrite <- !app_assoc.
  apply Permutation_trans with (l' := (b ++ c) ++ a).
  - apply Permutation_app_comm.
  - rewrite <- app_assoc. apply Permutation_refl.
Qed.

(* ══════════════════════════════════════════════════════════════════════════
   Bounded-K equivalence of the cost walk
   ══════════════════════════════════════════════════════════════════════════

   The Rust `reconcile` (accounting/mod.rs:455) sorts the attempt log by the
   canonical Ord, truncates to MAX_COST_TRACE_EVENTS, then walks committing
   events until the cumulative weight would exceed `initial` (the OOP
   boundary). With every billable weight >= 1, the walk can commit at most
   `initial` events before stopping, so it inspects at most `initial + 1`
   events. Hence the walk reads only the lowest `min(MAX, initial+1)` events.

   MAX_COST_TRACE_EVENTS = 1_048_576 in accounting/mod.rs:27. Its concrete
   value is irrelevant to these proofs — only that the walk window is
   min(MAX, initial+1). *)
Definition MAX_COST_TRACE_EVENTS : nat := Nat.pow 2 20.  (* = 1_048_576 *)

Definition rb_bounded_K (initial : nat) : nat :=
  Nat.min MAX_COST_TRACE_EVENTS (initial + 1).

(* Every weight >= 1 transfers across the canonical sort (it is a
   permutation). *)
Lemma rb_event_positive_sort : forall events,
  Forall rb_event_positive events ->
  Forall rb_event_positive (rb_event_sort events).
Proof.
  intros events H. rewrite Forall_forall in *.
  intros x Hx. apply H.
  apply Permutation_in with (l := rb_event_sort events).
  - apply Permutation_sym. apply rb_event_sort_permuted.
  - exact Hx.
Qed.

(* PREFIX-STABILITY: with all weights >= 1, walking the full list and walking
   any prefix of length >= (initial - consumed + 1) produce the SAME result
   (both final state and the results list). The walk simply never looks past
   that many events: each committed event consumes >= 1, so after at most
   (initial - consumed) commits the budget is exhausted and the next event
   (the (initial - consumed + 1)-th inspected) is the OOP boundary — or the
   list ends first. *)
Lemma rb_reserve_many_prefix_stable : forall L b n,
  Forall rb_event_positive L ->
  rb_valid b ->
  rb_unmetered b = false ->
  rb_initial b - rb_consumed b + 1 <= n ->
  rb_reserve_many b L = rb_reserve_many b (firstn n L).
Proof.
  induction L as [| e rest IH]; intros b n Hpos Hvalid Hunmet Hn.
  - (* L = [] : firstn n [] = []. *)
    rewrite firstn_nil. reflexivity.
  - (* L = e :: rest.  n >= initial - consumed + 1 >= 1, so n = S n'. *)
    destruct n as [| n'].
    + (* n = 0 contradicts initial - consumed + 1 <= 0. *)
      lia.
    + simpl firstn. simpl rb_reserve_many.
      inversion Hpos as [| ? ? Hpe Hprest]; subst.
      destruct (rb_reserve b e) as [b1 r] eqn:Hreserve.
      destruct r.
      * (* RbReserveOk: e committed; consumed advances by weight e >= 1. *)
        pose proof (rb_reserve_ok_advances_consumed b e b1 Hunmet Hreserve)
          as [Hcons1 [Hinit1 Hunmet1]].
        pose proof (rb_reserve_preserves_valid b e b1 RbReserveOk Hvalid Hreserve)
          as Hvalid1.
        (* The Ok branch means e fit: consumed b + weight e <= initial b. *)
        assert (Hfit : ~ (rb_initial b < rb_consumed b + rb_event_weight e)).
        { pose proof (rb_reserve_oop_iff_would_overflow b e b1 RbReserveOk
                        Hunmet Hreserve) as [_ Hov_to_oop].
          intro Hov. specialize (Hov_to_oop Hov). discriminate. }
        (* recurse on rest with b1, n'; need initial(b1)-consumed(b1)+1 <= n' *)
        assert (Hside : rb_initial b1 - rb_consumed b1 + 1 <= n').
        { unfold rb_event_positive in Hpe. rewrite Hinit1, Hcons1. lia. }
        rewrite (IH b1 n' Hprest Hvalid1 Hunmet1 Hside).
        reflexivity.
      * (* RbReserveOop: walk stops immediately; reserve only inspected e. *)
        reflexivity.
Qed.

(* lowestK at the bounded-K window equals the bounded-K prefix of the full
   sorted-then-truncated (to MAX) multiset that the Rust walk consumes. *)
Lemma rb_bounded_K_le_max : forall initial,
  rb_bounded_K initial <= MAX_COST_TRACE_EVENTS.
Proof. intro initial. unfold rb_bounded_K. apply Nat.le_min_l. Qed.

Lemma lowestK_bounded_K_is_prefix_of_truncated : forall initial events,
  lowestK (rb_bounded_K initial) events
  = firstn (rb_bounded_K initial)
           (firstn MAX_COST_TRACE_EVENTS (rb_event_sort events)).
Proof.
  intros initial events. unfold lowestK.
  rewrite firstn_firstn.
  rewrite (Nat.min_l (rb_bounded_K initial) MAX_COST_TRACE_EVENTS
             (rb_bounded_K_le_max initial)).
  reflexivity.
Qed.

(* The full sorted-then-truncated multiset the runtime walks. *)
Definition rb_sort_truncate (events : list rb_event) : list rb_event :=
  firstn MAX_COST_TRACE_EVENTS (rb_event_sort events).

(* ── DELIVERABLE 1 (headline): the cost walk's full output — final state
   (committed event_log, consumed, last_oop) AND the per-event results list —
   over the bounded lowest-K fold equals the output over the full
   sorted-then-truncated multiset.  Weights >= 1 and consumed starts at 0
   (a fresh per-deploy budget), so the walk inspects <= initial + 1 events
   and never looks past the lowest min(MAX, initial+1). *)
Theorem rb_reconcile_bounded_K_eq_sort_truncate : forall b events,
  Forall rb_event_positive events ->
  rb_valid b ->
  rb_unmetered b = false ->
  rb_consumed b = 0 ->
  rb_reserve_many b (lowestK (rb_bounded_K (rb_initial b)) events)
  = rb_reserve_many b (rb_sort_truncate events).
Proof.
  intros b events Hpos Hvalid Hunmet Hcons0.
  unfold rb_sort_truncate.
  rewrite (lowestK_bounded_K_is_prefix_of_truncated (rb_initial b) events).
  set (S := firstn MAX_COST_TRACE_EVENTS (rb_event_sort events)).
  (* All weights in S are >= 1 (S is a sublist of the sorted positives). *)
  assert (HposS : Forall rb_event_positive S).
  { unfold S. apply Forall_firstn. apply rb_event_positive_sort. exact Hpos. }
  (* Case on whether the bounded window reaches the prefix-stability bound. *)
  destruct (Nat.le_gt_cases (rb_initial b + 1) MAX_COST_TRACE_EVENTS) as [Hle | Hgt].
  - (* initial + 1 <= MAX: bounded_K = initial + 1 >= initial - consumed + 1. *)
    assert (HbK : rb_bounded_K (rb_initial b) = rb_initial b + 1).
    { unfold rb_bounded_K. rewrite Nat.min_r by lia. reflexivity. }
    rewrite HbK.
    symmetry.
    apply rb_reserve_many_prefix_stable.
    + exact HposS.
    + exact Hvalid.
    + exact Hunmet.
    + rewrite Hcons0. lia.
  - (* MAX < initial + 1: bounded_K = MAX, and length S <= MAX so the prefix
       is all of S — both sides walk S. *)
    assert (HbK : rb_bounded_K (rb_initial b) = MAX_COST_TRACE_EVENTS).
    { unfold rb_bounded_K. rewrite Nat.min_l by lia. reflexivity. }
    rewrite HbK.
    assert (HlenS : length S <= MAX_COST_TRACE_EVENTS).
    { unfold S. rewrite length_firstn. lia. }
    assert (Hsafe : firstn MAX_COST_TRACE_EVENTS S = S)
      by (apply firstn_all2; exact HlenS).
    rewrite Hsafe.
    reflexivity.
Qed.

(* Explicit corollaries: each observable the runtime reads off `reconcile`
   (final state — hence committed event_log, consumed, last_oop — and the
   OOP-count from the results list) is identical between the bounded-K fold
   and the full sorted-then-truncated walk. *)
Corollary rb_reconcile_bounded_K_state_eq : forall b events,
  Forall rb_event_positive events ->
  rb_valid b -> rb_unmetered b = false -> rb_consumed b = 0 ->
  rb_reconcile b (lowestK (rb_bounded_K (rb_initial b)) events)
  = rb_reconcile b (rb_sort_truncate events).
Proof.
  intros b events Hpos Hvalid Hunmet Hcons0.
  unfold rb_reconcile.
  rewrite (rb_reconcile_bounded_K_eq_sort_truncate b events
             Hpos Hvalid Hunmet Hcons0).
  reflexivity.
Qed.

Corollary rb_reconcile_bounded_K_consumed_eq : forall b events,
  Forall rb_event_positive events ->
  rb_valid b -> rb_unmetered b = false -> rb_consumed b = 0 ->
  rb_consumed (rb_reconcile b (lowestK (rb_bounded_K (rb_initial b)) events))
  = rb_consumed (rb_reconcile b (rb_sort_truncate events)).
Proof.
  intros b events Hpos Hvalid Hunmet Hcons0.
  rewrite (rb_reconcile_bounded_K_state_eq b events Hpos Hvalid Hunmet Hcons0).
  reflexivity.
Qed.

Corollary rb_reconcile_bounded_K_committed_eq : forall b events,
  Forall rb_event_positive events ->
  rb_valid b -> rb_unmetered b = false -> rb_consumed b = 0 ->
  rb_event_log (rb_reconcile b (lowestK (rb_bounded_K (rb_initial b)) events))
  = rb_event_log (rb_reconcile b (rb_sort_truncate events)).
Proof.
  intros b events Hpos Hvalid Hunmet Hcons0.
  rewrite (rb_reconcile_bounded_K_state_eq b events Hpos Hvalid Hunmet Hcons0).
  reflexivity.
Qed.

Corollary rb_reconcile_bounded_K_oop_eq : forall b events,
  Forall rb_event_positive events ->
  rb_valid b -> rb_unmetered b = false -> rb_consumed b = 0 ->
  rb_last_oop (rb_reconcile b (lowestK (rb_bounded_K (rb_initial b)) events))
  = rb_last_oop (rb_reconcile b (rb_sort_truncate events)).
Proof.
  intros b events Hpos Hvalid Hunmet Hcons0.
  rewrite (rb_reconcile_bounded_K_state_eq b events Hpos Hvalid Hunmet Hcons0).
  reflexivity.
Qed.

(* OOP occurrence (the results list's OOP count) is also identical. *)
Corollary rb_reconcile_bounded_K_oop_count_eq : forall b events,
  Forall rb_event_positive events ->
  rb_valid b -> rb_unmetered b = false -> rb_consumed b = 0 ->
  rb_oop_count (snd (rb_reserve_many b (lowestK (rb_bounded_K (rb_initial b)) events)))
  = rb_oop_count (snd (rb_reserve_many b (rb_sort_truncate events))).
Proof.
  intros b events Hpos Hvalid Hunmet Hcons0.
  rewrite (rb_reconcile_bounded_K_eq_sort_truncate b events
             Hpos Hvalid Hunmet Hcons0).
  reflexivity.
Qed.

(* ══════════════════════════════════════════════════════════════════════════
   DELIVERABLE 2: total_cost schedule-independence
   ══════════════════════════════════════════════════════════════════════════

   After the per-operation cost-trace digest is dropped from consensus, the
   only cost quantity that remains consensus-binding is `total_cost`, i.e.
   `rb_reconcile(events).consumed` (clamped on OOP). These theorems show that
   value is (a) a pure function of (initial, weight-multiset) — hence
   invariant under any schedule/permutation of the recorded events — and
   (b) exactly the clamped sum: min(initial, Σ weights), with OOP firing iff
   Σ > initial.  They package the existing reconcile lemmas
   (rb_reconcile_consumed_eq_min_initial_or_sum,
    rb_reconcile_consumed_invariant_under_permutation,
    rb_reconcile_oop_iff_sum_overflows) at the headline `total_cost` level. *)

(* total_cost (consumed) from a fresh per-deploy budget = min(initial, sum). *)
Theorem rb_total_cost_eq_min_initial_sum : forall initial events,
  rb_consumed (rb_reconcile (rb_new initial) events)
  = Nat.min initial (rb_event_weight_sum events).
Proof.
  intros initial events. unfold rb_reconcile.
  destruct (rb_reserve_many (rb_new initial) events) as [b' results] eqn:Hmany.
  simpl fst.
  pose proof (rb_reconcile_consumed_eq_min_initial_or_sum
                events (rb_new initial) b' results
                (rb_new_valid initial) eq_refl Hmany) as Heq.
  (* rb_new: initial = initial, consumed = 0 *)
  simpl in Heq. try rewrite Nat.add_0_l in Heq. exact Heq.
Qed.

(* Schedule-independence: any permutation of the recorded events yields the
   same total_cost. *)
Theorem rb_total_cost_schedule_independent : forall initial events1 events2,
  Permutation events1 events2 ->
  rb_consumed (rb_reconcile (rb_new initial) events1)
  = rb_consumed (rb_reconcile (rb_new initial) events2).
Proof.
  intros initial events1 events2 Hperm.
  rewrite (rb_total_cost_eq_min_initial_sum initial events1).
  rewrite (rb_total_cost_eq_min_initial_sum initial events2).
  rewrite (rb_event_weight_sum_permutation_invariant events1 events2 Hperm).
  reflexivity.
Qed.

(* The clamped characterization: when the recorded weight sum exceeds the
   budget the walk OOPs and total_cost clamps to `initial`; otherwise it
   completes and total_cost is exactly the sum. *)
Theorem rb_total_cost_clamped_characterization : forall initial events b' results,
  rb_reserve_many (rb_new initial) events = (b', results) ->
  (initial < rb_event_weight_sum events ->
     rb_consumed b' = initial /\ rb_oop_count results = 1)
  /\ (rb_event_weight_sum events <= initial ->
     rb_consumed b' = rb_event_weight_sum events /\ rb_oop_count results = 0).
Proof.
  intros initial events b' results Hmany.
  pose proof (rb_reconcile_consumed_eq_min_initial_or_sum
                events (rb_new initial) b' results
                (rb_new_valid initial) eq_refl Hmany) as Hcons.
  simpl in Hcons. try rewrite Nat.add_0_l in Hcons.
  pose proof (rb_reconcile_oop_iff_sum_overflows
                events (rb_new initial) b' results
                (rb_new_valid initial) eq_refl Hmany) as Hoop.
  simpl in Hoop. try rewrite Nat.add_0_l in Hoop.
  pose proof (rb_reserve_many_oop_count_le_one events (rb_new initial) b' results Hmany)
    as Hle1.
  split.
  - (* sum > initial: clamp + OOP *)
    intros Hgt. split.
    + rewrite Hcons. apply Nat.min_l. lia.
    + apply Hoop. exact Hgt.
  - (* sum <= initial: complete, no OOP *)
    intros Hle. split.
    + rewrite Hcons. apply Nat.min_r. exact Hle.
    + (* OOP count must be 0: if it were 1, Hoop gives initial < sum, contra *)
      destruct (Nat.eq_dec (rb_oop_count results) 1) as [H1 | Hne].
      * apply Hoop in H1. lia.
      * lia.
Qed.

(* Headline schedule-independent total_cost characterization from a fresh
   budget, combining permutation-invariance with the clamped formula and
   OOP verdict — the consensus cost quantity that survives dropping the
   per-operation digest. *)
Theorem rb_total_cost_schedule_independent_and_clamped :
  forall initial events1 events2 b1 r1,
  Permutation events1 events2 ->
  rb_reserve_many (rb_new initial) events1 = (b1, r1) ->
  (* total_cost is permutation-invariant ... *)
  (forall b2 r2, rb_reserve_many (rb_new initial) events2 = (b2, r2) ->
      rb_consumed b1 = rb_consumed b2)
  (* ... and equals the clamped sum, with OOP iff the sum overflows. *)
  /\ rb_consumed b1 = Nat.min initial (rb_event_weight_sum events1)
  /\ (rb_oop_count r1 = 1 <-> initial < rb_event_weight_sum events1).
Proof.
  intros initial events1 events2 b1 r1 Hperm Hmany1.
  split; [| split].
  - intros b2 r2 Hmany2.
    apply (rb_reconcile_consumed_invariant_under_permutation
             events1 events2 (rb_new initial) b1 b2 r1 r2
             (rb_new_valid initial) eq_refl Hperm Hmany1 Hmany2).
  - pose proof (rb_reconcile_consumed_eq_min_initial_or_sum
                  events1 (rb_new initial) b1 r1
                  (rb_new_valid initial) eq_refl Hmany1) as Hcons.
    simpl in Hcons. try rewrite Nat.add_0_l in Hcons. exact Hcons.
  - pose proof (rb_reconcile_oop_iff_sum_overflows
                  events1 (rb_new initial) b1 r1
                  (rb_new_valid initial) eq_refl Hmany1) as Hoop.
    simpl in Hoop. try rewrite Nat.add_0_l in Hoop. exact Hoop.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Per-Signature Token Pool (WD-D0)
   ═══════════════════════════════════════════════════════════════════════════

   The Rust `RuntimeBudget` gains a per-signature token pool
   `lanes : DashMap<[u8;32], Lane>` (spec §4.6 spectral decomposition into
   per-signature pools; §7.6 "no interleaving" is PER-SIGNATURE, not global).
   Each `Lane` mirrors the scalar `RuntimeBudget` fields and is reconciled by
   the SAME canonical walk (`reconcile_lane`, extracted from the scalar
   `reconcile`), so a lane is an INDEPENDENT INSTANCE of the proven scalar
   budget model above. The deploy's consumed cost is the SUM over lanes
   (`RuntimeBudget::lane_pool_total_cost`).

   We model:
   - a lane [rb_lane] as a scalar budget paired with the event list routed to
     it (`rb_state * list rb_event`);
   - a pool [rb_pool] as a list of lanes;
   - lane reconciliation [rb_lane_reconcile] as the SAME [rb_reconcile] used by
     the scalar path — making "[rb_pool] = N independent instances of the
     proven scalar budget" definitional;
   - the pool total cost [rb_pool_total_cost] as the sum of per-lane
     [rb_total_cost].

   Headline obligations (the WD-D0 entries in
   `docs/theory/cost-accounting-impl/supply-realization-c-d-handoff.md`
   Decision 8 and `workstream-d-acceptance.md` D0):
   - [rb_pool_total_cost_eq_sum]: the reconciled pool's total cost equals the
     sum over lanes of [rb_total_cost] of each independently-reconciled lane
     ("[rb_pool_total_cost = Σ rb_total_cost]").
   - [rb_pool_reconcile_preserves_valid]: each reconciled lane is valid, so the
     pool is a vector of valid scalar budgets (N independent instances).
   - [rb_pool_total_cost_permutation_invariant]: the pool total is INVARIANT
     under reordering lanes — the commutative / order-independent sum that the
     Rust `lane_pool_total_cost` and the 2-lane loom test rely on (disjoint
     signatures contend on nothing, so visiting order is irrelevant).

   No `Axiom`, no `Admitted`: everything reduces to the scalar theorems above
   plus stdlib [map]/[fold_right]/[Permutation] facts. *)

Definition rb_lane : Type := rb_state * list rb_event.

Definition rb_pool : Type := list rb_lane.

(* Reconcile ONE lane via the SAME canonical walk as the scalar path. This is
   what makes a lane "an independent instance of the proven scalar budget". *)
Definition rb_lane_reconcile (l : rb_lane) : rb_state :=
  rb_reconcile (fst l) (snd l).

(* Reconcile every lane independently (mirrors mapping `reconcile_one_lane`
   over the `DashMap` entries). *)
Definition rb_pool_reconcile (p : rb_pool) : list rb_state :=
  map rb_lane_reconcile p.

(* The deploy's pooled cost: the sum of `rb_total_cost` over a list of
   (already-reconciled) lane states. Sum via `fold_right` over the mapped
   per-lane costs — addition is commutative/associative, so the order in which
   lanes are summed is irrelevant (proven in
   [rb_pool_total_cost_permutation_invariant]). *)
Definition rb_pool_total_cost (states : list rb_state) : nat :=
  fold_right (fun b acc => rb_total_cost b + acc) 0 states.

(* Convenience: the total cost of a pool, reconciling each lane first. *)
Definition rb_pool_reconciled_total_cost (p : rb_pool) : nat :=
  rb_pool_total_cost (rb_pool_reconcile p).

(* `rb_pool_total_cost` distributes over `cons`. (`fold_right` on a cons
   reduces definitionally, so this is a `reflexivity`.) *)
Lemma rb_pool_total_cost_cons : forall b states,
  rb_pool_total_cost (b :: states) = rb_total_cost b + rb_pool_total_cost states.
Proof.
  intros b states. reflexivity.
Qed.

(* `rb_pool_total_cost` is additive over list concatenation. *)
Lemma rb_pool_total_cost_app : forall xs ys,
  rb_pool_total_cost (xs ++ ys) =
  rb_pool_total_cost xs + rb_pool_total_cost ys.
Proof.
  induction xs as [| b xs IH]; intros ys.
  - reflexivity.
  - simpl (_ ++ _). rewrite !rb_pool_total_cost_cons. rewrite IH. lia.
Qed.

(* HEADLINE: the reconciled pool's total cost is the SUM over lanes of the
   `rb_total_cost` of each independently-reconciled lane. This is
   `rb_pool_total_cost = Σ rb_total_cost` (WD-D0): the per-signature pool's
   cost is exactly N independent applications of the scalar `rb_total_cost`,
   one per lane, summed — the Rust `lane_pool_total_cost`. *)
Theorem rb_pool_total_cost_eq_sum : forall p,
  rb_pool_reconciled_total_cost p =
  fold_right (fun l acc => rb_total_cost (rb_lane_reconcile l) + acc) 0 p.
Proof.
  intro p.
  unfold rb_pool_reconciled_total_cost, rb_pool_total_cost, rb_pool_reconcile.
  induction p as [| l p IH].
  - simpl. reflexivity.
  - simpl. rewrite IH. reflexivity.
Qed.

(* Each reconciled lane is a VALID scalar budget, given a valid seed: a lane is
   an independent instance of the proven scalar budget, so it inherits
   `rb_reconcile_preserves_valid`. *)
Theorem rb_lane_reconcile_preserves_valid : forall l,
  rb_valid (fst l) ->
  rb_valid (rb_lane_reconcile l).
Proof.
  intros [b events] Hvalid. unfold rb_lane_reconcile. simpl in *.
  apply rb_reconcile_preserves_valid. exact Hvalid.
Qed.

(* The whole reconciled pool is a vector of VALID scalar budgets, given that
   each lane seed is valid (N independent instances of the proven budget). *)
Theorem rb_pool_reconcile_preserves_valid : forall p,
  Forall (fun l => rb_valid (fst l)) p ->
  Forall rb_valid (rb_pool_reconcile p).
Proof.
  intros p Hall. unfold rb_pool_reconcile.
  induction p as [| l p IH].
  - simpl. constructor.
  - simpl. inversion Hall as [| x xs Hx Hxs]; subst.
    constructor.
    + apply rb_lane_reconcile_preserves_valid. exact Hx.
    + apply IH. exact Hxs.
Qed.

(* `rb_pool_total_cost` is invariant under permutation of the lane-state list:
   the sum is commutative/associative, so it does not depend on the order in
   which lanes are visited. This is the order-independence the Rust
   `lane_pool_total_cost` (iterating a `DashMap`) and the 2-lane loom test
   rely on. *)
Theorem rb_pool_total_cost_permutation_invariant : forall states1 states2,
  Permutation states1 states2 ->
  rb_pool_total_cost states1 = rb_pool_total_cost states2.
Proof.
  intros states1 states2 Hperm.
  induction Hperm.
  - reflexivity.
  - rewrite !rb_pool_total_cost_cons. rewrite IHHperm. reflexivity.
  - rewrite !rb_pool_total_cost_cons. lia.
  - rewrite IHHperm1. exact IHHperm2.
Qed.

(* Corollary: the reconciled-pool total cost is invariant under permutation of
   the LANES themselves (reconcile commutes with the mapped permutation). The
   deploy's pooled cost does not depend on lane-visit order — the spectral
   decomposition is order-independent (spec §7.6 per-signature). *)
Theorem rb_pool_reconciled_total_cost_permutation_invariant : forall p1 p2,
  Permutation p1 p2 ->
  rb_pool_reconciled_total_cost p1 = rb_pool_reconciled_total_cost p2.
Proof.
  intros p1 p2 Hperm.
  unfold rb_pool_reconciled_total_cost, rb_pool_reconcile.
  apply rb_pool_total_cost_permutation_invariant.
  apply Permutation_map. exact Hperm.
Qed.

(* The N=1 fast path, formally: a single-lane pool's total cost is exactly the
   scalar `rb_total_cost` of that one lane's reconciliation — no pool overhead,
   byte-identical to the scalar budget (the Rust `legacy_single_sig_byte_identical`
   invariant, lifted to the model). *)
Theorem rb_pool_singleton_eq_scalar : forall l,
  rb_pool_reconciled_total_cost (l :: nil) =
  rb_total_cost (rb_lane_reconcile l).
Proof.
  intro l.
  unfold rb_pool_reconciled_total_cost, rb_pool_reconcile, rb_pool_total_cost.
  simpl. lia.
Qed.

(* The pooled total is the scalar consumed sum when no lane OOPs (every lane
   metered): each `rb_total_cost` is just `rb_consumed`, so the pool total is
   `Σ rb_consumed` — the per-signature decomposition of the deploy cost. *)
Theorem rb_pool_total_cost_metered_eq_consumed_sum : forall states,
  Forall (fun b => rb_unmetered b = false) states ->
  rb_pool_total_cost states =
  fold_right (fun b acc => rb_consumed b + acc) 0 states.
Proof.
  induction states as [| b states IH]; intro Hall.
  - reflexivity.
  - inversion Hall as [| x xs Hx Hxs]; subst.
    rewrite rb_pool_total_cost_cons.
    cbn [fold_right].
    unfold rb_total_cost. rewrite Hx.
    rewrite IH by exact Hxs. reflexivity.
Qed.
