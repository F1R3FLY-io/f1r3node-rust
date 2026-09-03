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
