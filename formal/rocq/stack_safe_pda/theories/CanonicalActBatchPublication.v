From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import RPB3Admission.

Import ListNotations CanonicalKeyProtocol IndexedWindowTransport CheckedACTLineTransport.
Set Implicit Arguments.

Module CanonicalActBatchPublication.

Inductive terminal := Active | Done | Failed (error : nat).
Inductive epm_mode := Empty | SetMode | MapMode.

Record preserved_authority := PreservedAuthority {
  terminal_state : terminal;
  mode : epm_mode;
  empty_state : nat;
  empty_position : provenance;
  value_reader : list nat;
  value_table_identity : nat;
  value_table_start : nat;
  value_count : nat;
  value_count_position : provenance;
  endpoint_count : nat;
  value_records : nat;
  prefix_bytes_read : nat;
  prefix_bytes_traversed : nat;
  terminal_checks : nat;
  awaiting_child : bool
}.

Record authority := Authority {
  act_cursor : cursor;
  act_active : bool;
  retained : preserved_authority;
  topology_events : nat;
  frame_last_source : option provenance
}.

Definition authority_eq_dec (x y : authority) : {x = y} + {x <> y}.
Proof.
  decide equality; try apply cursor_eq_dec;
    repeat decide equality; apply Nat.eq_dec.
Defined.

Record request := Request {
  origin : authority;
  batch_direction : direction;
  batch_width : nat
}.

Definition expanded request :=
  match batch_direction request with
  | Forward => 2 * batch_width request
  | Unwind => batch_width request
  end.

Definition eligible current :=
  act_active current && negb (awaiting_child (retained current)) &&
  match terminal_state (retained current), mode (retained current) with
  | Active, SetMode | Active, MapMode => true
  | _, _ => false
  end.

Definition outer_guard limit current request :=
  if authority_eq_dec current (origin request) then
    eligible current && (expanded request <=? limit) &&
    (topology_events current + expanded request <=? limit)
  else false.

Definition transport_offer source request :=
  Offer (act_cursor (origin request)) source
    (batch_direction request) (batch_width request).

Definition publish current next count :=
  Authority next (act_active current) (retained current)
    (topology_events current + count) (cursor_last next).

Definition preflight limit source current request : option authority :=
  if outer_guard limit current request then
    option_map (fun next => publish current next (expanded request))
      (commit (act_cursor current) (transport_offer source request))
  else None.

Definition apply_batch limit source current request :=
  match preflight limit source current request with
  | Some next => next
  | None => current
  end.

Definition trace source current request :=
  match batch_direction request with
  | Forward => forward_events source (cursor_index (act_cursor current)) (batch_width request)
  | Unwind => leave_events source (cursor_length (act_cursor current))
      (cursor_index (act_cursor current)) (batch_width request)
  end.

Definition observe_epm current event :=
  match event with
  | Edge byte => Authority (act_cursor current) (act_active current) (retained current)
      (S (topology_events current)) (Some (byte_provenance byte))
  | AbsentValue position | Leave position =>
      Authority (act_cursor current) (act_active current) (retained current)
        (S (topology_events current)) (Some position)
  end.

Definition observe_trace current events := fold_left observe_epm events current.

Theorem absent_value_preserves_value_table_and_endpoint_count current position :
  retained (observe_epm current (AbsentValue position)) = retained current /\
  topology_events (observe_epm current (AbsentValue position)) = S (topology_events current).
Proof. split; reflexivity. Qed.

Theorem scalar_observation_preserves_value_authority current events :
  retained (observe_trace current events) = retained current /\
  topology_events (observe_trace current events) = topology_events current + length events.
Proof.
  unfold observe_trace. revert current.
  induction events as [|event events IH]; intro current; cbn.
  - split; [reflexivity|lia].
  - specialize (IH (observe_epm current event)). destruct IH as [same count].
    destruct event; cbn in *; split; auto; lia.
Qed.

Theorem expanded_trace_counts_every_absent_value source current request :
  length (trace source current request) = expanded request.
Proof.
  pose proof (expanded_event_counts_preserve_logical_work source
    (cursor_index (act_cursor current)) (cursor_length (act_cursor current))
    (batch_width request)) as [forward_count unwind_count].
  unfold trace, expanded. destruct (batch_direction request); assumption.
Qed.

Theorem successful_preflight_commits_exact_scalar_transport limit source current request next :
  preflight limit source current request = Some next ->
  scalar_run source (expanded request) (act_cursor current) =
    Some (act_cursor next, trace source current request) /\
  retained next = retained current /\
  act_active next = act_active current /\
  topology_events next = topology_events current + expanded request /\
  frame_last_source next = cursor_last (act_cursor next).
Proof.
  unfold preflight. destruct (outer_guard limit current request); [|discriminate].
  destruct (commit (act_cursor current) (transport_offer source request)) as [inner|]
    eqn:accepted; cbn; [|discriminate].
  intro result. inversion result; subst next; clear result.
  pose proof (@accepted_commit_has_the_exact_scalar_trace
    (act_cursor current) (transport_offer source request) inner accepted) as exact_trace.
  cbn in exact_trace.
  split.
  - unfold expanded, trace. destruct (batch_direction request); exact exact_trace.
  - repeat split; reflexivity.
Qed.

Lemma scalar_step_publishes_observed_last source current next event outer :
  scalar_next source current = Some (next, event) ->
  frame_last_source (observe_epm outer event) = cursor_last next.
Proof.
  unfold scalar_next. destruct (cursor_phase current);
    try destruct (S (cursor_index current) <? cursor_length current);
    try destruct (cursor_index current <? cursor_length current);
    try discriminate; intro step; inversion step; reflexivity.
Qed.

Lemma scalar_run_publishes_observed_last source count current next events outer :
  frame_last_source outer = cursor_last current ->
  scalar_run source count current = Some (next, events) ->
  frame_last_source (observe_trace outer events) = cursor_last next.
Proof.
  revert current next events outer. induction count as [|count IH];
    intros current next events outer initial run; cbn [scalar_run] in run.
  - inversion run; subst; exact initial.
  - destruct (scalar_next source current) as [[middle event]|] eqn:step;
      [|discriminate].
    destruct (scalar_run source count middle) as [[final tail]|] eqn:rest;
      cbn in run; inversion run; subst.
    change (frame_last_source (observe_trace (observe_epm outer event) tail) = cursor_last next).
    eapply IH; [eapply scalar_step_publishes_observed_last; exact step|exact rest].
Qed.

Lemma nonempty_scalar_run_publishes_observed_last source count current next events outer :
  0 < count -> scalar_run source count current = Some (next, events) ->
  frame_last_source (observe_trace outer events) = cursor_last next.
Proof.
  destruct count as [|count]; [lia|]. intros positive run. cbn [scalar_run] in run.
  destruct (scalar_next source current) as [[middle event]|] eqn:step; [|discriminate].
  destruct (scalar_run source count middle) as [[final tail]|] eqn:rest;
    cbn in run; inversion run; subst.
  change (frame_last_source (observe_trace (observe_epm outer event) tail) = cursor_last next).
  eapply scalar_run_publishes_observed_last;
    [eapply scalar_step_publishes_observed_last; exact step|exact rest].
Qed.

Lemma successful_preflight_is_nonempty limit source current request next :
  preflight limit source current request = Some next -> 0 < expanded request.
Proof.
  unfold preflight. destruct (outer_guard limit current request); [|discriminate].
  unfold commit. destruct (guard (act_cursor current) (transport_offer source request))
    eqn:checked; [|discriminate]. intro accepted.
  unfold guard in checked.
  destruct (cursor_eq_dec (act_cursor current) (offer_origin (transport_offer source request)));
    [|discriminate]. apply andb_true_iff in checked. destruct checked as [positive _].
  change (0 <? batch_width request = true) in positive. apply Nat.ltb_lt in positive.
  unfold expanded. destruct (batch_direction request); lia.
Qed.

Theorem success_matches_outer_scalar_observation limit source current request next :
  preflight limit source current request = Some next ->
  retained next = retained (observe_trace current (trace source current request)) /\
  topology_events next = topology_events (observe_trace current (trace source current request)) /\
  frame_last_source next = frame_last_source (observe_trace current (trace source current request)).
Proof.
  intro accepted.
  pose proof (@successful_preflight_commits_exact_scalar_transport limit source current request next accepted)
    as [exact_trace [same [_ [count last]]]].
  pose proof (scalar_observation_preserves_value_authority current (trace source current request))
    as [scalar_same scalar_count].
  pose proof (@nonempty_scalar_run_publishes_observed_last source (expanded request)
    (act_cursor current) (act_cursor next) (trace source current request) current
    (@successful_preflight_is_nonempty limit source current request next accepted) exact_trace)
    as scalar_last.
  rewrite expanded_trace_counts_every_absent_value in scalar_count. repeat split; congruence.
Qed.

Theorem refused_preflight_preserves_all_authority limit source current request :
  preflight limit source current request = None ->
  apply_batch limit source current request = current.
Proof. intro declined. unfold apply_batch. now rewrite declined. Qed.

Theorem stale_enclosing_authority_declines_before_transport limit source current request :
  current <> origin request -> preflight limit source current request = None.
Proof.
  intro stale. unfold preflight, outer_guard.
  destruct (authority_eq_dec current (origin request)); [contradiction|reflexivity].
Qed.

Theorem zero_width_declines_before_publication limit source current request :
  batch_width request = 0 -> preflight limit source current request = None.
Proof.
  intro zero. unfold preflight. destruct (outer_guard limit current request); [|reflexivity].
  rewrite zero_width_offer_is_refused; [reflexivity|exact zero].
Qed.

Theorem ineligible_authority_declines_before_transport limit source current request :
  eligible current = false -> preflight limit source current request = None.
Proof.
  intro inactive. unfold preflight, outer_guard.
  destruct (authority_eq_dec current (origin request)); [rewrite inactive|]; reflexivity.
Qed.

Theorem inactive_cursor_declines limit source current request :
  act_active current = false -> preflight limit source current request = None.
Proof. intro inactive. apply ineligible_authority_declines_before_transport.
  unfold eligible. now rewrite inactive. Qed.

Theorem awaiting_child_declines limit source current request :
  awaiting_child (retained current) = true -> preflight limit source current request = None.
Proof. intro waiting. apply ineligible_authority_declines_before_transport.
  unfold eligible. rewrite waiting; cbn. now rewrite andb_false_r. Qed.

Theorem terminal_or_empty_declines limit source current request :
  terminal_state (retained current) <> Active \/ mode (retained current) = Empty ->
  preflight limit source current request = None.
Proof.
  intro stopped. apply ineligible_authority_declines_before_transport. unfold eligible.
  destruct (terminal_state (retained current)); destruct (mode (retained current));
    try (rewrite andb_false_r; reflexivity); destruct stopped; congruence.
Qed.

Theorem event_overflow_declines_before_transport limit source current request :
  limit < expanded request \/ limit < topology_events current + expanded request ->
  preflight limit source current request = None.
Proof.
  intro overflow. unfold preflight, outer_guard.
  destruct (authority_eq_dec current (origin request)); [|reflexivity].
  destruct overflow as [width_overflow|sum_overflow].
  - assert (expanded request <=? limit = false) by (apply Nat.leb_gt; assumption).
    rewrite H, andb_false_r; reflexivity.
  - assert (topology_events current + expanded request <=? limit = false)
      by (apply Nat.leb_gt; assumption).
    now rewrite H, andb_false_r.
Qed.

Theorem accepted_event_count_fits_limit limit source current request next :
  preflight limit source current request = Some next -> topology_events next <= limit.
Proof.
  intro accepted.
  pose proof (@successful_preflight_commits_exact_scalar_transport limit source current request next accepted)
    as [_ [_ [_ [count _]]]]. rewrite count.
  unfold preflight in accepted. destruct (outer_guard limit current request) eqn:guard;
    [|discriminate]. unfold outer_guard in guard.
  destruct (authority_eq_dec current (origin request)); [|discriminate].
  apply andb_true_iff in guard. destruct guard as [_ bound]. now apply Nat.leb_le.
Qed.

Theorem successful_last_position_comes_from_authoritative_source limit source current request next :
  preflight limit source current request = Some next ->
  frame_last_source next = Some (byte_provenance (source
    (match batch_direction request with
     | Forward => cursor_index (act_cursor current) + batch_width request - 1
     | Unwind => cursor_length (act_cursor current) -
         (cursor_index (act_cursor current) + batch_width request)
     end))).
Proof.
  unfold preflight. destruct (outer_guard limit current request); [|discriminate].
  unfold commit. destruct (guard (act_cursor current) (transport_offer source request))
    eqn:checked; [|discriminate].
  intro accepted. inversion accepted; subst next; clear accepted.
  unfold guard in checked.
  destruct (cursor_eq_dec (act_cursor current) (offer_origin (transport_offer source request)));
    [|discriminate]. apply andb_true_iff in checked. destruct checked as [positive _].
  change (0 <? batch_width request = true) in positive. apply Nat.ltb_lt in positive.
  destruct (batch_width request) eqn:width; [lia|].
  destruct (batch_direction request) eqn:way;
    cbn [publish transport_offer]; rewrite ?way, ?width; reflexivity.
Qed.

Theorem checked_guards_construct_success limit source current request :
  outer_guard limit current request = true ->
  guard (act_cursor current) (transport_offer source request) = true ->
  preflight limit source current request =
    Some (publish current
      (match batch_direction request with
       | Forward => forward source (batch_width request) (act_cursor current)
       | Unwind => unwind source (batch_width request) (act_cursor current)
       end) (expanded request)).
Proof.
  intros outer inner. unfold preflight. rewrite outer.
  unfold commit. rewrite inner. reflexivity.
Qed.

Theorem success_exists_exactly_when_both_guards_pass limit source current request :
  (exists next, preflight limit source current request = Some next) <->
  outer_guard limit current request = true /\
  guard (act_cursor current) (transport_offer source request) = true.
Proof.
  unfold preflight, commit.
  destruct (outer_guard limit current request);
    destruct (guard (act_cursor current) (transport_offer source request)); cbn;
    split; intros result; try (destruct result as [next result]; discriminate);
    try (destruct result; discriminate).
  - auto.
  - eexists; reflexivity.
Qed.

Definition control_retained := {|
  terminal_state := Active; mode := MapMode; empty_state := 0;
  empty_position := (0, 0); value_reader := [7; 11];
  value_table_identity := 3; value_table_start := 7; value_count := 11;
  value_count_position := (2, 3); endpoint_count := 0; value_records := 0;
  prefix_bytes_read := 7; prefix_bytes_traversed := 7;
  terminal_checks := 0; awaiting_child := false
|}.

Definition control_authority phase :=
  Authority (Cursor 1 2 3 [4] 3 0 phase 5 (Some (8, 9))) true
    control_retained 5 (Some (8, 9)).

Example forward_control_accepts_all_but_the_last_byte source :
  exists next, preflight 100 source (control_authority LineEdge)
    (Request (control_authority LineEdge) Forward 2) = Some next.
Proof. apply success_exists_exactly_when_both_guards_pass; split; reflexivity. Qed.

Example unwind_control_accepts_line_bytes_but_not_the_node_leave source :
  exists next, preflight 100 source (control_authority LineLeave)
    (Request (control_authority LineLeave) Unwind 3) = Some next.
Proof. apply success_exists_exactly_when_both_guards_pass; split; reflexivity. Qed.

Example forward_control_refuses_consuming_the_last_scalar_byte source :
  preflight 100 source (control_authority LineEdge)
    (Request (control_authority LineEdge) Forward 3) = None.
Proof. reflexivity. Qed.

Example unwind_control_refuses_crossing_the_node_leave source :
  preflight 100 source (control_authority LineLeave)
    (Request (control_authority LineLeave) Unwind 4) = None.
Proof. reflexivity. Qed.

Print Assumptions absent_value_preserves_value_table_and_endpoint_count.
Print Assumptions scalar_observation_preserves_value_authority.
Print Assumptions expanded_trace_counts_every_absent_value.
Print Assumptions successful_preflight_commits_exact_scalar_transport.
Print Assumptions success_matches_outer_scalar_observation.
Print Assumptions refused_preflight_preserves_all_authority.
Print Assumptions stale_enclosing_authority_declines_before_transport.
Print Assumptions zero_width_declines_before_publication.
Print Assumptions ineligible_authority_declines_before_transport.
Print Assumptions inactive_cursor_declines.
Print Assumptions awaiting_child_declines.
Print Assumptions terminal_or_empty_declines.
Print Assumptions event_overflow_declines_before_transport.
Print Assumptions accepted_event_count_fits_limit.
Print Assumptions successful_last_position_comes_from_authoritative_source.
Print Assumptions checked_guards_construct_success.
Print Assumptions success_exists_exactly_when_both_guards_pass.
Print Assumptions forward_control_accepts_all_but_the_last_byte.
Print Assumptions unwind_control_accepts_line_bytes_but_not_the_node_leave.
Print Assumptions forward_control_refuses_consuming_the_last_scalar_byte.
Print Assumptions unwind_control_refuses_crossing_the_node_leave.

End CanonicalActBatchPublication.
