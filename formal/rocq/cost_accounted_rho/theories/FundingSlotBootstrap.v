(* G3 (DR-135, design v2 rules R1 to R5; DR-68 items 1 to 3): the
   installer's share at activation.

   The installed trigger is the stored receive of the installer. Its outer
   layer carries the installer's region (uniform signing, DR-68), as in the
   gateway form {for (...) body}[installer -o slot] of the user's decision of
   2026-10-19 (P1 def:funding-slot, def:sugar-lollipop). When the gateway
   activates the trigger, the installer pays the share of its stored receive
   as an owner, from its own purse, at its own price
   (installer_share usage price = usage * price). The slot pays the body
   and the outer purse pays its realized cost, as in
   activate_funded_lollipop. If the installer purse cannot pay the share,
   the activation does not fire and has no charge, and play records a
   refusal (Option 2).

   Added by G3, with no axiom:
   - installer_share_activation_is_exact_and_conserving: a fired activation
     debits the outer cost, the slot cost and the installer share from their
     three purses, burns all three, and conserves the accounted value.
   - installer_share_never_overdraws: a fired activation had the share in
     the installer purse.
   - unpaid_installer_share_refuses_activation: when the located activation
     is admissible and the installer purse cannot pay the share, the outcome
     is a refusal.
   - refused_activation_keeps_state: a refusal and a non-admissible
     activation leave the state unchanged.
   - nc_ignored_installer_share_runs_free: a proved negative control. The
     activation of activate_funded_lollipop alone ignores the share: in a
     state where the owner rule refuses, it fires and charges the installer
     nothing.

   Added by G3 step S11 (DR-135 item 13), with no axiom:
   - sponsored_activation_needs_only_caller_work: when the caller's client
     part (its own work and the owner work at price 0) fits its limit and
     its purse covers its hold, the sponsored activation has the outcome
     of the installer-share activation for every amount of priced owner
     work, and the caller pays only its own work, within its hold.
   - nc_whole_run_limit_refuses_a_fee_only_caller: a proved negative
     control. The rule before S11 counts the whole run against the limit,
     so a fee-only caller that the S11 rule runs exhausts its limit.

   Formulations rejected before any proof attempt (reasoned):
   - An option result cannot tell a refusal from a non-admissible
     activation. Play records only the refusal, so the outcome type has
     three cases.
   - A caller field in funding_slot_bootstrap_state would change the
     statements of all the theorems about the record (S11). The caller's
     purse is a separate number instead. *)
From Stdlib Require Import Arith.PeanoNat Bool.Bool Lia.

Record funding_slot_bootstrap_state : Type := {
  bootstrap_installer_balance : nat;
  bootstrap_sponsor_balance : nat;
  bootstrap_outer_balance : nat;
  bootstrap_slot_balance : nat;
  bootstrap_burned : nat
}.

Record funding_slot_registry : Type := {
  bootstrap_outer_exists : bool;
  bootstrap_slot_exists : bool
}.

Definition commit_funding_registry
  (accepted : bool)
  (registry : funding_slot_registry) : funding_slot_registry :=
  if accepted then
    {| bootstrap_outer_exists := true;
       bootstrap_slot_exists := true |}
  else registry.

Definition unsafe_precreate_outer
  (registry : funding_slot_registry) : funding_slot_registry :=
  {| bootstrap_outer_exists := true;
     bootstrap_slot_exists := bootstrap_slot_exists registry |}.

Definition bootstrap_accounted_value
  (state : funding_slot_bootstrap_state) : nat :=
  bootstrap_installer_balance state +
  bootstrap_sponsor_balance state +
  bootstrap_outer_balance state +
  bootstrap_slot_balance state +
  bootstrap_burned state.

Definition install_scaffold
  (cost : nat)
  (state : funding_slot_bootstrap_state)
  : option funding_slot_bootstrap_state :=
  if Nat.leb cost (bootstrap_installer_balance state) then
    Some {|
      bootstrap_installer_balance :=
        bootstrap_installer_balance state - cost;
      bootstrap_sponsor_balance := bootstrap_sponsor_balance state;
      bootstrap_outer_balance := bootstrap_outer_balance state;
      bootstrap_slot_balance := bootstrap_slot_balance state;
      bootstrap_burned := bootstrap_burned state + cost
    |}
  else None.

Definition eager_located_install_admissible
  (scaffold_cost outer_bound slot_bound : nat)
  (state : funding_slot_bootstrap_state) : bool :=
  Nat.leb scaffold_cost (bootstrap_installer_balance state) &&
  Nat.leb outer_bound (bootstrap_outer_balance state) &&
  Nat.leb slot_bound (bootstrap_slot_balance state).

Definition fund_bootstrap_purses
  (outer_amount slot_amount : nat)
  (state : funding_slot_bootstrap_state)
  : option funding_slot_bootstrap_state :=
  if Nat.leb
      (outer_amount + slot_amount)
      (bootstrap_sponsor_balance state) then
    Some {|
      bootstrap_installer_balance := bootstrap_installer_balance state;
      bootstrap_sponsor_balance :=
        bootstrap_sponsor_balance state - outer_amount - slot_amount;
      bootstrap_outer_balance :=
        bootstrap_outer_balance state + outer_amount;
      bootstrap_slot_balance :=
        bootstrap_slot_balance state + slot_amount;
      bootstrap_burned := bootstrap_burned state
    |}
  else None.

Definition fund_only_slot
  (slot_amount : nat)
  (state : funding_slot_bootstrap_state)
  : option funding_slot_bootstrap_state :=
  if Nat.leb slot_amount (bootstrap_sponsor_balance state) then
    Some {|
      bootstrap_installer_balance := bootstrap_installer_balance state;
      bootstrap_sponsor_balance :=
        bootstrap_sponsor_balance state - slot_amount;
      bootstrap_outer_balance := bootstrap_outer_balance state;
      bootstrap_slot_balance := bootstrap_slot_balance state + slot_amount;
      bootstrap_burned := bootstrap_burned state
    |}
  else None.

Definition bootstrap_locally_sufficient
  (outer_bound slot_bound : nat)
  (state : funding_slot_bootstrap_state) : bool :=
  Nat.leb outer_bound (bootstrap_outer_balance state) &&
  Nat.leb slot_bound (bootstrap_slot_balance state).

Definition activate_funded_lollipop
  (gateway_authenticated : bool)
  (outer_bound slot_bound outer_cost slot_cost : nat)
  (state : funding_slot_bootstrap_state)
  : option funding_slot_bootstrap_state :=
  if gateway_authenticated &&
      bootstrap_locally_sufficient outer_bound slot_bound state &&
      Nat.leb outer_cost outer_bound &&
      Nat.leb slot_cost slot_bound then
    Some {|
      bootstrap_installer_balance := bootstrap_installer_balance state;
      bootstrap_sponsor_balance := bootstrap_sponsor_balance state;
      bootstrap_outer_balance := bootstrap_outer_balance state - outer_cost;
      bootstrap_slot_balance := bootstrap_slot_balance state - slot_cost;
      bootstrap_burned := bootstrap_burned state + outer_cost + slot_cost
    |}
  else None.

Theorem scaffold_install_is_conserving :
  forall state cost installed,
    install_scaffold cost state = Some installed ->
    bootstrap_accounted_value installed = bootstrap_accounted_value state.
Proof.
  intros state cost installed result.
  unfold install_scaffold in result.
  destruct (Nat.leb cost (bootstrap_installer_balance state))
    eqn:sufficient; try discriminate.
  apply Nat.leb_le in sufficient.
  inversion result; subst; clear result.
  unfold bootstrap_accounted_value; simpl; lia.
Qed.

Theorem eager_located_install_rejects_new_zero_purses :
  forall state scaffold_cost outer_bound slot_bound,
    bootstrap_installer_balance state >= scaffold_cost ->
    bootstrap_outer_balance state = 0 ->
    bootstrap_slot_balance state = 0 ->
    outer_bound > 0 ->
    slot_bound > 0 ->
    eager_located_install_admissible
      scaffold_cost outer_bound slot_bound state = false.
Proof.
  intros state scaffold_cost outer_bound slot_bound installer_funded
    outer_zero slot_zero outer_positive slot_positive.
  unfold eager_located_install_admissible.
  assert (installer_check :
    Nat.leb scaffold_cost (bootstrap_installer_balance state) = true).
  { apply Nat.leb_le. exact installer_funded. }
  rewrite installer_check.
  rewrite outer_zero, slot_zero.
  assert (outer_check : Nat.leb outer_bound 0 = false).
  { apply Nat.leb_gt. lia. }
  rewrite outer_check.
  reflexivity.
Qed.

Theorem staged_scaffold_install_needs_no_candidate_purse_supply :
  forall state scaffold_cost,
    bootstrap_installer_balance state >= scaffold_cost ->
    exists installed, install_scaffold scaffold_cost state = Some installed.
Proof.
  intros state scaffold_cost sufficient.
  unfold install_scaffold.
  assert (check :
    Nat.leb scaffold_cost (bootstrap_installer_balance state) = true).
  { apply Nat.leb_le. exact sufficient. }
  rewrite check.
  eexists; reflexivity.
Qed.

Theorem dual_purse_funding_is_exact_and_conserving :
  forall state funded outer_amount slot_amount,
    fund_bootstrap_purses outer_amount slot_amount state = Some funded ->
    bootstrap_sponsor_balance funded + outer_amount + slot_amount =
      bootstrap_sponsor_balance state /\
    bootstrap_outer_balance funded =
      bootstrap_outer_balance state + outer_amount /\
    bootstrap_slot_balance funded =
      bootstrap_slot_balance state + slot_amount /\
    bootstrap_accounted_value funded = bootstrap_accounted_value state.
Proof.
  intros state funded outer_amount slot_amount result.
  unfold fund_bootstrap_purses in result.
  destruct (Nat.leb
    (outer_amount + slot_amount)
    (bootstrap_sponsor_balance state)) eqn:sufficient; try discriminate.
  apply Nat.leb_le in sufficient.
  inversion result; subst; clear result.
  unfold bootstrap_accounted_value; simpl.
  repeat split; lia.
Qed.

Theorem insufficient_dual_purse_funding_is_atomic :
  forall state outer_amount slot_amount,
    bootstrap_sponsor_balance state < outer_amount + slot_amount ->
    fund_bootstrap_purses outer_amount slot_amount state = None.
Proof.
  intros state outer_amount slot_amount insufficient.
  unfold fund_bootstrap_purses.
  assert (check :
    Nat.leb (outer_amount + slot_amount)
      (bootstrap_sponsor_balance state) = false).
  { apply Nat.leb_gt. exact insufficient. }
  rewrite check.
  reflexivity.
Qed.

Theorem rejected_dual_purse_funding_preserves_registry :
  forall registry,
    commit_funding_registry false registry = registry.
Proof.
  reflexivity.
Qed.

Theorem accepted_dual_purse_funding_creates_both_vaults :
  forall registry,
    bootstrap_outer_exists (commit_funding_registry true registry) = true /\
    bootstrap_slot_exists (commit_funding_registry true registry) = true.
Proof.
  intros registry.
  split; reflexivity.
Qed.

Theorem eager_target_creation_breaks_rejection_atomicity :
  let empty :=
    {| bootstrap_outer_exists := false;
       bootstrap_slot_exists := false |} in
  unsafe_precreate_outer empty <> empty.
Proof.
  simpl.
  discriminate.
Qed.

Theorem slot_only_funding_cannot_satisfy_positive_outer_bound :
  forall state funded slot_amount outer_bound slot_bound,
    bootstrap_outer_balance state = 0 ->
    outer_bound > 0 ->
    fund_only_slot slot_amount state = Some funded ->
    bootstrap_locally_sufficient outer_bound slot_bound funded = false.
Proof.
  intros state funded slot_amount outer_bound slot_bound outer_zero
    outer_positive result.
  unfold fund_only_slot in result.
  destruct (Nat.leb slot_amount (bootstrap_sponsor_balance state))
    eqn:sufficient; try discriminate.
  inversion result; subst; clear result.
  unfold bootstrap_locally_sufficient; simpl.
  rewrite outer_zero.
  assert (outer_check : Nat.leb outer_bound 0 = false).
  { apply Nat.leb_gt. lia. }
  rewrite outer_check.
  reflexivity.
Qed.

Theorem dual_funding_establishes_local_sufficiency :
  forall state funded outer_amount slot_amount outer_bound slot_bound,
    outer_bound <= bootstrap_outer_balance state + outer_amount ->
    slot_bound <= bootstrap_slot_balance state + slot_amount ->
    fund_bootstrap_purses outer_amount slot_amount state = Some funded ->
    bootstrap_locally_sufficient outer_bound slot_bound funded = true.
Proof.
  intros state funded outer_amount slot_amount outer_bound slot_bound
    outer_sufficient slot_sufficient result.
  unfold fund_bootstrap_purses in result.
  destruct (Nat.leb
    (outer_amount + slot_amount)
    (bootstrap_sponsor_balance state)) eqn:sponsor_sufficient;
    try discriminate.
  inversion result; subst; clear result.
  unfold bootstrap_locally_sufficient; simpl.
  assert (outer_check :
    Nat.leb outer_bound
      (bootstrap_outer_balance state + outer_amount) = true).
  { apply Nat.leb_le. exact outer_sufficient. }
  assert (slot_check :
    Nat.leb slot_bound
      (bootstrap_slot_balance state + slot_amount) = true).
  { apply Nat.leb_le. exact slot_sufficient. }
  rewrite outer_check, slot_check.
  reflexivity.
Qed.

Theorem activation_requires_gateway_authentication :
  forall state outer_bound slot_bound outer_cost slot_cost,
    activate_funded_lollipop false outer_bound slot_bound
      outer_cost slot_cost state = None.
Proof.
  reflexivity.
Qed.

Theorem activation_requires_both_located_purses :
  forall state outer_bound slot_bound outer_cost slot_cost,
    bootstrap_locally_sufficient outer_bound slot_bound state = false ->
    activate_funded_lollipop true outer_bound slot_bound
      outer_cost slot_cost state = None.
Proof.
  intros state outer_bound slot_bound outer_cost slot_cost insufficient.
  unfold activate_funded_lollipop.
  rewrite insufficient.
  reflexivity.
Qed.

Theorem activated_lollipop_settlement_is_exact_and_conserving :
  forall state settled outer_bound slot_bound outer_cost slot_cost,
    activate_funded_lollipop true outer_bound slot_bound
      outer_cost slot_cost state = Some settled ->
    bootstrap_outer_balance settled + outer_cost =
      bootstrap_outer_balance state /\
    bootstrap_slot_balance settled + slot_cost =
      bootstrap_slot_balance state /\
    bootstrap_burned settled =
      bootstrap_burned state + outer_cost + slot_cost /\
    bootstrap_accounted_value settled = bootstrap_accounted_value state.
Proof.
  intros state settled outer_bound slot_bound outer_cost slot_cost result.
  unfold activate_funded_lollipop in result.
  destruct (bootstrap_locally_sufficient outer_bound slot_bound state)
    eqn:locally_sufficient; simpl in result; try discriminate.
  destruct (Nat.leb outer_cost outer_bound) eqn:outer_bounded;
    try discriminate.
  destruct (Nat.leb slot_cost slot_bound) eqn:slot_bounded;
    try discriminate.
  unfold bootstrap_locally_sufficient in locally_sufficient.
  apply andb_true_iff in locally_sufficient as [outer_supply slot_supply].
  apply Nat.leb_le in outer_supply, slot_supply.
  apply Nat.leb_le in outer_bounded, slot_bounded.
  inversion result; subst; clear result.
  unfold bootstrap_accounted_value; simpl.
  repeat split; lia.
Qed.

(* G3 (DR-135): the installer's share at activation. *)

Inductive installer_activation : Type :=
| InstallerActivated (settled : funding_slot_bootstrap_state)
| InstallerShareRefused
| ActivationNotAdmissible.

(* The share of the installer's stored receive at the installer's own
   price (v2 R4). *)
Definition installer_share (usage price : nat) : nat := usage * price.

Definition activate_with_installer_share
  (gateway_authenticated : bool)
  (outer_bound slot_bound outer_cost slot_cost share : nat)
  (state : funding_slot_bootstrap_state)
  : installer_activation :=
  match activate_funded_lollipop gateway_authenticated outer_bound slot_bound
          outer_cost slot_cost state with
  | None => ActivationNotAdmissible
  | Some located =>
      if Nat.leb share (bootstrap_installer_balance state) then
        InstallerActivated {|
          bootstrap_installer_balance :=
            bootstrap_installer_balance located - share;
          bootstrap_sponsor_balance := bootstrap_sponsor_balance located;
          bootstrap_outer_balance := bootstrap_outer_balance located;
          bootstrap_slot_balance := bootstrap_slot_balance located;
          bootstrap_burned := bootstrap_burned located + share
        |}
      else InstallerShareRefused
  end.

(* The state after an activation outcome: a refusal and a non-admissible
   activation keep the state. *)
Definition state_after_activation
  (outcome : installer_activation)
  (state : funding_slot_bootstrap_state)
  : funding_slot_bootstrap_state :=
  match outcome with
  | InstallerActivated settled => settled
  | InstallerShareRefused => state
  | ActivationNotAdmissible => state
  end.

Lemma located_activation_keeps_installer :
  forall state located outer_bound slot_bound outer_cost slot_cost,
    activate_funded_lollipop true outer_bound slot_bound outer_cost slot_cost
      state = Some located ->
    bootstrap_installer_balance located = bootstrap_installer_balance state /\
    bootstrap_sponsor_balance located = bootstrap_sponsor_balance state.
Proof.
  intros state located outer_bound slot_bound outer_cost slot_cost result.
  unfold activate_funded_lollipop in result.
  destruct (bootstrap_locally_sufficient outer_bound slot_bound state);
    simpl in result; try discriminate.
  destruct (Nat.leb outer_cost outer_bound); try discriminate.
  destruct (Nat.leb slot_cost slot_bound); try discriminate.
  inversion result; subst; clear result.
  split; reflexivity.
Qed.

Theorem installer_share_activation_is_exact_and_conserving :
  forall state settled outer_bound slot_bound outer_cost slot_cost share,
    activate_with_installer_share true outer_bound slot_bound outer_cost
      slot_cost share state = InstallerActivated settled ->
    bootstrap_installer_balance settled + share =
      bootstrap_installer_balance state /\
    bootstrap_outer_balance settled + outer_cost =
      bootstrap_outer_balance state /\
    bootstrap_slot_balance settled + slot_cost =
      bootstrap_slot_balance state /\
    bootstrap_burned settled =
      bootstrap_burned state + outer_cost + slot_cost + share /\
    bootstrap_accounted_value settled = bootstrap_accounted_value state.
Proof.
  intros state settled outer_bound slot_bound outer_cost slot_cost share
    result.
  unfold activate_with_installer_share in result.
  destruct (activate_funded_lollipop true outer_bound slot_bound outer_cost
              slot_cost state) as [located |] eqn:located_result;
    try discriminate.
  destruct (Nat.leb share (bootstrap_installer_balance state))
    eqn:share_fits; try discriminate.
  apply Nat.leb_le in share_fits.
  destruct (located_activation_keeps_installer state located outer_bound
              slot_bound outer_cost slot_cost located_result)
    as [same_installer same_sponsor].
  destruct (activated_lollipop_settlement_is_exact_and_conserving state
              located outer_bound slot_bound outer_cost slot_cost
              located_result)
    as [outer_exact [slot_exact [burned_exact conserved]]].
  inversion result; subst; clear result.
  unfold bootstrap_accounted_value in *; simpl.
  repeat split; lia.
Qed.

Theorem installer_share_never_overdraws :
  forall state settled outer_bound slot_bound outer_cost slot_cost share,
    activate_with_installer_share true outer_bound slot_bound outer_cost
      slot_cost share state = InstallerActivated settled ->
    share <= bootstrap_installer_balance state.
Proof.
  intros state settled outer_bound slot_bound outer_cost slot_cost share
    result.
  unfold activate_with_installer_share in result.
  destruct (activate_funded_lollipop true outer_bound slot_bound outer_cost
              slot_cost state); try discriminate.
  destruct (Nat.leb share (bootstrap_installer_balance state))
    eqn:share_fits; try discriminate.
  now apply Nat.leb_le in share_fits.
Qed.

Theorem unpaid_installer_share_refuses_activation :
  forall state located outer_bound slot_bound outer_cost slot_cost share,
    activate_funded_lollipop true outer_bound slot_bound outer_cost slot_cost
      state = Some located ->
    bootstrap_installer_balance state < share ->
    activate_with_installer_share true outer_bound slot_bound outer_cost
      slot_cost share state = InstallerShareRefused.
Proof.
  intros state located outer_bound slot_bound outer_cost slot_cost share
    located_result unpaid.
  unfold activate_with_installer_share.
  rewrite located_result.
  assert (check : Nat.leb share (bootstrap_installer_balance state) = false).
  { apply Nat.leb_gt. exact unpaid. }
  rewrite check.
  reflexivity.
Qed.

Theorem refused_activation_keeps_state :
  forall outcome state,
    outcome = InstallerShareRefused \/ outcome = ActivationNotAdmissible ->
    state_after_activation outcome state = state.
Proof.
  intros outcome state [refused | rejected]; subst; reflexivity.
Qed.

(* The control: a state after installation and funding in which the
   installer purse holds 3 and the share is 4. *)
Definition installer_short_state : funding_slot_bootstrap_state := {|
  bootstrap_installer_balance := 3;
  bootstrap_sponsor_balance := 2;
  bootstrap_outer_balance := 3;
  bootstrap_slot_balance := 3;
  bootstrap_burned := 1
|}.

(* The owner rule refuses the activation, because the installer purse holds
   3 and the share is 4. The rule that ignores the share fires in the same
   state. It leaves the installer purse untouched and burns only the outer
   and slot costs, so the installer's stored receive runs free. *)
Theorem nc_ignored_installer_share_runs_free :
  let share := installer_share 2 2 in
  activate_with_installer_share true 2 2 1 1 share installer_short_state
    = InstallerShareRefused /\
  exists located,
    activate_funded_lollipop true 2 2 1 1 installer_short_state = Some located /\
    bootstrap_installer_balance located
      = bootstrap_installer_balance installer_short_state /\
    bootstrap_burned located = bootstrap_burned installer_short_state + 1 + 1 /\
    0 < share.
Proof.
  cbv zeta.
  split.
  - reflexivity.
  - exists {|
      bootstrap_installer_balance := 3;
      bootstrap_sponsor_balance := 2;
      bootstrap_outer_balance := 2;
      bootstrap_slot_balance := 2;
      bootstrap_burned := 3
    |}.
    split; [reflexivity |].
    split; [reflexivity |].
    split; [reflexivity |].
    unfold installer_share.
    lia.
Qed.

(* G3 step S11 (DR-135 item 13): the caller reserves only its own work.

   The caller's send activates the trigger. Its signed limit and its hold
   before execution cover only the client part of its run: its own work
   and the owner work at price 0 (S11 decision 2). The priced owner work,
   the installer share and the located body, counts only against the
   owners' budgets. Who pays does not change: the caller pays its own work
   at its own price, and the owners pay their shares. The caller's purse
   is a separate number, so funding_slot_bootstrap_state does not change. *)

Record sponsored_caller : Type := {
  caller_own_usage : nat;
  caller_price : nat;
  caller_limit : nat;
  caller_zero_priced_owner_usage : nat;
  caller_priced_owner_usage : nat
}.

(* The client part: the work that the caller's limit covers. *)
Definition caller_client_part (caller : sponsored_caller) : nat :=
  caller_own_usage caller + caller_zero_priced_owner_usage caller.

(* The caller's hold: its limit at its price. The hold formula does not
   change. *)
Definition caller_hold (caller : sponsored_caller) : nat :=
  caller_limit caller * caller_price caller.

(* The caller's charge: its own work at its own price. *)
Definition caller_charge (caller : sponsored_caller) : nat :=
  caller_own_usage caller * caller_price caller.

(* The work that the meter counts against the caller's limit. The S11 rule
   is count_priced_owner_work = false. The rule before S11, which the
   control uses, is count_priced_owner_work = true. *)
Definition caller_bound_usage
  (count_priced_owner_work : bool)
  (caller : sponsored_caller) : nat :=
  caller_client_part caller +
  (if count_priced_owner_work then caller_priced_owner_usage caller else 0).

(* The outcome of the caller's run: the activation outcome with the
   caller's balance after its charge, or the exhaustion of its limit
   (DR-113), after which nothing fires. *)
Inductive sponsored_activation : Type :=
| SponsoredRun (outcome : installer_activation) (caller_balance : nat)
| CallerLimitExhausted.

Definition activate_with_caller_bound
  (count_priced_owner_work : bool)
  (caller : sponsored_caller)
  (caller_balance : nat)
  (gateway_authenticated : bool)
  (outer_bound slot_bound outer_cost slot_cost share : nat)
  (state : funding_slot_bootstrap_state)
  : sponsored_activation :=
  if Nat.leb (caller_bound_usage count_priced_owner_work caller)
       (caller_limit caller) then
    SponsoredRun
      (activate_with_installer_share gateway_authenticated outer_bound
         slot_bound outer_cost slot_cost share state)
      (caller_balance - caller_charge caller)
  else CallerLimitExhausted.

(* The S11 rule. *)
Definition activate_sponsored := activate_with_caller_bound false.

Lemma caller_bound_usage_is_client_part :
  forall caller,
    caller_bound_usage false caller = caller_client_part caller.
Proof.
  intros caller.
  unfold caller_bound_usage.
  apply Nat.add_0_r.
Qed.

(* When the client part fits the limit and the purse covers the hold, the
   sponsored activation has the outcome of the installer-share activation,
   for every amount of priced owner work. The caller's charge is its own
   work, within its hold, and its purse never goes below zero. *)
Theorem sponsored_activation_needs_only_caller_work :
  forall caller caller_balance gateway_authenticated outer_bound slot_bound
         outer_cost slot_cost share state,
    caller_client_part caller <= caller_limit caller ->
    caller_hold caller <= caller_balance ->
    activate_sponsored caller caller_balance gateway_authenticated
      outer_bound slot_bound outer_cost slot_cost share state
    = SponsoredRun
        (activate_with_installer_share gateway_authenticated outer_bound
           slot_bound outer_cost slot_cost share state)
        (caller_balance - caller_charge caller) /\
    caller_charge caller <= caller_hold caller /\
    caller_balance - caller_charge caller + caller_charge caller
      = caller_balance.
Proof.
  intros caller caller_balance gateway_authenticated outer_bound slot_bound
    outer_cost slot_cost share state fits covered.
  assert (charge_within_hold : caller_charge caller <= caller_hold caller).
  { unfold caller_charge, caller_hold.
    apply Nat.mul_le_mono_r.
    unfold caller_client_part in fits.
    lia. }
  split.
  - unfold activate_sponsored, activate_with_caller_bound.
    rewrite caller_bound_usage_is_client_part.
    assert (check :
      Nat.leb (caller_client_part caller) (caller_limit caller) = true).
    { apply Nat.leb_le. exact fits. }
    rewrite check.
    reflexivity.
  - split.
    + exact charge_within_hold.
    + lia.
Qed.

(* The control: a fee-only caller. Its own work 1 and the owner work at
   price 0 (1) fit its limit 2, its purse holds exactly its hold 2, and
   the priced owner work is 3. *)
Definition fee_only_caller : sponsored_caller := {|
  caller_own_usage := 1;
  caller_price := 1;
  caller_limit := 2;
  caller_zero_priced_owner_usage := 1;
  caller_priced_owner_usage := 3
|}.

(* In installer_short_state with share 1, the installer-share activation
   fires, and the S11 rule runs it for the fee-only caller, which pays 1.
   The rule before S11 counts the whole run, 5, against the limit 2, so
   the same caller exhausts its limit and nothing fires. *)
Theorem nc_whole_run_limit_refuses_a_fee_only_caller :
  caller_client_part fee_only_caller <= caller_limit fee_only_caller /\
  caller_hold fee_only_caller <= 2 /\
  (exists settled,
     activate_with_installer_share true 2 2 1 1 1 installer_short_state
       = InstallerActivated settled) /\
  activate_sponsored fee_only_caller 2 true 2 2 1 1 1 installer_short_state
    = SponsoredRun
        (activate_with_installer_share true 2 2 1 1 1 installer_short_state)
        1 /\
  activate_with_caller_bound true fee_only_caller 2 true 2 2 1 1 1
    installer_short_state = CallerLimitExhausted.
Proof.
  split; [cbv; lia |].
  split; [cbv; lia |].
  split; [eexists; reflexivity |].
  split; reflexivity.
Qed.

Print Assumptions scaffold_install_is_conserving.
Print Assumptions eager_located_install_rejects_new_zero_purses.
Print Assumptions staged_scaffold_install_needs_no_candidate_purse_supply.
Print Assumptions dual_purse_funding_is_exact_and_conserving.
Print Assumptions insufficient_dual_purse_funding_is_atomic.
Print Assumptions rejected_dual_purse_funding_preserves_registry.
Print Assumptions accepted_dual_purse_funding_creates_both_vaults.
Print Assumptions eager_target_creation_breaks_rejection_atomicity.
Print Assumptions slot_only_funding_cannot_satisfy_positive_outer_bound.
Print Assumptions dual_funding_establishes_local_sufficiency.
Print Assumptions activation_requires_gateway_authentication.
Print Assumptions activation_requires_both_located_purses.
Print Assumptions activated_lollipop_settlement_is_exact_and_conserving.
Print Assumptions installer_share_activation_is_exact_and_conserving.
Print Assumptions installer_share_never_overdraws.
Print Assumptions unpaid_installer_share_refuses_activation.
Print Assumptions refused_activation_keeps_state.
Print Assumptions nc_ignored_installer_share_runs_free.
Print Assumptions sponsored_activation_needs_only_caller_work.
Print Assumptions nc_whole_run_limit_refuses_a_fee_only_caller.
