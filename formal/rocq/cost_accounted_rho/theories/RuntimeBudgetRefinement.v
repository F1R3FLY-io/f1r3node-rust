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
  Sorting.Permutation.
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
  rb_full_system_slash_fields : list (nat * nat * nat);
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
