From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool Lia Sorting.Permutation.
From CostAccountedRho Require Import PersistentFundingAllowance ConsentedFundingAllowance
  FundingConsentHistory LocatedStackConsumption AuthorityPresentation.
Import ListNotations.

Fixpoint set_work (index value : nat) (work : list nat) : list nat :=
  match index, work with
  | 0, _ :: rest => value :: rest
  | S index, head :: rest => head :: set_work index value rest
  | _, [] => []
  end.

Definition total_work := fold_right Nat.add 0.

Lemma set_work_total : forall work index old value,
  nth_error work index = Some old ->
  total_work (set_work index value work) + old = total_work work + value.
Proof.
  induction work as [|head rest IH]; intros index old value H;
    destruct index; simpl in H; try discriminate.
  - inversion H; subst. simpl. lia.
  - specialize (IH _ _ value H). simpl. lia.
Qed.

Definition work_fits (work : nat) (slot : option nat) : Prop :=
  match slot with Some limit => work <= limit | None => work = 0 end.

Lemma fresh_firing_identity : forall identities,
  ~ In (S (fold_right Nat.max 0 identities)) identities.
Proof.
  assert (bound : forall identities id, In id identities -> id <= fold_right Nat.max 0 identities).
  { induction identities as [|head rest IH]; intros id H; simpl in *; [contradiction|].
    destruct H as [<-|H].
    - apply Nat.le_max_l.
    - eapply Nat.le_trans; [exact (IH _ H)|apply Nat.le_max_r]. }
  intros identities H. pose proof (bound _ _ H). lia.
Qed.

Lemma work_fits_total : forall work slots,
  Forall2 work_fits work slots -> total_work work <= reserved_total slots.
Proof.
  intros work slots H. induction H; simpl; [lia|].
  destruct y; unfold work_fits in H; lia.
Qed.

Lemma firing_preserves_slot_bounds : forall work slots index old limit added,
  Forall2 work_fits work slots ->
  nth_error work index = Some old -> nth_error slots index = Some (Some limit) ->
  old + added <= limit ->
  Forall2 work_fits (set_work index (old + added) work) slots.
Proof.
  intros work slots index old limit added H. revert index old limit.
  induction H; intros index old limit W S B; destruct index; simpl in *; try discriminate.
  - inversion W; inversion S; subst. constructor; assumption.
  - constructor; [exact H|]. eapply IHForall2; eauto.
Qed.

Lemma closing_preserves_slot_bounds : forall work slots index old limit remaining,
  Forall2 work_fits work slots ->
  nth_error work index = Some old ->
  close_slot index slots = Some (limit, remaining) ->
  Forall2 work_fits (set_work index 0 work) remaining.
Proof.
  intros work slots index old limit remaining H. revert index old limit remaining.
  induction H; intros index old limit remaining W C; destruct index; simpl in *; try discriminate.
  - destruct y; inversion C; subst. constructor; [reflexivity|assumption].
  - destruct (close_slot index l') as [[found tail]|] eqn:E; try discriminate.
    inversion C; subst. constructor; [exact H|]. eapply IHForall2; eauto.
Qed.

Section PersistentActivation.

Context {atom location surface permission : Type}.
Context (near : surface -> location -> Prop).
Context (charge : allowance_capture -> @located_request atom surface -> nat).
Context (permits : permission -> allowance_capture -> @located_request atom surface -> Prop).

Definition charge_policy : Prop :=
  (forall record request, request_demand request = [] -> charge record request = 0) /\
  forall record first second,
    request_surface first = request_surface second ->
    Permutation (request_demand first) (request_demand second) ->
    Permutation (request_stacks first) (request_stacks second) ->
    charge record first = charge record second.

Theorem positive_empty_demand_refutes_charge_policy : forall record request,
  request_demand request = [] -> 0 < charge record request -> ~ charge_policy.
Proof.
  intros record request Empty Positive [Zero _]. rewrite (Zero _ _ Empty) in Positive. lia.
Qed.

Theorem presentation_dependent_charge_refutes_policy : forall record first second,
  request_surface first = request_surface second ->
  Permutation (request_demand first) (request_demand second) ->
  Permutation (request_stacks first) (request_stacks second) ->
  charge record first <> charge record second -> ~ charge_policy.
Proof.
  intros record first second Surface Demand Selection Different [_ Stable].
  apply Different. apply Stable; assumption.
Qed.

Record firing_state := {
  service_permission : permission;
  funding : consented_allowance;
  stacks : @located_inventory atom location;
  active_work : list nat;
  retained_charge : nat;
  firing_identities : list nat
}.

Definition firing_valid (state : firing_state) : Prop :=
  consented_valid (funding state) /\
  Forall2 work_fits (active_work state) (active_slots (quantity_state (funding state))) /\
  retained_charge state = consumed (quantity_state (funding state)) + total_work (active_work state) /\
  NoDup (firing_identities state).

Definition replace_funding state next work : firing_state :=
  {| service_permission := service_permission state; funding := next;
     stacks := stacks state; active_work := work;
     retained_charge := retained_charge state; firing_identities := firing_identities state |}.

Inductive firing_action :=
| Prepare (expected root price amount : nat) (authorized : bool)
| Fire (identity index : nat) (request : @located_request atom surface)
| Close (index : nat)
| TransferPermission (expected : nat) (terms : funding_terms) (authorized : bool)
| ExpandPermission (expected amount : nat) (authorized : bool)
| WalletDeposit (amount : nat)
| ReusePermission.

Inductive firing_step : firing_action -> firing_state -> firing_state -> Prop :=
| prepare_firing : forall state expected root price amount authorized next,
    reserve_consented (funding state) expected root price amount authorized = Some next ->
    firing_step (Prepare expected root price amount authorized) state
      (replace_funding state next (active_work state ++ [0]))
| retain_firing : forall state identity index request record old limit next_stacks,
    ~ In identity (firing_identities state) ->
    nth_error (captures (funding state)) index = Some record ->
    nth_error (active_slots (quantity_state (funding state))) index = Some (Some limit) ->
    nth_error (active_work state) index = Some old ->
    permits (service_permission state) record request ->
    0 < charge record request -> old + charge record request <= limit ->
    located_step near request (stacks state) next_stacks ->
    charge_policy ->
    firing_step (Fire identity index request) state
      {| service_permission := service_permission state; funding := funding state;
         stacks := next_stacks;
         active_work := set_work index (old + charge record request) (active_work state);
         retained_charge := retained_charge state + charge record request;
         firing_identities := identity :: firing_identities state |}
| close_firing : forall state index old record next,
    nth_error (active_work state) index = Some old ->
    settle_consented (funding state) index old = Some (record, next) ->
    firing_step (Close index) state
      (replace_funding state next (set_work index 0 (active_work state)))
| transfer_firing_permission : forall state expected terms authorized next,
    transfer_consented (funding state) expected terms authorized = Some next ->
    firing_step (TransferPermission expected terms authorized) state
      (replace_funding state next (active_work state))
| expand_firing_permission : forall state expected amount authorized next,
    expand_consented (funding state) expected amount authorized = Some next ->
    firing_step (ExpandPermission expected amount authorized) state
      (replace_funding state next (active_work state))
| deposit_wallet : forall state amount, firing_step (WalletDeposit amount) state state
| reuse_firing_permission : forall state, firing_step ReusePermission state state.

Inductive firing_history : list firing_action -> firing_state -> firing_state -> Prop :=
| firing_history_nil : forall state, firing_history [] state state
| firing_history_cons : forall action rest before middle after,
    firing_step action before middle -> firing_history rest middle after ->
    firing_history (action :: rest) before after.

Theorem firing_step_preserves_permission : forall action before after,
  firing_step action before after -> service_permission after = service_permission before.
Proof. intros action before after H. destruct H; reflexivity. Qed.

Theorem firing_histories_preserve_permission : forall actions before after,
  firing_history actions before after -> service_permission after = service_permission before.
Proof.
  intros actions before after H. induction H; [reflexivity|].
  rewrite IHfiring_history. now apply firing_step_preserves_permission in H.
Qed.

Theorem retained_work_never_decreases : forall action before after,
  firing_step action before after -> retained_charge before <= retained_charge after.
Proof. intros action before after H. destruct H; simpl; lia. Qed.

Theorem firing_step_preserves_accounting : forall action before after,
  firing_step action before after -> firing_valid before -> firing_valid after.
Proof.
  intros action before after Hstep [V [B [T U]]].
  destruct Hstep; unfold firing_valid; simpl in *.
  - pose proof (reserve_consented_preserves_validity _ _ _ _ _ _ _ V H) as NV.
    pose proof (consented_step_refines_quantity_step _
      (ReserveConsented expected root price amount authorized) _ H) as Q.
    simpl in Q.
    destruct (authorized && (expected =? authorization_version (quantity_state (funding state))) &&
      (amount <=? available (quantity_state (funding state)))) eqn:E; try discriminate.
    inversion Q. split; [exact NV|]. split.
    + simpl. apply Forall2_app; [exact B|]. constructor; [simpl; lia|constructor].
    + simpl. split; [unfold total_work in *; rewrite fold_right_app; simpl; lia|exact U].
  - split; [exact V|]. split.
    + eapply firing_preserves_slot_bounds; eauto.
    + split.
      * pose proof (set_work_total _ _ _ (old + charge record request) H2). lia.
      * constructor; assumption.
  - pose proof (settlement_preserves_consented_validity _ _ _ _ _ V H0) as NV.
    pose proof (settlement_matches_captured_amount _ _ _ _ _ (proj2 V) H0) as [_ [_ C]].
    assert (NB : Forall2 work_fits (set_work index 0 (active_work state))
      (active_slots (quantity_state next))).
    { unfold settle_consented in H0.
      destruct (nth_error (captures (funding state)) index); try discriminate.
      unfold settle_allowance in H0.
      destruct (close_slot index (active_slots (quantity_state (funding state))))
        as [[reserved remaining]|] eqn:E; try discriminate.
      destruct (old <=? reserved); inversion H0; subst. simpl.
      eapply closing_preserves_slot_bounds; eauto. }
    split; [exact NV|]. split; [exact NB|]. split; [|exact U].
    pose proof (set_work_total _ _ _ 0 H). lia.
  - pose proof (transfer_consented_preserves_validity _ _ _ _ _ V H) as NV.
    pose proof (consented_step_refines_quantity_step _
      (TransferConsented expected terms authorized) _ H) as Q.
    pose proof (transfer_preserves_allowance_stocks _ _ _ _ Q) as [_ [_ [C [S _]]]].
    split; [exact NV|]. rewrite C, S. auto.
  - pose proof (expansion_preserves_consented_validity _ _ _ _ _ V H) as NV.
    pose proof (consented_step_refines_quantity_step _
      (ExpandConsented expected amount authorized) _ H) as Q.
    simpl in Q.
    destruct (authorized && (expected =? authorization_version (quantity_state (funding state))));
      try discriminate.
    inversion Q. split; [exact NV|]. simpl. auto.
  - auto.
  - auto.
Qed.

Theorem arbitrary_firing_histories_preserve_accounting : forall actions before after,
  firing_history actions before after -> firing_valid before -> firing_valid after.
Proof.
  intros actions before after H. induction H; intro V; [exact V|].
  apply IHfiring_history. eapply firing_step_preserves_accounting; eauto.
Qed.

Theorem retained_work_is_bounded_by_issued_allowance : forall state,
  firing_valid state -> retained_charge state <= issued (quantity_state (funding state)).
Proof.
  intros state [[C _] [B [T _]]].
  pose proof (work_fits_total _ _ B). unfold conserved in C. lia.
Qed.

Definition action_stack_selection action : list nat :=
  match action with Fire _ _ request => request_stacks request | _ => [] end.

Theorem firing_step_refines_ordered_consumption : forall action before after,
  firing_step action before after ->
  selected_stack_step (action_stack_selection action)
    (forget_locations (stacks before)) (forget_locations (stacks after)).
Proof.
  intros action before after H. destruct H; simpl;
    try (split; [constructor|intro id; reflexivity]).
  exact (located_step_refines_selected_consumption near _ _ _ H6).
Qed.

Theorem firing_histories_refine_ordered_consumption : forall actions before after,
  firing_history actions before after ->
  selected_stack_history (map action_stack_selection actions)
    (forget_locations (stacks before)) (forget_locations (stacks after)).
Proof.
  intros actions before after H. induction H; simpl; [constructor|].
  econstructor; [exact (firing_step_refines_ordered_consumption _ _ _ H)|exact IHfiring_history].
Qed.

Theorem reusable_permission_cannot_recreate_cells : forall actions before after id cells,
  firing_history actions before after -> forget_locations (stacks before) id = Some cells ->
  forget_locations (stacks after) id =
    Some (skipn (selected_uses id (map action_stack_selection actions)) cells) /\
  selected_uses id (map action_stack_selection actions) <= length cells.
Proof.
  intros actions before after id cells H C.
  exact (selected_history_preserves_ordered_suffix _ _ _
    (firing_histories_refine_ordered_consumption _ _ _ H) id cells C).
Qed.

Theorem firing_step_preserves_captures : forall action before after index record,
  firing_step action before after ->
  nth_error (captures (funding before)) index = Some record ->
  nth_error (captures (funding after)) index = Some record.
Proof.
  intros action before after index record Hstep N. destruct Hstep; simpl in *; try exact N.
  - exact (consented_step_preserves_existing_capture _
      (ReserveConsented expected root price amount authorized) _ _ _ H N).
  - eapply (consented_step_preserves_existing_capture _ (SettleConsented index0 old) _ _ _); [|exact N].
    simpl. rewrite H0. reflexivity.
  - exact (consented_step_preserves_existing_capture _
      (TransferConsented expected terms authorized) _ _ _ H N).
  - exact (consented_step_preserves_existing_capture _
      (ExpandConsented expected amount authorized) _ _ _ H N).
Qed.

Theorem firing_history_preserves_captures : forall actions before after index record,
  firing_history actions before after ->
  nth_error (captures (funding before)) index = Some record ->
  nth_error (captures (funding after)) index = Some record.
Proof.
  intros actions before after index record H. induction H; intro N; [exact N|].
  apply IHfiring_history. eapply firing_step_preserves_captures; eauto.
Qed.

Theorem firing_step_preserves_locations : forall action before after id place cells,
  firing_step action before after -> stacks before id = Some (place, cells) ->
  exists remaining, stacks after id = Some (place, remaining).
Proof.
  intros action before after id place cells Hstep E.
  destruct Hstep; simpl in *; try (eexists; exact E).
  destruct H6 as [_ ->]. eapply consumption_preserves_location. exact E.
Qed.

Theorem firing_history_preserves_locations : forall actions before after id place cells,
  firing_history actions before after -> stacks before id = Some (place, cells) ->
  exists remaining, stacks after id = Some (place, remaining).
Proof.
  intros actions before after id place cells H. revert cells.
  induction H; intros cells E; [eexists; exact E|].
  destruct (firing_step_preserves_locations _ _ _ _ _ _ H E) as [remaining N].
  eapply IHfiring_history. exact N.
Qed.

Definition explicit_firing_expansion action : nat :=
  match action with ExpandPermission _ amount _ => amount | _ => 0 end.

Theorem firing_step_issuance_change : forall action before after,
  firing_step action before after ->
  issued (quantity_state (funding after)) = issued (quantity_state (funding before)) +
    explicit_firing_expansion action.
Proof.
  intros action before after Hstep. destruct Hstep; simpl; try lia.
  - exact (consented_step_changes_issuance_only_by_expansion _
      (ReserveConsented expected root price amount authorized) _ H).
  - eapply (consented_step_changes_issuance_only_by_expansion _ (SettleConsented index old) _).
    simpl. rewrite H0. reflexivity.
  - exact (consented_step_changes_issuance_only_by_expansion _
      (TransferConsented expected terms authorized) _ H).
  - exact (consented_step_changes_issuance_only_by_expansion _
      (ExpandConsented expected amount authorized) _ H).
Qed.

Theorem firing_history_issuance_change : forall actions before after,
  firing_history actions before after ->
  issued (quantity_state (funding after)) = issued (quantity_state (funding before)) +
    fold_right (fun action total => explicit_firing_expansion action + total) 0 actions.
Proof.
  intros actions before after H. induction H; simpl; [lia|].
  rewrite IHfiring_history, (firing_step_issuance_change _ _ _ H). lia.
Qed.

Theorem retained_work_has_initial_and_expansion_bound : forall actions before after,
  firing_history actions before after -> firing_valid before ->
  retained_charge after <= issued (quantity_state (funding before)) +
    fold_right (fun action total => explicit_firing_expansion action + total) 0 actions.
Proof.
  intros actions before after H V.
  pose proof (arbitrary_firing_histories_preserve_accounting _ _ _ H V) as A.
  pose proof (retained_work_is_bounded_by_issued_allowance _ A) as B.
  rewrite (firing_history_issuance_change _ _ _ H) in B. exact B.
Qed.

Theorem retained_firing_consumes_a_head : forall identity index request before after,
  firing_step (Fire identity index request) before after -> request_stacks request <> [].
Proof.
  intros identity index request before after H. inversion H; subst.
  intro E.
  match goal with
  | L : located_step _ _ _ _, P : charge_policy, B : 0 < charge _ _ |- _ =>
    destruct L as [[_ [_ C]] _];
    unfold exact_cover, presentation_atoms in C;
    rewrite E in C; simpl in C; apply Permutation_nil in C;
    destruct P as [Z _]; rewrite (Z _ _ C) in B; lia
  end.
Qed.

Theorem close_refunds_only_unspent_reservation : forall state index old record next,
  firing_valid state -> nth_error (active_work state) index = Some old ->
  settle_consented (funding state) index old = Some (record, next) ->
  available (quantity_state next) = available (quantity_state (funding state)) +
    (captured_amount record - old) /\
  consumed (quantity_state next) = consumed (quantity_state (funding state)) + old.
Proof.
  intros state index old record next [V _] W S.
  exact (proj2 (settlement_matches_captured_amount _ _ _ _ _ (proj2 V) S)).
Qed.

Theorem closed_recorded_work_cannot_settle_twice : forall state index old record next,
  settle_consented (funding state) index old = Some (record, next) ->
  ~ exists after, firing_step (Close index)
    (replace_funding state next (set_work index 0 (active_work state))) after.
Proof.
  intros state index old record next Closed [after H]. inversion H; subst; simpl in *.
  match goal with S : settle_consented next index ?debit = Some _ |- _ =>
    rewrite (consented_settlement_cannot_repeat _ _ _ _ _ debit Closed) in S;
    discriminate
  end.
Qed.

Theorem retained_firing_has_exact_delta : forall identity index request before after,
  firing_step (Fire identity index request) before after ->
  exists record,
    nth_error (captures (funding before)) index = Some record /\
    retained_charge after = retained_charge before + charge record request /\
    firing_identities after = identity :: firing_identities before /\
    ~ In identity (firing_identities before).
Proof.
  intros identity index request before after H. inversion H; subst; simpl.
  eexists. repeat split; eauto.
Qed.

Definition retained_firing_count actions : nat :=
  fold_right (fun action total => match action with Fire _ _ _ => S total | _ => total end) 0 actions.

Theorem positive_firing_count_is_bounded : forall actions before after,
  firing_history actions before after ->
  retained_charge before + retained_firing_count actions <= retained_charge after.
Proof.
  intros actions before after H. induction H; simpl; [lia|].
  destruct H; simpl in *; lia.
Qed.

Theorem funded_repeated_firings_exist : forall rounds cut candidate place tail state old limit record,
  charge_policy -> near cut place ->
  let request := {| request_surface := cut; request_demand := [candidate]; request_stacks := [0] |} in
  charge record request = 1 -> permits (service_permission state) record request ->
  nth_error (captures (funding state)) 0 = Some record ->
  nth_error (active_slots (quantity_state (funding state))) 0 = Some (Some limit) ->
  nth_error (active_work state) 0 = Some old -> old + rounds <= limit ->
  stacks state 0 = Some (place, repeat [candidate] rounds ++ tail) ->
  exists actions after,
    firing_history actions state after /\ length actions = rounds /\
    funding after = funding state /\ service_permission after = service_permission state /\
    retained_charge after = retained_charge state + rounds /\
    nth_error (active_work after) 0 = Some (old + rounds) /\
    stacks after 0 = Some (place, tail).
Proof.
  induction rounds as [|rounds IH]; intros cut candidate place tail state old limit record
    Policy Near request Charge Permit Capture Slot Work Bound Cells.
  - exists [], state. split; [constructor|].
    repeat split; try reflexivity; try (rewrite Nat.add_0_r; assumption).
    + lia.
    + exact Cells.
  - set (identity := S (fold_right Nat.max 0 (firing_identities state))).
    set (remaining := consume_located [0] (stacks state)).
    assert (Located : located_step near request (stacks state) remaining).
    { split; [|reflexivity]. split.
      - repeat constructor. simpl. tauto.
      - split.
        + intros id [<-|H]; [|contradiction].
          exists place, [candidate], (repeat [candidate] rounds ++ tail). auto.
        + unfold exact_cover, presentation_atoms, selected_head. simpl.
          rewrite Cells. simpl. apply Permutation_refl. }
    set (middle := {| service_permission := service_permission state; funding := funding state;
      stacks := remaining; active_work := set_work 0 (old + charge record request) (active_work state);
      retained_charge := retained_charge state + charge record request;
      firing_identities := identity :: firing_identities state |}).
    assert (Step : firing_step (Fire identity 0 request) state middle).
    { eapply retain_firing with (record := record) (old := old) (limit := limit);
        try eassumption.
      - apply fresh_firing_identity.
      - lia.
      - lia. }
    assert (NextWork : nth_error (active_work middle) 0 = Some (old + 1)).
    { unfold middle; simpl. destruct (active_work state) as [|head rest];
        simpl in Work; try discriminate. simpl. now rewrite Charge. }
    assert (NextCells : stacks middle 0 = Some (place, repeat [candidate] rounds ++ tail)).
    { unfold middle, remaining, consume_located; simpl. rewrite Cells. reflexivity. }
    destruct (IH cut candidate place tail middle (old + 1) limit record Policy Near
      Charge Permit Capture Slot NextWork) as [actions [after [History [Length [Funding [Permission [Total [FinalWork FinalCells]]]]]]]];
      try lia; try exact NextCells.
    exists (Fire identity 0 request :: actions), after.
    split; [econstructor; eauto|]. split; [simpl; lia|].
    split; [exact Funding|]. split; [exact Permission|].
    split.
    + unfold middle in Total. simpl in Total. rewrite Charge in Total. lia.
    + split; [rewrite FinalWork; f_equal; lia|exact FinalCells].
Qed.

Theorem exhausted_slot_rejects_even_with_cells : forall state identity index request used,
  nth_error (active_slots (quantity_state (funding state))) index = Some (Some used) ->
  nth_error (active_work state) index = Some used ->
  ~ exists after, firing_step (Fire identity index request) state after.
Proof.
  intros state identity index request used Slot Work [after H]. inversion H; subst.
  match goal with
  | S : nth_error (active_slots _) _ = Some (Some _), W : nth_error (active_work _) _ = Some _ |- _ =>
    rewrite Slot in S; inversion S; subst;
    rewrite Work in W; inversion W; subst; lia
  end.
Qed.

Theorem exhausted_stack_rejects_even_with_allowance : forall state identity index request id place,
  In id (request_stacks request) -> stacks state id = Some (place, []) ->
  ~ exists after, firing_step (Fire identity index request) state after.
Proof.
  intros state identity index request id place Hin Empty [after H]. inversion H; subst.
  match goal with L : located_step _ _ _ _ |- _ =>
    destruct L as [[_ [Heads _]] _];
    destruct (Heads id Hin) as [actual [head [tail [Entry _]]]];
    rewrite Empty in Entry; discriminate
  end.
Qed.

End PersistentActivation.
