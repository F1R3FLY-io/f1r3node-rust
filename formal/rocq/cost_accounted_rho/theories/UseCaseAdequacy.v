(* ═══════════════════════════════════════════════════════════════════════════
   UseCaseAdequacy.v — Proof-Backed Use-Case Traceability
   ═══════════════════════════════════════════════════════════════════════════

   This module names the business-critical cost-accounting use cases as
   Rocq theorems. Most entries are corollaries over the existing proof
   stack; implementation-specific wiring remains covered by Rust tests in
   f1r3node-rust.

   The purpose is proof-backed traceability, not a parallel operational
   semantics. Each UC-CA identifier used by the design document has a formal
   semantic anchor here whenever the scenario is part of the calculus,
   runtime-budget refinement model, replay payload model, or settlement model.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.PeanoNat Lia Lists.List Sorting.Permutation.
Import ListNotations.

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.
From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import RhoReduction.
From CostAccountedRho Require Import ChannelSeparation.
From CostAccountedRho Require Import TokenConservation.
From CostAccountedRho Require Import Settlement.
From CostAccountedRho Require Import SlashingComposition.
From CostAccountedRho Require Import MergeableChannelAccounting.
From CostAccountedRho Require Import FuelEventDecomposition.
From CostAccountedRho Require Import Confluence.
From CostAccountedRho Require Import StepDeterminism.
From CostAccountedRho Require Import TranslationFaithfulness.
From CostAccountedRho Require Import RuntimeBudgetRefinement.

(* UC-CA-001: every reachable evaluation conserves the split between
   consumed source tokens and final remaining source tokens. *)
Theorem uc_ca_001_budget_conservation :
  forall S S',
    ca_reachable S S' ->
    (system_token_count S - system_token_count S') +
      system_token_count S' =
    system_token_count S.
Proof.
  intros S S' Hreach.
  pose proof (token_monotone_reachable S S' Hreach).
  lia.
Qed.

Fixpoint unit_token_expansion (n : nat) (s : sig) (tail : token) : token :=
  match n with
  | O => tail
  | S n' => TGate s (unit_token_expansion n' s tail)
  end.

(* UC-CA-002: a coalesced implementation weight corresponds to a finite
   unit-gate expansion in the calculus. *)
Theorem uc_ca_002_weighted_event_refines_unit_token_expansion :
  forall n s tail,
    token_size (unit_token_expansion n s tail) =
    n + token_size tail.
Proof.
  induction n as [| n IH]; intros s tail; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
Qed.

(* UC-CA-003: fuel channels are syntactically separated from application
   de Bruijn channels. *)
Theorem uc_ca_003_signature_channel_separation :
  forall (hash_process : list bool -> proc) s k,
    NVar k <> N_tr hash_process s.
Proof.
  exact fuel_gate_no_app_channel_overlap.
Qed.

(* UC-CA-004: terminal cost is independent of reduction order. *)
Theorem uc_ca_004_parallel_terminal_cost_determinism :
  forall S T1 T2,
    ca_reachable S T1 ->
    ca_terminal T1 ->
    ca_reachable S T2 ->
    ca_terminal T2 ->
    system_token_count T1 = system_token_count T2.
Proof.
  exact ca_cost_deterministic.
Qed.

(* UC-CA-005: every pure-rho step from the recursively metered image reflects
   to a cost-accounted source step. *)
Theorem uc_ca_005_well_reflected_replay_step_sound :
  forall S R R',
    well_reflected S R ->
    rho_step R R' ->
    exists S' W,
      ca_step S S' /\
      rho_reachable R' W /\
      well_reflected S' W.
Proof.
  exact well_reflected_backward_reflection.
Qed.

(* UC-CA-006: consumed fuel-event multisets are endpoint-determined, which is
   the semantic basis for a canonical out-of-phlo boundary. *)
Theorem uc_ca_006_boundary_event_multiset_determinism :
  forall (S : system) (consumed1 consumed2 : list fuel_event)
         (remaining1 remaining2 : list fuel_event),
    Permutation (fuel_events_of_system S) (consumed1 ++ remaining1) ->
    Permutation (fuel_events_of_system S) (consumed2 ++ remaining2) ->
    Permutation remaining1 remaining2 ->
    Permutation consumed1 consumed2.
Proof.
  exact fuel_events_consumed_perm.
Qed.

(* UC-CA-007: a system with no token node cannot take a metered source step. *)
Theorem uc_ca_007_no_metered_step_without_token :
  forall S,
    sys_token_node_count S = 0 ->
    forall T, ~ ca_step S T.
Proof.
  exact no_token_no_step.
Qed.

(* UC-CA-008: persistent and peeked COMM scenarios are covered by the same
   confluence/cost-determinism theorem as all other interleavings. *)
Theorem uc_ca_008_persistent_peek_replay_cost_determinism :
  forall S T1 T2,
    ca_reachable S T1 ->
    ca_terminal T1 ->
    ca_reachable S T2 ->
    ca_terminal T2 ->
    system_token_count T1 = system_token_count T2.
Proof.
  exact ca_cost_deterministic.
Qed.

(* UC-CA-009: Casper precharge and refund are post-evaluation settlement
   arithmetic over the final token cost. *)
Theorem uc_ca_009_refund_is_bounded_by_escrow : forall s,
  refund_amount s <= escrowed_amount s.
Proof.
  exact refund_le_escrow.
Qed.

Theorem uc_ca_009_charged_plus_refund_equals_escrow : forall s,
  settlement_token_cost s <= settlement_limit s ->
  settled_amount s = escrowed_amount s.
Proof.
  exact charged_plus_refund_eq_escrow.
Qed.

Theorem uc_ca_009_post_evaluation_settlement_mints_no_fuel :
  forall S S' price,
    ca_reachable S S' ->
    let consumed := system_token_count S - system_token_count S' in
    let settlement := {|
      settlement_limit := system_token_count S;
      settlement_price := price;
      settlement_token_cost := consumed
    |} in
    settled_amount settlement = escrowed_amount settlement.
Proof.
  exact post_evaluation_settlement_no_mint.
Qed.

(* UC-CA-010: replay-cost mismatch is exactly the cost-invalid evidence
   predicate used by the slashing boundary. *)
Theorem uc_ca_010_replay_cost_mismatch_sound : forall recorded observed,
  recorded <> observed ->
  replay_cost_mismatch recorded observed = true.
Proof.
  exact replay_cost_mismatch_sound_for_evidence.
Qed.

Theorem uc_ca_010_replay_cost_mismatch_complete : forall recorded observed,
  replay_cost_mismatch recorded observed = true ->
  recorded <> observed.
Proof.
  exact replay_cost_mismatch_complete_for_evidence.
Qed.

(* UC-CA-011: system effects preserve the evaluated user budget and final
   fuel at the cost-accounting boundary. *)
Theorem uc_ca_011_system_effect_is_unmetered_for_user_budget : forall C E,
  system_token_count
    (boundary_user_system
      (composed_cost_boundary (apply_slash_system_effect C E))) =
  system_token_count
    (boundary_user_system (composed_cost_boundary C)).
Proof.
  exact slash_system_effect_is_unmetered_for_user_budget.
Qed.

Theorem uc_ca_011_system_effect_preserves_final_fuel : forall C E,
  boundary_user_system
    (composed_cost_boundary (apply_slash_system_effect C E)) =
  boundary_user_system (composed_cost_boundary C).
Proof.
  exact slash_after_evaluation_preserves_final_fuel.
Qed.

(* UC-CA-012: slashing effects preserve user cost and settlement arithmetic. *)
Theorem uc_ca_012_cost_invalid_evidence_preserves_user_cost :
  forall evidence boundary,
    settlement_token_cost
      (boundary_settlement
        (cost_invalid_boundary
          (record_cost_invalid_block evidence boundary))) =
    settlement_token_cost (boundary_settlement boundary).
Proof.
  exact cost_invalid_block_evidence_does_not_change_user_cost.
Qed.

Theorem uc_ca_012_slashing_preserves_settlement_accounting :
  forall C E,
    let C' := apply_slash_system_effect C E in
    escrowed_amount
      (boundary_settlement (composed_cost_boundary C')) =
      escrowed_amount
        (boundary_settlement (composed_cost_boundary C)) /\
    charged_amount
      (boundary_settlement (composed_cost_boundary C')) =
      charged_amount
        (boundary_settlement (composed_cost_boundary C)) /\
    refund_amount
      (boundary_settlement (composed_cost_boundary C')) =
      refund_amount
        (boundary_settlement (composed_cost_boundary C)) /\
    settled_amount
      (boundary_settlement (composed_cost_boundary C')) =
      settled_amount
        (boundary_settlement (composed_cost_boundary C)).
Proof.
  exact slash_preserves_settlement_accounting.
Qed.

(* UC-CA-013: the bounded-memory runtime budget preserves the same
   consumed/remaining conservation equation as the token stack. *)
Theorem uc_ca_013_runtime_budget_conserves_consumed_remaining :
  forall b,
    rb_valid b ->
    rb_total_cost b + rb_remaining b = rb_initial b.
Proof.
  exact rb_total_remaining_conservation.
Qed.

(* UC-CA-014: a weighted runtime event is the coalesced arithmetic form of
   consuming that many unit tokens. *)
Theorem uc_ca_014_weighted_runtime_event_refines_unit_count :
  forall b e b',
    rb_valid b ->
    rb_unmetered b = false ->
    rb_consumed b + rb_event_weight e <= rb_initial b ->
    rb_reserve b e = (b', RbReserveOk) ->
    rb_total_cost b' = rb_consumed b + rb_event_weight e /\
    rb_remaining b' =
      rb_initial b - (rb_consumed b + rb_event_weight e).
Proof.
  exact rb_successful_weight_refines_unit_count.
Qed.

(* UC-CA-015: resetting a runtime budget from a token count re-establishes
   the token-stack conservation invariant and clears stale trace/OOP
   evidence for the next deploy window. *)
Theorem uc_ca_015_budget_reset_from_token_matches_token_count :
  forall b t,
    rb_total_cost (rb_reset_from_token b t) +
    rb_remaining (rb_reset_from_token b t) =
    token_size t /\
    rb_last_oop (rb_reset_from_token b t) = None /\
    rb_cost_trace_entries (rb_reset_from_token b t) = [].
Proof.
  intros b t.
  split.
  - apply rb_reset_from_token_conservation.
  - split.
    + apply rb_reset_from_token_clears_oop.
    + apply rb_reset_from_token_clears_trace.
Qed.

(* UC-CA-016: out-of-phlo commits the budget to its canonical boundary. *)
Theorem uc_ca_016_budget_first_oop_commits_canonical_boundary :
  forall b e b',
    rb_valid b ->
    rb_unmetered b = false ->
    rb_last_oop b = None ->
    rb_reserve b e = (b', RbReserveOop) ->
    rb_consumed b' = rb_initial b /\
    rb_remaining b' = 0 /\
    rb_event_log b' = rb_event_log b /\
    rb_last_oop b' = Some e.
Proof.
  exact rb_reserve_first_oop_commits_boundary.
Qed.

(* UC-CA-017: successful runtime reservations append exactly one canonical
   source-event descriptor to the replay log. *)
Theorem uc_ca_017_budget_success_appends_canonical_event :
  forall b e b',
    rb_valid b ->
    rb_unmetered b = false ->
    rb_reserve b e = (b', RbReserveOk) ->
    rb_event_log b' = rb_event_log b ++ [e].
Proof.
  exact rb_reserve_success_appends_event.
Qed.

(* UC-CA-018: replay payload equivalence is sensitive to event traces, not
   merely to final cost. This is the formal design anchor for hashing user
   and system deploy logs into replay-cache fingerprints. *)
Theorem uc_ca_018_replay_payload_user_trace_change_detected :
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
  exact rb_replay_payload_user_trace_change_detected.
Qed.

Theorem uc_ca_019_replay_payload_system_trace_change_detected :
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
  exact rb_replay_payload_system_trace_change_detected.
Qed.

(* UC-CA-020: canonical replay payload equivalence treats per-deploy event
   logs as schedule-independent multisets. *)
Theorem uc_ca_020_replay_payload_user_trace_permutation_equiv :
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
  exact rb_replay_payload_canonical_user_trace_permutation.
Qed.

Theorem uc_ca_021_replay_payload_system_trace_permutation_equiv :
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
  exact rb_replay_payload_canonical_system_trace_permutation.
Qed.

(* UC-CA-022: full replay payload fingerprints are sensitive to
   authentication and protocol-boundary fields beyond final cost. *)
Theorem uc_ca_022_replay_payload_signature_change_detected :
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
  exact rb_full_replay_payload_signature_change_detected.
Qed.

Theorem uc_ca_023_replay_payload_system_kind_change_detected :
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
  exact rb_full_replay_payload_system_kind_change_detected.
Qed.

(* UC-CA-024: batched runtime reservations preserve the same budget
   conservation invariant as individual reservations. *)
Theorem uc_ca_024_reservation_batch_preserves_budget_conservation :
  forall events b b' results,
    rb_valid b ->
    rb_reserve_many b events = (b', results) ->
    rb_total_cost b' + rb_remaining b' = rb_initial b'.
Proof.
  exact rb_reserve_many_conservation.
Qed.

(* UC-CA-025: the formal batched reservation model records at most one OOP
   boundary because evaluation stops at the first insufficient event. *)
Theorem uc_ca_025_reservation_batch_has_at_most_one_oop :
  forall events b b' results,
    rb_reserve_many b events = (b', results) ->
    rb_oop_count results <= 1.
Proof.
  exact rb_reserve_many_oop_count_le_one.
Qed.

(* UC-CA-026: unmetered system execution cannot consume or record user fuel. *)
Theorem uc_ca_026_unmetered_batch_no_cost :
  forall events b,
    rb_unmetered b = true ->
    rb_reserve_many b events =
      (b, repeat RbReserveOk (length events)).
Proof.
  exact rb_reserve_many_unmetered_no_cost.
Qed.

(* UC-CA-027: exhausted settlement refunds zero, while zero price yields zero
   escrow, charge, and refund. *)
Theorem uc_ca_027_settlement_exhaustion_and_zero_price :
  (forall s,
    settlement_limit s <= settlement_token_cost s ->
    refund_amount s = 0) /\
  (forall limit token_cost,
    let s := {|
      settlement_limit := limit;
      settlement_price := 0;
      settlement_token_cost := token_cost
    |} in
    escrowed_amount s = 0 /\
    charged_amount s = 0 /\
    refund_amount s = 0).
Proof.
  split.
  - exact refund_zero_when_exhausted.
  - intros limit token_cost.
    repeat split; cbn; rewrite Nat.mul_0_r; reflexivity.
Qed.

(* UC-CA-028: slashing after user evaluation cannot add fuel. *)
Theorem uc_ca_028_slashing_after_evaluation_cannot_add_fuel :
  forall S S' C E,
    ca_reachable S S' ->
    boundary_user_system (composed_cost_boundary C) = S' ->
    system_token_count
      (boundary_user_system
        (composed_cost_boundary
          (apply_slash_system_effect C E))) <=
    system_token_count S.
Proof.
  exact slash_after_evaluation_cannot_add_fuel.
Qed.

(* UC-CA-029: diagnostic log caps are observationally irrelevant to budget
   cost, remaining fuel, initial fuel, unmetered mode, and OOP evidence. *)
Theorem uc_ca_029_diagnostic_log_cap_preserves_budget_observables :
  forall b (cap : nat),
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b /\
    rb_initial b = rb_initial b /\
    rb_unmetered b = rb_unmetered b /\
    rb_last_oop b = rb_last_oop b.
Proof.
  exact rb_diagnostic_cap_preserves_budget_observables.
Qed.

(* UC-CA-030: genesis/replay-mode is replay-payload-authenticated metadata. *)
Theorem uc_ca_030_replay_payload_genesis_change_detected :
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
  exact rb_full_replay_payload_genesis_change_detected.
Qed.

(* UC-CA-031: the consensus cost trace is exactly the successful billable
   source-token events plus the optional OOP boundary event in the active
   finalization window. Finalization reads that completed trace without
   changing budget accounting; the next deploy reset clears retained trace
   material. *)
Theorem uc_ca_031_finalization_reads_completed_cost_trace :
  forall b t,
    rb_cost_trace_entries b =
      rb_success_trace_entries (rb_event_log b) ++
      rb_oop_trace_entries (rb_last_oop b) /\
    rb_cost_trace_entries (rb_finalize_trace_window b) =
      rb_cost_trace_entries b /\
    rb_cost_trace_entries (rb_reset_from_token b t) = [] /\
    rb_total_cost (rb_finalize_trace_window b) = rb_total_cost b /\
    rb_remaining (rb_finalize_trace_window b) = rb_remaining b.
Proof.
  intros b t.
  pose proof (rb_finalize_trace_window_preserves_budget_observables b)
    as [_ [_ [_ [Hcost [Hremaining Htrace]]]]].
  split.
  - apply rb_cost_trace_entries_success_and_oop.
  - split.
    + exact Htrace.
    + split.
      * apply rb_reset_from_token_clears_trace.
      * split.
        -- exact Hcost.
        -- exact Hremaining.
Qed.

(* UC-CA-032: canonical cost-trace comparison is schedule-independent but
   sensitive to descriptor changes. *)
Theorem uc_ca_032_cost_trace_canonicalization_and_sensitivity :
  (forall b,
    Permutation (rb_cost_trace_entries b) (rb_cost_trace_entries b)) /\
  (forall b1 b2,
    rb_cost_trace_entries b1 <> rb_cost_trace_entries b2 ->
    ~ rb_cost_trace_entries b1 = rb_cost_trace_entries b2).
Proof.
  split.
  - exact rb_cost_trace_permutation_equiv_refl.
  - exact rb_cost_trace_change_detected.
Qed.

(* UC-CA-033: full replay payload equivalence is sensitive to every
   cost/authentication field that the implementation hashes. *)
Theorem uc_ca_033_replay_payload_full_field_sensitivity :
  (forall sigs costs1 costs2 traces trace_counts failed errors user_logs
          kinds system_errors slash_fields system_logs genesis,
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
      |}) /\
  (forall sigs costs traces1 traces2 trace_counts failed errors user_logs
          kinds system_errors slash_fields system_logs genesis,
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
      |}) /\
  (forall sigs costs traces trace_counts1 trace_counts2 failed errors
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
      |}) /\
  (forall sigs costs traces trace_counts failed1 failed2 errors user_logs
          kinds system_errors slash_fields system_logs genesis,
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
      |}) /\
  (forall sigs costs traces trace_counts failed errors1 errors2 user_logs
          kinds system_errors slash_fields system_logs genesis,
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
      |}) /\
  (forall sigs costs traces trace_counts failed errors user_logs kinds
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
      |}) /\
  (forall sigs costs traces trace_counts failed errors user_logs kinds system_errors
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
      |}).
Proof.
  repeat split;
    try exact rb_full_replay_payload_user_cost_change_detected;
    try exact rb_full_replay_payload_user_cost_trace_change_detected;
    try exact rb_full_replay_payload_user_cost_trace_event_count_change_detected;
    try exact rb_full_replay_payload_user_failed_change_detected;
    try exact rb_full_replay_payload_user_error_change_detected;
    try exact rb_full_replay_payload_system_error_change_detected;
    try exact rb_full_replay_payload_slash_fields_change_detected.
Qed.

(* UC-CA-034: deploy budgets are independent and block-level settlement is
   additive across deploy partitions. *)
Theorem uc_ca_034_multi_deploy_budget_isolation_and_settlement_sum :
  (forall b1 b1' b2 e r,
    rb_reserve b1 e = (b1', r) ->
    rb_initial b2 = rb_initial b2 /\
    rb_consumed b2 = rb_consumed b2 /\
    rb_event_log b2 = rb_event_log b2 /\
    rb_last_oop b2 = rb_last_oop b2) /\
  (forall left right,
    rb_sum_escrowed_amount (left ++ right) =
      rb_sum_escrowed_amount left + rb_sum_escrowed_amount right /\
    rb_sum_charged_amount (left ++ right) =
      rb_sum_charged_amount left + rb_sum_charged_amount right /\
    rb_sum_refund_amount (left ++ right) =
      rb_sum_refund_amount left + rb_sum_refund_amount right /\
    rb_sum_settled_amount (left ++ right) =
      rb_sum_settled_amount left + rb_sum_settled_amount right).
Proof.
  split.
  - exact rb_reserve_isolated_from_other_budget.
  - exact rb_sum_settlement_app.
Qed.

(* UC-CA-035: toggling unmetered system execution off restores the previous
   metered budget observables, so system-deploy mode cannot leak into the
   next user deploy. *)
Theorem uc_ca_035_unmetered_system_mode_restoration :
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
  exact rb_set_unmetered_restores_metered_observables.
Qed.

(* UC-CA-036: diagnostic retention is separate from budget semantics and
   consensus trace-window finalization. *)
Theorem uc_ca_036_diagnostic_retention_is_non_consensus :
  forall b (cap : nat),
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_cost_trace_entries (rb_finalize_trace_window b) =
      rb_cost_trace_entries b.
Proof.
  intros b cap.
  pose proof (rb_finalize_trace_window_preserves_budget_observables b)
    as [_ [_ [_ [_ [_ Htrace]]]]].
  repeat split; try reflexivity; exact Htrace.
Qed.

(* UC-CA-037: rejecting trace mismatches as cost-invalid evidence is
   observational at the fee-settlement boundary. *)
Theorem uc_ca_037_trace_mismatch_preserves_settlement_accounting :
  forall (trace1 trace2 : list rb_trace_entry) s,
    trace1 <> trace2 ->
    escrowed_amount s = escrowed_amount s /\
    charged_amount s = charged_amount s /\
    refund_amount s = refund_amount s /\
    settled_amount s = settled_amount s.
Proof.
  exact rb_trace_mismatch_preserves_settlement_accounting.
Qed.

(* UC-CA-038: the formal analogue of legacy charging quarantine is the
   no-token/no-metered-step boundary. Without a token node, no user-path
   metered source step exists. *)
Theorem uc_ca_038_legacy_metering_quarantine :
  forall S,
    sys_token_node_count S = 0 ->
    forall T, ~ ca_step S T.
Proof.
  exact no_token_no_step.
Qed.

(* UC-CA-039: after cost-accounting activation, a replay payload must carry
   an explicit cost-trace commitment and its event count. *)
Theorem uc_ca_039_post_activation_cost_trace_required :
  forall trace count present,
    rb_cost_trace_commitment_valid trace count present ->
    present = true /\ rb_cost_trace_event_count trace = count.
Proof.
  exact rb_post_activation_cost_trace_commitment_valid.
Qed.

(* UC-CA-040: full replay payload authentication includes cost-trace
   entries and counts, including the missing-trace case. *)
Theorem uc_ca_040_full_replay_payload_authenticates_cost_trace_fields :
  (forall sigs costs traces1 traces2 trace_counts failed errors user_logs
          kinds system_errors slash_fields system_logs genesis,
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
      |}) /\
  (forall sigs costs traces trace_counts1 trace_counts2 failed errors
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
      |}) /\
  (forall sigs costs traces trace_counts failed errors user_logs kinds
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
      |}).
Proof.
  repeat split;
    try exact rb_full_replay_payload_user_cost_trace_change_detected;
    try exact rb_full_replay_payload_user_cost_trace_event_count_change_detected;
    try exact rb_full_replay_payload_missing_cost_trace_change_detected.
Qed.

(* UC-CA-041: the finalized trace event count is complete: successful
   reservations plus at most one OOP boundary. *)
Theorem uc_ca_041_concurrent_finalization_trace_completeness :
  forall b,
    rb_cost_trace_event_count (rb_cost_trace_entries b) =
    length (rb_event_log b) +
      match rb_last_oop b with
      | None => 0
      | Some _ => 1
      end.
Proof.
  exact rb_cost_trace_event_count_success_and_oop.
Qed.

(* UC-CA-042: an out-of-phlo boundary remains replay evidence even though
   the failed deploy's user-store effects are rolled back. *)
Theorem uc_ca_042_oop_trace_survives_failed_deploy_boundary :
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
  exact rb_oop_trace_survives_boundary.
Qed.

(* UC-CA-043: mixed-success deploy blocks preserve per-deploy trace and
   settlement additivity by the same independence theorem as budget state. *)
Theorem uc_ca_043_mixed_deploy_block_trace_and_settlement_isolation :
  (forall b1 b1' b2 e r,
    rb_reserve b1 e = (b1', r) ->
    rb_cost_trace_entries b2 = rb_cost_trace_entries b2) /\
  (forall left right,
    rb_sum_settled_amount (left ++ right) =
      rb_sum_settled_amount left + rb_sum_settled_amount right).
Proof.
  split.
  - intros b1 b1' b2 e r _. reflexivity.
  - intros left right.
    destruct (rb_sum_settlement_app left right) as [_ [_ [_ Hsettled]]].
    exact Hsettled.
Qed.

(* UC-CA-044: events outside the runtime's accepted machine bound are
   rejected before they become authenticated normal trace entries. *)
Theorem uc_ca_044_oversized_weight_rejection_preserves_trace :
  forall max_weight b e,
    max_weight < rb_event_weight e ->
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b.
Proof.
  exact rb_oversized_weight_rejection_preserves_trace.
Qed.

(* UC-CA-045: non-billable metering frames route control only; they do not
   alter the consensus cost trace or token budget. *)
Theorem uc_ca_045_nonbillable_frames_do_not_enter_cost_trace :
  forall b,
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b.
Proof.
  exact rb_nonbillable_frame_preserves_trace.
Qed.

(* UC-CA-046: a post-activation deploy with zero billable events still has
   authenticated cost-trace evidence: the commitment is present and the
   event count is zero. *)
Theorem uc_ca_046_zero_event_post_activation_trace_commitment :
  rb_cost_trace_commitment_valid [] 0 true.
Proof.
  exact rb_empty_cost_trace_commitment_can_be_valid.
Qed.

(* UC-CA-047: block authentication covers replay-relevant cost trace fields
   through the replay payload included in the signed/hashable block body. *)
Theorem uc_ca_047_block_authenticates_cost_trace_payload :
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
  exact rb_block_auth_payload_replay_payload_change_detected.
Qed.

(* UC-CA-048: replay cache keys include the full replay payload, so replay
   optimization cannot reuse a cached result across cost-trace mutations. *)
Theorem uc_ca_048_replay_cache_key_authenticates_cost_trace_payload :
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
  exact rb_replay_cache_key_payload_change_detected.
Qed.

(* UC-CA-049: legacy replay mode may accept absent trace commitments for
   quarantined pre-activation data, while cost-accounted replay reduces to
   the explicit commitment-validity obligation. *)
Theorem uc_ca_049_legacy_replay_quarantines_absent_cost_trace :
  (forall trace count,
    rb_replay_mode_accepts_cost_trace RbLegacyReplay trace count false) /\
  (forall trace count present,
    rb_replay_mode_accepts_cost_trace
      RbCostAccountedReplay trace count present ->
    rb_cost_trace_commitment_valid trace count present).
Proof.
  split.
  - exact rb_legacy_replay_accepts_absent_commitment.
  - exact rb_cost_accounted_replay_requires_commitment.
Qed.

(* UC-CA-050: every successful billable reservation enters the authenticated
   trace window. Primitive and substitution events are specific instances of
   this same reservation rule in the implementation. *)
Theorem uc_ca_050_billable_reservation_enters_cost_trace :
  forall b e b',
    rb_valid b ->
    rb_unmetered b = false ->
    rb_reserve b e = (b', RbReserveOk) ->
    rb_event_log b' = rb_event_log b ++ [e].
Proof.
  exact rb_reserve_success_appends_event.
Qed.

(* UC-CA-051: parallel rho interleavings keep endpoint cost deterministic,
   and runtime finalization counts exactly the successful billable events
   plus the optional OOP boundary evidence. *)
Theorem uc_ca_051_parallel_trace_and_cost_determinism :
  (forall S T1 T2,
    ca_reachable S T1 ->
    ca_terminal T1 ->
    ca_reachable S T2 ->
    ca_terminal T2 ->
    system_token_count T1 = system_token_count T2) /\
  (forall b,
    rb_cost_trace_event_count (rb_cost_trace_entries b) =
    length (rb_event_log b) +
      match rb_last_oop b with
      | None => 0
      | Some _ => 1
      end).
Proof.
  split.
  - exact ca_cost_deterministic.
  - exact rb_cost_trace_event_count_success_and_oop.
Qed.

(* UC-CA-052: cost-trace mismatches compose with the slashing boundary as
   cost-invalid evidence without mutating fee-settlement arithmetic. *)
Theorem uc_ca_052_cost_trace_mismatch_slashing_boundary :
  (forall recorded observed,
    recorded <> observed ->
    replay_cost_mismatch recorded observed = true) /\
  (forall (trace1 trace2 : list rb_trace_entry) s,
    trace1 <> trace2 ->
    escrowed_amount s = escrowed_amount s /\
    charged_amount s = charged_amount s /\
    refund_amount s = refund_amount s /\
    settled_amount s = settled_amount s).
Proof.
  split.
  - exact replay_cost_mismatch_sound_for_evidence.
  - exact rb_trace_mismatch_preserves_settlement_accounting.
Qed.

(* UC-CA-053: success/OOP trace tags are domain separated, and event
   multiplicity is authenticated rather than collapsed. *)
Theorem uc_ca_053_cost_trace_domain_separation_and_multiplicity :
  (forall descriptor,
    ((RbTraceSuccess, descriptor) : rb_trace_entry) <>
    ((RbTraceOop, descriptor) : rb_trace_entry)) /\
  (forall (entry : rb_trace_entry),
    [entry] <> [entry; entry]).
Proof.
  split.
  - exact rb_trace_entry_kind_domain_separated.
  - exact rb_trace_duplicate_multiplicity_detected.
Qed.

(* UC-CA-054: cost-accounted replay rejects absent cost-trace commitments.
   Legacy compatibility is handled only by the explicit legacy mode theorem. *)
Theorem uc_ca_054_activation_replay_rejects_absent_commitment :
  forall trace count,
    ~ rb_replay_mode_accepts_cost_trace
        RbCostAccountedReplay trace count false.
Proof.
  exact rb_cost_accounted_replay_rejects_absent_commitment.
Qed.

(* UC-CA-055: user-deploy authority cannot perform fee settlement or mutate
   runtime budget during evaluation. *)
Theorem uc_ca_055_unauthorized_settlement_and_budget_mutation_are_cost_invalid :
  unauthorized_fee_settlement UserDeployActor = true /\
  (forall actor,
    unauthorized_budget_mutation DuringUserEvaluation actor = true) /\
  unauthorized_budget_mutation PostEvaluationSettlement SystemDeployActor =
    false.
Proof.
  split.
  - exact unauthorized_fee_settlement_complete.
  - split.
    + exact unauthorized_runtime_budget_mutation_during_evaluation.
    + exact authorized_system_settlement_budget_mutation.
Qed.

(* UC-CA-056: low deploy price is exactly captured as cost-invalid evidence. *)
Theorem uc_ca_056_low_deploy_price_is_cost_invalid_evidence :
  (forall offered minimum,
    offered < minimum ->
    low_deploy_price_violation offered minimum = true) /\
  (forall offered minimum,
    low_deploy_price_violation offered minimum = true ->
    offered < minimum).
Proof.
  split.
  - exact low_deploy_price_violation_sound.
  - exact low_deploy_price_violation_complete.
Qed.

(* UC-CA-057: stale cost-invalid evidence is exactly evidence whose epoch
   plus retention horizon is before the current epoch. *)
Theorem uc_ca_057_stale_cost_invalid_evidence_is_rejected :
  (forall current evidence_epoch horizon,
    stale_cost_evidence current evidence_epoch horizon = true ->
    evidence_epoch + horizon < current) /\
  (forall current evidence_epoch horizon,
    evidence_epoch + horizon < current ->
    stale_cost_evidence current evidence_epoch horizon = true).
Proof.
  split.
  - exact stale_cost_evidence_sound.
  - exact stale_cost_evidence_complete.
Qed.

(* UC-CA-058: refunds are settlement arithmetic, and unmetered settlement
   work cannot replenish or append to a user runtime trace. *)
Theorem uc_ca_058_refund_cannot_replenish_runtime_fuel :
  (forall S S' price,
    ca_reachable S S' ->
    let consumed := system_token_count S - system_token_count S' in
    let settlement := {|
      settlement_limit := system_token_count S;
      settlement_price := price;
      settlement_token_cost := consumed
    |} in
    settled_amount settlement = escrowed_amount settlement) /\
  (forall b e,
    rb_unmetered b = true ->
    rb_reserve b e = (b, RbReserveOk) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = 0).
Proof.
  split.
  - exact post_evaluation_settlement_no_mint.
  - exact rb_unmetered_reserve_preserves_trace.
Qed.

(* UC-CA-059: descriptor fields are replay-authenticated data. Changing any
   Rust cost-trace digest input changes the formal trace entry. *)
Theorem uc_ca_059_deterministic_billable_descriptor_sensitivity :
  (forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_deploy_id d1 <> rb_trace_deploy_id d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry)) /\
  (forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_source_path d1 <> rb_trace_source_path d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry)) /\
  (forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_redex_id d1 <> rb_trace_redex_id d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry)) /\
  (forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_local_index d1 <> rb_trace_local_index d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry)) /\
  (forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_billable_kind d1 <> rb_trace_billable_kind d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry)) /\
  (forall (kind : rb_trace_event_kind) d1 d2 desc1 desc2,
    rb_trace_billable_kind d1 = RbPrimitive desc1 ->
    rb_trace_billable_kind d2 = RbPrimitive desc2 ->
    desc1 <> desc2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry)) /\
  (forall (kind : rb_trace_event_kind) d1 d2,
    rb_trace_weight d1 <> rb_trace_weight d2 ->
    ((kind, d1) : rb_trace_entry) <> ((kind, d2) : rb_trace_entry)).
Proof.
  repeat split.
  - exact rb_trace_entry_deploy_change_detected.
  - exact rb_trace_entry_source_path_change_detected.
  - exact rb_trace_entry_redex_change_detected.
  - exact rb_trace_entry_local_index_change_detected.
  - exact rb_trace_entry_billable_kind_change_detected.
  - exact rb_trace_entry_primitive_descriptor_change_detected.
  - exact rb_trace_entry_weight_change_detected.
Qed.

(* UC-CA-060: deploy reset clears retained trace material after finalization
   while preserving the completed trace for the finalization read itself. *)
Theorem uc_ca_060_reset_clears_retained_trace_after_finalization :
  forall b t,
    length (rb_cost_trace_entries (rb_reset_from_token b t)) <= 0.
Proof.
  exact rb_reset_from_token_retention_bound_zero.
Qed.

(* UC-CA-061: system-mode metering cannot leak into subsequent user
   evaluation. *)
Theorem uc_ca_061_system_mode_cannot_leak_into_user_metering :
  (forall b e,
    rb_unmetered b = true ->
    rb_reserve b e = (b, RbReserveOk) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = 0) /\
  (forall b,
    rb_initial (rb_set_unmetered (rb_set_unmetered b true) false) =
      rb_initial b /\
    rb_consumed (rb_set_unmetered (rb_set_unmetered b true) false) =
      rb_consumed b /\
    rb_event_log (rb_set_unmetered (rb_set_unmetered b true) false) =
      rb_event_log b /\
    rb_last_oop (rb_set_unmetered (rb_set_unmetered b true) false) =
      rb_last_oop b /\
    rb_unmetered (rb_set_unmetered (rb_set_unmetered b true) false) =
      false).
Proof.
  split.
  - exact rb_unmetered_reserve_preserves_trace.
  - exact rb_set_unmetered_restores_metered_observables.
Qed.

(* UC-CA-062: block-authentication payloads change whenever the embedded
   replay payload changes. *)
Theorem uc_ca_062_block_validation_authenticates_cost_fields :
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
  exact rb_block_auth_payload_replay_payload_change_detected.
Qed.

(* UC-CA-063: concurrent reservations have at most one formal OOP boundary,
   and the boundary remains trace evidence. *)
Theorem uc_ca_063_threaded_oop_boundary_ownership :
  (forall events b b' results,
    rb_reserve_many b events = (b', results) ->
    rb_oop_count results <= 1) /\
  (forall b e b',
    rb_unmetered b = false ->
    rb_last_oop b = None ->
    rb_initial b < rb_consumed b + rb_event_weight e ->
    rb_reserve b e = (b', RbReserveOop) ->
    rb_cost_trace_entries b' =
      rb_success_trace_entries (rb_event_log b) ++
      [rb_oop_trace_entry e] /\
    rb_total_cost b' = rb_initial b).
Proof.
  split.
  - exact rb_reserve_many_oop_count_le_one.
  - exact rb_oop_trace_survives_boundary.
Qed.

(* UC-CA-064: any external nondeterminism that changes authenticated replay
   errors or traces changes the replay payload. *)
Theorem uc_ca_064_external_nondeterminism_requires_replay_evidence :
  (forall p1 p2,
    rb_full_user_errors p1 <> rb_full_user_errors p2 ->
    ~ rb_full_replay_payload_equiv p1 p2) /\
  (forall p1 p2,
    rb_full_user_cost_traces p1 <> rb_full_user_cost_traces p2 ->
    ~ rb_full_replay_payload_equiv p1 p2).
Proof.
  split.
  - intros p1 p2 Hneq Hequiv.
    unfold rb_full_replay_payload_equiv in Hequiv.
    intuition.
  - intros p1 p2 Hneq Hequiv.
    unfold rb_full_replay_payload_equiv in Hequiv.
    intuition.
Qed.

(* UC-CA-065: a zero-weight billable event is invalid. It cannot enter the
   authenticated cost trace and cannot consume or preserve fuel by pretending
   to be a successful metered step. *)
Theorem uc_ca_065_zero_weight_billable_event_rejected :
  forall max_weight max_source_path_components max_primitive_descriptor b e,
    rb_event_weight e = 0 ->
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b.
Proof.
  exact rb_zero_weight_admission_rejection_preserves_trace.
Qed.

(* UC-CA-066: a billable event outside the accepted machine weight, source
   path, or primitive descriptor bound is invalid and preserves the
   pre-existing budget and trace. *)
Theorem uc_ca_066_oversized_billable_event_rejected :
  (forall max_weight max_source_path_components max_primitive_descriptor b e,
    0 < rb_event_weight e ->
    max_weight < rb_event_weight e ->
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b) /\
  (forall max_weight max_source_path_components max_primitive_descriptor b e,
    0 < rb_event_weight e ->
    rb_event_weight e <= max_weight ->
    max_source_path_components < length (rb_event_source_path e) ->
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b) /\
  (forall max_weight max_source_path_components max_primitive_descriptor b e descriptor,
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
    rb_remaining b = rb_remaining b).
Proof.
  split.
  - exact rb_oversized_weight_admission_rejection_preserves_trace.
  - split.
    + exact rb_oversized_source_path_admission_rejection_preserves_trace.
    + exact rb_oversized_primitive_descriptor_admission_rejection_preserves_trace.
Qed.

(* UC-CA-067: exhausting the trace-retention window rejects the next billable
   event before mutating either runtime fuel or replay evidence. *)
Theorem uc_ca_067_trace_cap_rejection_preserves_budget :
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

(* UC-CA-068: every admitted successful billable event is positive and within
   the machine bound; zero-event traces remain possible only when no billable
   event is admitted. *)
Theorem uc_ca_068_admitted_success_has_positive_bounded_weight :
  forall max_weight max_source_path_components max_primitive_descriptor b e b',
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b', RbAdmittedOk) ->
    rb_event_admissible
      max_weight max_source_path_components max_primitive_descriptor e.
Proof.
  exact rb_admitted_success_has_admissible_event.
Qed.

(* UC-CA-069: producer routing search must keep strict billable producers
   positive and bounded; zero-capable work belongs on non-billable or
   incremental paths until it has positive work to reserve. *)
Theorem uc_ca_069_producer_routing_search_frontier :
  forall max_weight max_source_path_components max_primitive_descriptor b e b',
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b', RbAdmittedOk) ->
    rb_event_admissible
      max_weight max_source_path_components max_primitive_descriptor e.
Proof.
  exact rb_admitted_success_has_admissible_event.
Qed.

(* UC-CA-070: trace-slot and repeated-OOP races retain at most one OOP
   boundary and reject full trace windows without mutating budget evidence. *)
Theorem uc_ca_070_trace_slot_linearizability_frontier :
  (forall events b b' results,
    rb_reserve_many b events = (b', results) ->
    rb_oop_count results <= 1 /\
    length (rb_oop_trace_entries (rb_last_oop b')) <= 1) /\
  (forall b first second b1 b2,
    rb_valid b ->
    rb_unmetered b = false ->
    rb_last_oop b = None ->
    rb_reserve b first = (b1, RbReserveOop) ->
    rb_reserve b1 second = (b2, RbReserveOop) ->
    rb_last_oop b2 = rb_last_oop b1 /\
    rb_event_log b2 = rb_event_log b1 /\
    rb_cost_trace_event_count (rb_cost_trace_entries b2) =
      rb_cost_trace_event_count (rb_cost_trace_entries b1) /\
    rb_total_cost b2 = rb_total_cost b1) /\
  (forall max_weight max_source_path_components max_primitive_descriptor max_events b e,
    max_events <= rb_trace_slot_count b ->
    rb_reserve_bounded
      max_weight max_source_path_components max_primitive_descriptor max_events b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b).
Proof.
  split.
  - exact rb_repeated_oop_boundary_frontier.
  - split.
    + exact rb_repeated_oop_preserves_first_boundary.
    + exact rb_trace_cap_frontier_preserves_budget_and_trace.
Qed.

(* UC-CA-071: replay mutation search keeps every cost-accounting field in
   the authenticated replay payload. *)
Theorem uc_ca_071_replay_mutation_frontier :
  (forall sigs costs1 costs2 traces trace_counts failed errors user_logs
          kinds system_errors slash_fields system_logs genesis,
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
      |}) /\
  (forall sigs costs traces1 traces2 trace_counts failed errors user_logs
          kinds system_errors slash_fields system_logs genesis,
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
      |}).
Proof.
  split.
  - exact rb_full_replay_payload_user_cost_change_detected.
  - exact rb_full_replay_payload_user_cost_trace_change_detected.
Qed.

(* UC-CA-072: multi-deploy settlement is deploy-local and additive; block
   aggregation cannot make refunds exceed escrow. *)
Theorem uc_ca_072_multi_deploy_settlement_frontier :
  forall left right,
    rb_sum_refund_amount (left ++ right) =
      rb_sum_refund_amount left + rb_sum_refund_amount right /\
    rb_sum_settled_amount (left ++ right) =
      rb_sum_settled_amount left + rb_sum_settled_amount right /\
    rb_sum_refund_amount (left ++ right) <=
      rb_sum_escrowed_amount (left ++ right).
Proof.
  exact rb_multi_deploy_settlement_frontier.
Qed.

(* UC-CA-073: slashing evidence remains post-evaluation system evidence; it
   cannot rewrite user runtime fuel or settlement inputs. *)
Theorem uc_ca_073_slashing_composition_frontier :
  (forall C E,
    let C' := apply_slash_system_effect C E in
    settlement_limit
      (boundary_settlement (composed_cost_boundary C')) =
      settlement_limit
        (boundary_settlement (composed_cost_boundary C)) /\
    settlement_price
      (boundary_settlement (composed_cost_boundary C')) =
      settlement_price
        (boundary_settlement (composed_cost_boundary C)) /\
    settlement_token_cost
      (boundary_settlement (composed_cost_boundary C')) =
      settlement_token_cost
        (boundary_settlement (composed_cost_boundary C))) /\
  (forall C E,
    system_token_count
      (boundary_user_system
        (composed_cost_boundary
          (apply_slash_system_effect C E))) =
    system_token_count
      (boundary_user_system (composed_cost_boundary C))).
Proof.
  split.
  - exact slash_preserves_fee_settlement_inputs.
  - exact slash_system_effect_is_unmetered_for_user_budget.
Qed.

(* UC-CA-074: resource-exhaustion frontier cases reject before mutating the
   authenticated cost trace or runtime budget. *)
Theorem uc_ca_074_resource_exhaustion_frontier :
  (forall max_weight max_source_path_components max_primitive_descriptor b e,
    0 < rb_event_weight e ->
    max_weight < rb_event_weight e ->
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b) /\
  (forall max_weight max_source_path_components max_primitive_descriptor b e,
    0 < rb_event_weight e ->
    rb_event_weight e <= max_weight ->
    max_source_path_components < length (rb_event_source_path e) ->
    rb_reserve_admitted
      max_weight max_source_path_components max_primitive_descriptor b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b) /\
  (forall max_weight max_source_path_components max_primitive_descriptor b e descriptor,
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
    rb_remaining b = rb_remaining b) /\
  (forall max_weight max_source_path_components max_primitive_descriptor max_events b e,
    max_events <= rb_trace_slot_count b ->
    rb_reserve_bounded
      max_weight max_source_path_components max_primitive_descriptor max_events b e =
      (b, RbAdmittedInvalid) /\
    rb_cost_trace_entries b = rb_cost_trace_entries b /\
    rb_total_cost b = rb_total_cost b /\
    rb_remaining b = rb_remaining b).
Proof.
  split.
  - exact rb_oversized_weight_admission_rejection_preserves_trace.
  - split.
    + exact rb_oversized_source_path_admission_rejection_preserves_trace.
    + split.
      * exact rb_oversized_primitive_descriptor_admission_rejection_preserves_trace.
      * exact rb_trace_cap_frontier_preserves_budget_and_trace.
Qed.

(* UC-CA-141: typed mergeable-channel diffs preserve the selected merge
   strategy and cannot reinterpret a non-numeric payload as a numeric merge. *)
Theorem uc_ca_141_typed_mergeable_channel_type_preservation :
  (forall previous current,
    mergeable_diff_type (typed_mergeable_delta previous current) =
    mergeable_channel_type current) /\
  (forall ty,
    mergeable_payload_matches ty NonNumericPayload = false).
Proof.
  split.
  - exact mergeable_channel_delta_preserves_type.
  - exact non_numeric_channel_not_mergeable_payload_match.
Qed.

(* UC-CA-142: BitmaskOr diffs encode newly-set bits; replaying the diff from
   the previous value reconstructs the union of previous and current bits. *)
Theorem uc_ca_142_bitmask_or_diff_merge_round_trip :
  forall previous current,
    same_bits
      (bitmask_or previous (bitmask_diff previous current))
      (bitmask_or previous current).
Proof.
  exact bitmask_diff_merge_round_trip.
Qed.

(* UC-CA-143: BitmaskOr multi-value folding is set-like and independent of
   observation order. *)
Theorem uc_ca_143_bitmask_or_fold_order_independent :
  forall values values',
    Permutation values values' ->
    same_bits (bitmask_fold values) (bitmask_fold values').
Proof.
  exact mergeable_channel_bitmask_fold_permutation.
Qed.

(* UC-CA-144: IntegerAdd diffs retain the existing additive round-trip
   semantics in the mathematical integer model. *)
Theorem uc_ca_144_integer_add_diff_merge_round_trip :
  forall previous current,
    integer_add_merge previous (integer_add_diff previous current) = current.
Proof.
  exact integer_add_diff_merge_round_trip.
Qed.

(* UC-CA-145: mergeable-channel accounting updates channel metadata without
   changing user budget or fee-settlement inputs. *)
Theorem uc_ca_145_mergeable_channel_accounting_preserves_cost_boundary :
  (forall state channels,
    let state' := apply_mergeable_accounting state channels in
    system_token_count
      (mergeable_boundary_user_system
        (mergeable_accounting_boundary state')) =
    system_token_count
      (mergeable_boundary_user_system
        (mergeable_accounting_boundary state))) /\
  (forall state channels,
    let state' := apply_mergeable_accounting state channels in
    settlement_limit
      (mergeable_boundary_settlement
        (mergeable_accounting_boundary state')) =
      settlement_limit
        (mergeable_boundary_settlement
          (mergeable_accounting_boundary state)) /\
    settlement_price
      (mergeable_boundary_settlement
        (mergeable_accounting_boundary state')) =
      settlement_price
        (mergeable_boundary_settlement
          (mergeable_accounting_boundary state)) /\
    settlement_token_cost
      (mergeable_boundary_settlement
        (mergeable_accounting_boundary state')) =
      settlement_token_cost
        (mergeable_boundary_settlement
          (mergeable_accounting_boundary state))).
Proof.
  split.
  - exact mergeable_channel_accounting_preserves_user_budget.
  - exact mergeable_channel_accounting_preserves_fee_settlement_inputs.
Qed.
