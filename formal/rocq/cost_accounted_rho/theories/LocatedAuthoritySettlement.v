(* G3 (DR-135, design v2 rules R2 to R4 and R10; DR-68 items 1 to 3):
   owner purses over a whole run.

   An owner purse pays for the stored process of its owner when another
   deployment fires it. No deployment reserves it. So the bound of an owner
   purse is not a reservation but its owner budget (v2 R3: the balance at
   R_{i-1} minus the later holds and the own holds). Each event reserves its
   plan or is rejected with no effect (reserve_or_reject). A run folds that
   decision over the events in the order of their critical sections.

   Added by G3, with no axiom:
   - run_debits_within_initial_balance: after a run, every purse holds its
     initial balance minus the demand of the fired events, and that demand
     never exceeds the initial balance. For an owner purse the initial
     balance is the owner budget, so a run never draws an owner purse past
     its budget. The decision procedure of the run is a section variable,
     so the theorem holds for every decision procedure.
   - nc_ungated_run_overdraws: a proved negative control. A run that
     debits every event without the gate draws two units from a purse of
     budget one, and the conclusion of the theorem fails for it.

   Added by G3 step S11 (DR-135 item 13), with no axiom (caller run):
   - caller_run_client_part_within_limit: a successful run of the caller's
     meter counts against the signed limit only the client part, and that
     part is within the limit. The total usage is the whole run.
   - caller_run_ignores_priced_owner_usage: the decisions of the meter and
     the counted part depend only on the usage that the S11 rule counts,
     so the priced owner work never counts against the limit.
   - caller_run_total_work_bounded: the total work of a successful run is
     the caller's own work, the owner work at price 0 and the priced owner
     work. The first two count for the caller and stay within the limit.
   - nc_counting_priced_owner_usage_refuses_a_fee_only_caller: a proved
     negative control. The rule before S11 counts a priced owner share
     against the limit and denies a run that the S11 rule grants.
   - nc_uncounted_zero_price_work_exceeds_the_bound: a proved negative
     control. A rule that also drops the owner work at price 0 grants a
     run whose own work and owner work at price 0 exceed the limit, so the
     conclusion of caller_run_total_work_bounded fails for it.

   Formulations rejected before any proof attempt (reasoned):
   - A run over a decidable, finite purse type would need a list of all
     purses. The section variable can_debit_dec keeps the purse type
     abstract, as reserve_or_reject already does with its decision argument.
   - A statement with truncated subtraction alone (final = initial - demand)
     is true for the ungated run too, because nat subtraction stops at zero.
     The theorem therefore states final + demand = initial, which the
     ungated run violates. *)
From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Section LocatedAuthoritySettlement.

Context {signature location purse event : Type}.
Context (purse_eq_dec : forall left right : purse, {left = right} + {left <> right}).

Record region := {
  region_signature : signature;
  region_location : location
}.

Definition purse_balance := purse -> nat.
Definition region_purse_plan := region -> purse.

Definition purse_demand
  (regions : list region)
  (plan : region_purse_plan)
  (selected : purse)
  : nat :=
  fold_right
    (fun current total =>
       if purse_eq_dec (plan current) selected then S total else total)
    0
    regions.

Definition can_debit
  (balance : purse_balance)
  (regions : list region)
  (plan : region_purse_plan)
  : Prop :=
  forall selected, purse_demand regions plan selected <= balance selected.

Definition debit
  (balance : purse_balance)
  (regions : list region)
  (plan : region_purse_plan)
  : purse_balance :=
  fun selected => balance selected - purse_demand regions plan selected.

Definition reserve_or_reject
  (balance : purse_balance)
  (regions : list region)
  (plan : region_purse_plan)
  (decision : {can_debit balance regions plan} + {~ can_debit balance regions plan})
  : purse_balance * bool :=
  if decision then (debit balance regions plan, true) else (balance, false).

Record located_plan := {
  located_regions : list region;
  located_purses : region_purse_plan;
  located_near : region -> purse -> Prop
}.

Definition plan_is_located (plan : located_plan) : Prop :=
  forall current,
    In current (located_regions plan) ->
    located_near plan current (located_purses plan current).

Definition atom_multiset (regions : list region) : list signature :=
  map region_signature regions.

Definition plans_preserve_authority
  (left right : located_plan)
  : Prop :=
  Permutation
    (atom_multiset (located_regions left))
    (atom_multiset (located_regions right)).

Definition replay_refines
  (play replay : located_plan)
  : Prop :=
  located_regions play = located_regions replay /\
  forall current,
    In current (located_regions play) ->
    located_purses play current = located_purses replay current.

Definition dependencies_committed
  (dependencies committed : list event)
  : Prop :=
  forall dependency, In dependency dependencies -> In dependency committed.

Definition event_debit
  (balance : purse_balance)
  (plan : located_plan)
  : purse_balance :=
  debit balance (located_regions plan) (located_purses plan).

Definition settlement_conserves
  (initial final : purse_balance)
  (regions : list region)
  (plan : region_purse_plan)
  : Prop :=
  forall selected,
    final selected + purse_demand regions plan selected = initial selected.

Theorem rejected_event_is_state_atomic : forall balance regions plan decision,
  ~ can_debit balance regions plan ->
  reserve_or_reject balance regions plan decision = (balance, false).
Proof.
  intros balance regions plan decision rejected.
  unfold reserve_or_reject.
  destruct decision as [funded | underfunded].
  - contradiction.
  - reflexivity.
Qed.

Theorem admitted_event_debits_exactly : forall balance regions plan decision,
  can_debit balance regions plan ->
  reserve_or_reject balance regions plan decision =
    (debit balance regions plan, true).
Proof.
  intros balance regions plan decision funded.
  unfold reserve_or_reject.
  destruct decision as [accepted | rejected].
  - reflexivity.
  - contradiction.
Qed.

Theorem debit_preserves_unselected_purse : forall balance regions plan selected,
  purse_demand regions plan selected = 0 ->
  debit balance regions plan selected = balance selected.
Proof.
  intros balance regions plan selected absent.
  unfold debit.
  rewrite absent, Nat.sub_0_r.
  reflexivity.
Qed.

Theorem explicit_regions_do_not_debit_ambient_purse :
  forall balance regions plan ambient,
    (forall current, In current regions -> plan current <> ambient) ->
    debit balance regions plan ambient = balance ambient.
Proof.
  intros balance regions plan ambient separated.
  apply debit_preserves_unselected_purse.
  induction regions as [| current rest IH].
  - reflexivity.
  - cbn.
    destruct (purse_eq_dec (plan current) ambient) as [same | different].
    + exfalso.
      exact (separated current (or_introl eq_refl) same).
    + apply IH.
      intros candidate present.
      apply separated.
      now right.
Qed.

Theorem debit_conserves_each_purse : forall balance regions plan selected,
  purse_demand regions plan selected <= balance selected ->
  debit balance regions plan selected
    + purse_demand regions plan selected
    = balance selected.
Proof.
  intros balance regions plan selected sufficient.
  unfold debit.
  lia.
Qed.

Theorem admitted_settlement_conserves : forall balance regions plan,
  can_debit balance regions plan ->
  settlement_conserves balance (debit balance regions plan) regions plan.
Proof.
  intros balance regions plan funded selected.
  apply debit_conserves_each_purse.
  apply funded.
Qed.

Theorem plan_permutation_preserves_authority : forall left right,
  Permutation (located_regions left) (located_regions right) ->
  plans_preserve_authority left right.
Proof.
  intros left right permutation.
  unfold plans_preserve_authority, atom_multiset.
  now apply Permutation_map.
Qed.

Theorem replay_preserves_authority : forall play replay,
  replay_refines play replay ->
  plans_preserve_authority play replay.
Proof.
  intros play replay [same_regions same_purses].
  unfold plans_preserve_authority, atom_multiset.
  now rewrite same_regions.
Qed.

Lemma purse_demand_plan_ext : forall regions left right selected,
  (forall current, In current regions -> left current = right current) ->
  purse_demand regions left selected = purse_demand regions right selected.
Proof.
  intros regions left right selected extension.
  induction regions as [| current rest IH].
  - reflexivity.
  - simpl.
    rewrite (extension current (or_introl eq_refl)).
    destruct (purse_eq_dec (right current) selected).
    + f_equal.
      apply IH.
      intros nested nested_in.
      apply extension.
      now right.
    + apply IH.
      intros nested nested_in.
      apply extension.
      now right.
Qed.

Theorem replay_preserves_purse_debit : forall balance play replay,
  replay_refines play replay ->
  forall selected,
    event_debit balance play selected =
    event_debit balance replay selected.
Proof.
  intros balance play replay [same_regions same_purses] selected.
  unfold event_debit, debit.
  rewrite same_regions.
  f_equal.
  apply purse_demand_plan_ext.
  intros current current_in.
  apply same_purses.
  now rewrite same_regions.
Qed.

Theorem continuation_requires_outer_event : forall outer continuation committed,
  dependencies_committed [outer] committed ->
  In continuation committed ->
  In outer committed.
Proof.
  intros outer continuation committed dependencies committed_continuation.
  apply dependencies.
  now left.
Qed.

Theorem cross_deploy_slot_identity_is_replay_stable :
  forall play replay first_region later_region shared_purse,
    replay_refines play replay ->
    In first_region (located_regions play) ->
    In later_region (located_regions play) ->
    located_purses play first_region = shared_purse ->
    located_purses play later_region = shared_purse ->
    located_purses replay first_region = shared_purse /\
    located_purses replay later_region = shared_purse.
Proof.
  intros play replay first_region later_region shared_purse
    [same_regions same_purses] first_in later_in first_slot later_slot.
  split.
  - rewrite <- (same_purses first_region first_in).
    exact first_slot.
  - rewrite <- (same_purses later_region later_in).
    exact later_slot.
Qed.

Theorem atomic_join_debits_all_or_none :
  forall balance plan decision,
    reserve_or_reject
      balance
      (located_regions plan)
      (located_purses plan)
      decision
      = (balance, false)
    \/
    reserve_or_reject
      balance
      (located_regions plan)
      (located_purses plan)
      decision
      = (event_debit balance plan, true).
Proof.
  intros balance plan decision.
  unfold reserve_or_reject, event_debit.
  destruct decision.
  - now right.
  - now left.
Qed.

(* G3 (DR-135): a run of events. An event is its region list and its
   region-to-purse plan. The run decides each event with can_debit_dec at
   the balance that the earlier events left, in the order of the critical
   sections of play (v2 section 4.4 item 1). *)

Definition run_event_plan := (list region * region_purse_plan)%type.

Context (can_debit_dec :
  forall balance regions plan,
    {can_debit balance regions plan} + {~ can_debit balance regions plan}).

Definition run_event
  (balance : purse_balance)
  (event : run_event_plan)
  : purse_balance * bool :=
  reserve_or_reject balance (fst event) (snd event)
    (can_debit_dec balance (fst event) (snd event)).

Fixpoint run_events
  (balance : purse_balance)
  (events : list run_event_plan)
  : purse_balance :=
  match events with
  | [] => balance
  | event :: rest => run_events (fst (run_event balance event)) rest
  end.

(* The demand of the fired events on one purse. *)
Fixpoint run_demand
  (balance : purse_balance)
  (events : list run_event_plan)
  (selected : purse)
  : nat :=
  match events with
  | [] => 0
  | event :: rest =>
      (if snd (run_event balance event)
       then purse_demand (fst event) (snd event) selected
       else 0)
      + run_demand (fst (run_event balance event)) rest selected
  end.

Theorem run_debits_within_initial_balance :
  forall events balance selected,
    run_events balance events selected + run_demand balance events selected
      = balance selected /\
    run_demand balance events selected <= balance selected.
Proof.
  induction events as [| [regions plan] rest IH]; intros balance selected.
  - simpl. split; lia.
  - cbn [run_events run_demand].
    unfold run_event, reserve_or_reject; cbn [fst snd].
    destruct (can_debit_dec balance regions plan) as [funded | underfunded].
    + cbn [fst snd].
      destruct (IH (debit balance regions plan) selected) as [conserved bounded].
      pose proof (funded selected) as fits.
      assert (step : debit balance regions plan selected
                     = balance selected - purse_demand regions plan selected)
        by reflexivity.
      rewrite step in conserved, bounded.
      split; lia.
    + cbn [fst snd].
      exact (IH balance selected).
Qed.

(* The run without the gate: every event debits its plan. *)
Fixpoint ungated_run_events
  (balance : purse_balance)
  (events : list run_event_plan)
  : purse_balance :=
  match events with
  | [] => balance
  | event :: rest => ungated_run_events (debit balance (fst event) (snd event)) rest
  end.

Fixpoint ungated_demand
  (events : list run_event_plan)
  (selected : purse)
  : nat :=
  match events with
  | [] => 0
  | event :: rest => purse_demand (fst event) (snd event) selected + ungated_demand rest selected
  end.

End LocatedAuthoritySettlement.

(* G3 (DR-135) negative control: two events that each draw one unit of the
   same purse of budget one. Without the gate the run draws two units, and
   the conclusion of run_debits_within_initial_balance fails: the final
   balance (truncated at zero) plus the demand is two, not one. *)

Definition unit_purse_eq_dec :
  forall left right : unit, {left = right} + {left <> right}.
Proof.
  intros [] [].
  left.
  reflexivity.
Defined.

Definition unit_owner_region : @region unit unit :=
  {| region_signature := tt; region_location := tt |}.

Definition unit_owner_draw : @run_event_plan unit unit unit :=
  ([unit_owner_region], fun _ => tt).

Theorem nc_ungated_run_overdraws :
  let budget := fun _ : unit => 1 in
  let events := [unit_owner_draw; unit_owner_draw] in
  ungated_demand unit_purse_eq_dec events tt = 2 /\
  budget tt < ungated_demand unit_purse_eq_dec events tt /\
  ~ (ungated_run_events unit_purse_eq_dec budget events tt
       + ungated_demand unit_purse_eq_dec events tt = budget tt /\
     ungated_demand unit_purse_eq_dec events tt <= budget tt).
Proof.
  cbv beta zeta.
  assert (two : ungated_demand unit_purse_eq_dec
                  [unit_owner_draw; unit_owner_draw] tt = 2)
    by reflexivity.
  assert (zero : ungated_run_events unit_purse_eq_dec (fun _ : unit => 1)
                   [unit_owner_draw; unit_owner_draw] tt = 0)
    by reflexivity.
  rewrite two, zero.
  split; [reflexivity |].
  split; [lia |].
  intros [conserved _].
  lia.
Qed.

(* G3 step S11 (DR-135 item 13): the caller reserves only its own work.

   The native meter of a caller's run reserves each charge against the
   signed limit (reserve_with_owner_usage in native_phlo_rules/execution.rs).
   A charge is a list of shares, one for each region. The total usage
   always grows by the whole charge. A limit rule names the shares that
   count against the limit. The S11 rule counts every share except an
   owner share at a price of 1 or more: the shares of client regions and
   the owner shares at price 0 (S11 decision 2). A priced owner share
   counts only against the budget of its owner, which
   run_debits_within_initial_balance bounds. A charge whose counted part
   does not fit the limit is denied and changes nothing, and a denied
   charge ends the run (DR-113). *)

Record meter_share := {
  share_usage : nat;
  share_is_owner : bool;
  share_price : nat
}.

(* An owner share at a price of 1 or more. *)
Definition priced_owner_share (share : meter_share) : bool :=
  share_is_owner share && Nat.ltb 0 (share_price share).

(* The share of a client region: the caller's own work. *)
Definition client_share (share : meter_share) : bool :=
  negb (share_is_owner share).

(* An owner share at price 0. *)
Definition zero_priced_owner_share (share : meter_share) : bool :=
  share_is_owner share && Nat.eqb (share_price share) 0.

Definition limit_rule := meter_share -> bool.

(* The S11 rule: every share except a priced owner share. *)
Definition s11_limit_rule : limit_rule :=
  fun share => negb (priced_owner_share share).

(* The rule before S11, for the first control: every share. *)
Definition whole_run_limit_rule : limit_rule := fun _ => true.

(* A rule that also drops the owner work at price 0, for the second
   control: the client shares only. *)
Definition client_only_limit_rule : limit_rule := client_share.

Definition charge_usage (charge : list meter_share) : nat :=
  fold_right (fun share total => share_usage share + total) 0 charge.

(* The usage of the shares that a rule selects. *)
Definition counted_usage
  (rule : limit_rule)
  (charge : list meter_share) : nat :=
  fold_right
    (fun share total => (if rule share then share_usage share else 0) + total)
    0
    charge.

Record caller_meter := {
  caller_meter_used : nat;
  caller_meter_client_used : nat
}.

Definition empty_caller_meter : caller_meter :=
  {| caller_meter_used := 0; caller_meter_client_used := 0 |}.

Definition reserve_caller_charge
  (rule : limit_rule)
  (limit : nat)
  (meter : caller_meter)
  (charge : list meter_share)
  : option caller_meter :=
  if Nat.leb (caller_meter_client_used meter + counted_usage rule charge) limit
  then
    Some {|
      caller_meter_used := caller_meter_used meter + charge_usage charge;
      caller_meter_client_used :=
        caller_meter_client_used meter + counted_usage rule charge
    |}
  else None.

Fixpoint caller_meter_run
  (rule : limit_rule)
  (limit : nat)
  (meter : caller_meter)
  (charges : list (list meter_share))
  : option caller_meter :=
  match charges with
  | [] => Some meter
  | charge :: rest =>
      match reserve_caller_charge rule limit meter charge with
      | Some next => caller_meter_run rule limit next rest
      | None => None
      end
  end.

Definition caller_run_usage (charges : list (list meter_share)) : nat :=
  fold_right (fun charge total => charge_usage charge + total) 0 charges.

Definition caller_run_counted_usage
  (rule : limit_rule)
  (charges : list (list meter_share)) : nat :=
  fold_right (fun charge total => counted_usage rule charge + total) 0 charges.

Lemma caller_meter_run_totals :
  forall rule limit charges meter final,
    caller_meter_run rule limit meter charges = Some final ->
    caller_meter_used final
      = caller_meter_used meter + caller_run_usage charges /\
    caller_meter_client_used final
      = caller_meter_client_used meter
        + caller_run_counted_usage rule charges /\
    (caller_meter_client_used meter <= limit ->
     caller_meter_client_used final <= limit).
Proof.
  intros rule limit charges.
  induction charges as [| charge rest IH]; intros meter final result.
  - cbn [caller_meter_run] in result.
    inversion result; subst; clear result.
    cbn [caller_run_usage caller_run_counted_usage fold_right].
    repeat split; lia.
  - cbn [caller_meter_run] in result.
    unfold reserve_caller_charge in result.
    destruct (Nat.leb (caller_meter_client_used meter
                         + counted_usage rule charge) limit)
      eqn:fits; try discriminate.
    apply Nat.leb_le in fits.
    cbn beta iota in result.
    destruct (IH _ final result) as [used_total [client_total client_bound]].
    cbn [caller_meter_used caller_meter_client_used] in used_total,
      client_total, client_bound.
    cbn [caller_run_usage caller_run_counted_usage fold_right].
    unfold caller_run_usage, caller_run_counted_usage
      in used_total, client_total.
    split; [lia |].
    split; [lia |].
    intros _.
    apply client_bound.
    exact fits.
Qed.

(* The counted part of a successful run is within the limit. *)
Theorem caller_run_client_part_within_limit :
  forall limit charges final,
    caller_meter_run s11_limit_rule limit empty_caller_meter charges
      = Some final ->
    caller_meter_client_used final
      = caller_run_counted_usage s11_limit_rule charges /\
    caller_meter_used final = caller_run_usage charges /\
    caller_run_counted_usage s11_limit_rule charges <= limit.
Proof.
  intros limit charges final result.
  destruct (caller_meter_run_totals s11_limit_rule limit charges
              empty_caller_meter final result)
    as [used_total [client_total client_bound]].
  assert (used_zero : caller_meter_used empty_caller_meter = 0)
    by reflexivity.
  assert (client_zero : caller_meter_client_used empty_caller_meter = 0)
    by reflexivity.
  assert (bounded : caller_meter_client_used final <= limit).
  { apply client_bound.
    rewrite client_zero.
    apply Nat.le_0_l. }
  split; [lia |].
  split; lia.
Qed.

(* A priced owner share counts nothing against the limit. *)
Lemma priced_owner_shares_count_nothing :
  forall extra charge,
    Forall (fun share => priced_owner_share share = true) extra ->
    counted_usage s11_limit_rule (extra ++ charge)
      = counted_usage s11_limit_rule charge.
Proof.
  intros extra charge priced.
  induction priced as [| share rest share_priced _ IH].
  - reflexivity.
  - assert (zero :
      (if s11_limit_rule share then share_usage share else 0) = 0).
    { unfold s11_limit_rule.
      rewrite share_priced.
      reflexivity. }
    change ((if s11_limit_rule share then share_usage share else 0)
              + counted_usage s11_limit_rule (rest ++ charge)
            = counted_usage s11_limit_rule charge).
    rewrite zero.
    exact IH.
Qed.

Lemma caller_meter_run_depends_on_counted_usage :
  forall limit left right left_meter right_meter,
    caller_meter_client_used left_meter
      = caller_meter_client_used right_meter ->
    map (counted_usage s11_limit_rule) left
      = map (counted_usage s11_limit_rule) right ->
    option_map caller_meter_client_used
      (caller_meter_run s11_limit_rule limit left_meter left)
    = option_map caller_meter_client_used
        (caller_meter_run s11_limit_rule limit right_meter right).
Proof.
  intros limit left.
  induction left as [| left_charge left_rest IH];
    intros right left_meter right_meter same_client same_counts;
    destruct right as [| right_charge right_rest];
    cbn [map] in same_counts; try discriminate.
  - cbn [caller_meter_run option_map].
    rewrite same_client.
    reflexivity.
  - injection same_counts as same_charge same_rest.
    cbn [caller_meter_run].
    unfold reserve_caller_charge.
    rewrite same_client, same_charge.
    destruct (Nat.leb (caller_meter_client_used right_meter
                         + counted_usage s11_limit_rule right_charge) limit).
    + cbn beta iota.
      apply IH.
      * reflexivity.
      * exact same_rest.
    + reflexivity.
Qed.

(* The decision of the meter and the counted part never depend on the
   priced owner work: two runs whose charges count the same usage under
   the S11 rule both succeed with the same counted part or both end
   denied, whatever priced owner shares they hold. *)
Theorem caller_run_ignores_priced_owner_usage :
  forall limit left right,
    map (counted_usage s11_limit_rule) left
      = map (counted_usage s11_limit_rule) right ->
    option_map caller_meter_client_used
      (caller_meter_run s11_limit_rule limit empty_caller_meter left)
    = option_map caller_meter_client_used
        (caller_meter_run s11_limit_rule limit empty_caller_meter right).
Proof.
  intros limit left right same_counts.
  apply caller_meter_run_depends_on_counted_usage.
  - reflexivity.
  - exact same_counts.
Qed.

Lemma share_usage_splits :
  forall share,
    share_usage share
      = (if client_share share then share_usage share else 0)
        + (if zero_priced_owner_share share then share_usage share else 0)
        + (if priced_owner_share share then share_usage share else 0) /\
    (if s11_limit_rule share then share_usage share else 0)
      = (if client_share share then share_usage share else 0)
        + (if zero_priced_owner_share share then share_usage share else 0).
Proof.
  intros [usage owner price].
  unfold client_share, zero_priced_owner_share, priced_owner_share,
    s11_limit_rule.
  cbn [share_usage share_is_owner share_price].
  destruct owner; destruct price as [| price]; cbn; lia.
Qed.

Lemma charge_usage_splits :
  forall charge,
    charge_usage charge
      = counted_usage client_share charge
        + counted_usage zero_priced_owner_share charge
        + counted_usage priced_owner_share charge /\
    counted_usage s11_limit_rule charge
      = counted_usage client_share charge
        + counted_usage zero_priced_owner_share charge.
Proof.
  induction charge as [| share rest [usage_rest counted_rest]].
  - cbn. lia.
  - destruct (share_usage_splits share) as [usage_share counted_share].
    unfold charge_usage, counted_usage in *.
    cbn [fold_right].
    split; lia.
Qed.

Lemma caller_run_usage_splits :
  forall charges,
    caller_run_usage charges
      = caller_run_counted_usage client_share charges
        + caller_run_counted_usage zero_priced_owner_share charges
        + caller_run_counted_usage priced_owner_share charges /\
    caller_run_counted_usage s11_limit_rule charges
      = caller_run_counted_usage client_share charges
        + caller_run_counted_usage zero_priced_owner_share charges.
Proof.
  induction charges as [| charge rest [usage_rest counted_rest]].
  - cbn. lia.
  - destruct (charge_usage_splits charge) as [usage_charge counted_charge].
    unfold caller_run_usage, caller_run_counted_usage in *.
    cbn [fold_right].
    split; lia.
Qed.

(* The total work of a successful run is the caller's own work, the owner
   work at price 0 and the priced owner work. The first two count for the
   caller and stay within its limit, so the total work never exceeds the
   limit plus the priced owner work, which the owner budgets bound. *)
Theorem caller_run_total_work_bounded :
  forall limit charges final,
    caller_meter_run s11_limit_rule limit empty_caller_meter charges
      = Some final ->
    caller_meter_used final
      = caller_run_counted_usage client_share charges
        + caller_run_counted_usage zero_priced_owner_share charges
        + caller_run_counted_usage priced_owner_share charges /\
    caller_run_counted_usage client_share charges
      + caller_run_counted_usage zero_priced_owner_share charges <= limit /\
    caller_meter_used final
      <= limit + caller_run_counted_usage priced_owner_share charges.
Proof.
  intros limit charges final result.
  destruct (caller_run_client_part_within_limit limit charges final result)
    as [_ [used_total within]].
  destruct (caller_run_usage_splits charges) as [usage_split counted_split].
  rewrite counted_split in within.
  split; [lia |].
  split; lia.
Qed.

(* Shares for the controls. *)
Definition client_unit_share (usage : nat) : meter_share :=
  {| share_usage := usage; share_is_owner := false; share_price := 1 |}.

Definition priced_owner_unit_share (usage : nat) : meter_share :=
  {| share_usage := usage; share_is_owner := true; share_price := 1 |}.

Definition zero_priced_owner_unit_share (usage : nat) : meter_share :=
  {| share_usage := usage; share_is_owner := true; share_price := 0 |}.

(* Control 1: a fee-only caller with limit 1 runs a charge with its own
   work 1 and a priced owner share 3. The S11 rule grants it, with total
   work 4. The rule before S11 counts the owner share against the limit
   and denies the run. *)
Theorem nc_counting_priced_owner_usage_refuses_a_fee_only_caller :
  let charges := [[client_unit_share 1; priced_owner_unit_share 3]] in
  caller_meter_run s11_limit_rule 1 empty_caller_meter charges
    = Some {| caller_meter_used := 4; caller_meter_client_used := 1 |} /\
  caller_meter_run whole_run_limit_rule 1 empty_caller_meter charges = None.
Proof.
  cbv zeta.
  split; reflexivity.
Qed.

(* Control 2: a caller with limit 1 runs a charge with its own work 1 and
   an owner share 1 at price 0. The S11 rule denies the run, because the
   owner work at price 0 counts for the caller. A rule that drops that
   work grants the run, and the conclusion of caller_run_total_work_bounded
   fails for it: the caller's own work and the owner work at price 0 are 2,
   above the limit 1. *)
Theorem nc_uncounted_zero_price_work_exceeds_the_bound :
  let charges := [[client_unit_share 1; zero_priced_owner_unit_share 1]] in
  caller_meter_run s11_limit_rule 1 empty_caller_meter charges = None /\
  caller_meter_run client_only_limit_rule 1 empty_caller_meter charges
    = Some {| caller_meter_used := 2; caller_meter_client_used := 1 |} /\
  1 < caller_run_counted_usage client_share charges
        + caller_run_counted_usage zero_priced_owner_share charges.
Proof.
  cbv zeta.
  split; [reflexivity |].
  split; [reflexivity |].
  cbv.
  lia.
Qed.
