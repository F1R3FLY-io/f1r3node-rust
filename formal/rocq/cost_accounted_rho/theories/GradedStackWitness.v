From Stdlib Require Import Lists.List Arith.PeanoNat Lia Sorting.Permutation.
From CostAccountedRho Require Import CostAccountedSyntax CASyntax CABinding CAReduction
  CAGradedTransition CAJoinConservation AuthorityPresentation LocatedStackConsumption
  PersistentActivation ConsentedFundingAllowance.
Import ListNotations.

Inductive witnessed_graded_step : signed_term -> sig -> signed_term -> list sig -> list token -> Prop :=
| witness_rule1 : forall x T U s t,
    witnessed_graded_step
      (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) s) (STStack (TGate s t)))
      s (STPar (subst_st T 0 (CQuote U)) (STStack t)) [s] [t]
| witness_rule2 : forall x T U s1 s2 t1 t2,
    witnessed_graded_step
      (STPar (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) (SAnd s1 s2))
        (STStack (TGate s1 t1))) (STStack (TGate s2 t2)))
      (SAnd s1 s2) (STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t2))
      [s1; s2] [t1; t2]
| witness_rule3 : forall x T U s1 s2 t,
    witnessed_graded_step
      (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) (SAnd s1 s2))
        (STStack (TGate (SAnd s1 s2) t)))
      (SAnd s1 s2) (STPar (subst_st T 0 (CQuote U)) (STStack t)) [SAnd s1 s2] [t]
| witness_rule4 : forall x T U s1 s2 t,
    witnessed_graded_step
      (STPar (STPar (STSigned (CPInput x T) s1) (STSigned (CPOutput x U) s2))
        (STStack (TGate (SAnd s1 s2) t)))
      (SAnd s1 s2) (STPar (subst_st T 0 (CQuote U)) (STStack t)) [SAnd s1 s2] [t]
| witness_rule5 : forall x T U s1 s2 t1 t2,
    witnessed_graded_step
      (STPar (STPar (STPar (STSigned (CPInput x T) s1) (STSigned (CPOutput x U) s2))
        (STStack (TGate s1 t1))) (STStack (TGate s2 t2)))
      (SAnd s1 s2) (STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t2))
      [s1; s2] [t1; t2]
| witness_join1 : forall xs Us T s t snds,
    snds = join_sends xs Us -> length xs = length Us ->
    witnessed_graded_step
      (STPar (STSigned (CPPar (CPJoin xs T) snds) s) (STStack (TGate s t)))
      s (STPar (subst_st_many T Us) (STStack t)) [s] [t]
| witness_join2 : forall xs Us ts T s t snds,
    snds = signed_sends xs Us ts -> length xs = length Us -> length xs = length ts ->
    witnessed_graded_step
      (STPar (STPar (STSigned (CPJoin xs T) s) snds)
        (STStack (TGate (join_token_key s ts) t)))
      (join_token_key s ts) (STPar (subst_st_many T Us) (STStack t)) [join_token_key s ts] [t]
| witness_parallel_left : forall before grade after context heads tails,
    witnessed_graded_step before grade after heads tails ->
    witnessed_graded_step (STPar before context) grade (STPar after context) heads tails
| witness_parallel_right : forall before grade after context heads tails,
    witnessed_graded_step before grade after heads tails ->
    witnessed_graded_step (STPar context before) grade (STPar context after) heads tails.

Theorem stack_witness_erases_to_actual_step : forall before grade after heads tails,
  witnessed_graded_step before grade after heads tails -> graded_step before grade after.
Proof.
  intros before grade after heads tails H. induction H.
  - apply g_rule1.
  - apply g_rule2.
  - apply g_rule3.
  - apply g_rule4.
  - apply g_rule5.
  - apply g_join1; assumption.
  - apply g_join2; assumption.
  - apply g_par_l. exact IHwitnessed_graded_step.
  - apply g_par_r. exact IHwitnessed_graded_step.
Qed.

Theorem every_actual_step_has_a_stack_witness : forall before grade after,
  graded_step before grade after ->
  exists heads tails, witnessed_graded_step before grade after heads tails.
Proof.
  intros before grade after H. induction H.
  - eexists _, _. apply witness_rule1.
  - eexists _, _. apply witness_rule2.
  - eexists _, _. apply witness_rule3.
  - eexists _, _. apply witness_rule4.
  - eexists _, _. apply witness_rule5.
  - eexists _, _. apply witness_join1; assumption.
  - eexists _, _. apply witness_join2; assumption.
  - destruct IHgraded_step as [heads [tails W]]. eexists _, _. apply witness_parallel_left. exact W.
  - destruct IHgraded_step as [heads [tails W]]. eexists _, _. apply witness_parallel_right. exact W.
Qed.

Theorem stack_instrumentation_preserves_the_transition_relation : forall before grade after,
  graded_step before grade after <->
  exists heads tails, witnessed_graded_step before grade after heads tails.
Proof.
  intros. split; [apply every_actual_step_has_a_stack_witness|].
  intros [heads [tails H]]. eapply stack_witness_erases_to_actual_step; eauto.
Qed.

Theorem witnessed_heads_and_tails_have_equal_length : forall before grade after heads tails,
  witnessed_graded_step before grade after heads tails -> length heads = length tails.
Proof. intros before grade after heads tails H. induction H; simpl; auto. Qed.

Theorem every_witness_consumes_a_gate : forall before grade after heads tails,
  witnessed_graded_step before grade after heads tails -> heads <> [].
Proof. intros before grade after heads tails H. induction H; discriminate || assumption. Qed.

Theorem witnessed_heads_have_exact_grade_atoms : forall before grade after heads tails,
  witnessed_graded_step before grade after heads tails ->
  concat (map payable_sig_atoms heads) = payable_sig_atoms grade.
Proof.
  intros before grade after heads tails H. induction H; simpl;
    repeat rewrite app_nil_r; auto.
Qed.

Fixpoint token_cells (tokens : token) : @authority_stack sig :=
  match tokens with
  | TUnit => []
  | TGate head tail => payable_sig_atoms head :: token_cells tail
  end.

Fixpoint stack_inputs (heads : list sig) (tails : list token) : list (@authority_stack sig) :=
  match heads, tails with
  | head :: heads, tail :: tails =>
      (payable_sig_atoms head :: token_cells tail) :: stack_inputs heads tails
  | _, _ => []
  end.

Theorem stack_input_pop_is_exact : forall heads tails,
  length heads = length tails ->
  pop_stacks (stack_inputs heads tails) = Some (map payable_sig_atoms heads, map token_cells tails).
Proof.
  induction heads as [|head heads IH]; intros tails H; destruct tails; simpl in *; try discriminate.
  - reflexivity.
  - rewrite IH by lia. reflexivity.
Qed.

Theorem actual_rule_projects_to_exact_ordered_pop : forall before grade after heads tails,
  witnessed_graded_step before grade after heads tails ->
  pop_stacks (stack_inputs heads tails) = Some (map payable_sig_atoms heads, map token_cells tails) /\
  exact_cover (payable_sig_atoms grade) (map payable_sig_atoms heads).
Proof.
  intros before grade after heads tails H. split.
  - apply stack_input_pop_is_exact. eapply witnessed_heads_and_tails_have_equal_length; eauto.
  - unfold exact_cover, presentation_atoms.
    rewrite (witnessed_heads_have_exact_grade_atoms _ _ _ _ _ H). apply Permutation_refl.
Qed.

Theorem different_residual_cells_cannot_match_the_witness : forall before grade after heads tails observed,
  witnessed_graded_step before grade after heads tails ->
  observed <> map token_cells tails ->
  pop_stacks (stack_inputs heads tails) <> Some (map payable_sig_atoms heads, observed).
Proof.
  intros before grade after heads tails observed H Different.
  rewrite (proj1 (actual_rule_projects_to_exact_ordered_pop _ _ _ _ _ H)).
  intro E. apply Different. congruence.
Qed.

Theorem witnessed_step_proves_actual_program_modality : forall before grade after heads tails formula,
  witnessed_graded_step before grade after heads tails -> gsat after formula -> gsat before (GDia grade formula).
Proof.
  intros before grade after heads tails formula W S.
  eapply gdia_complete; [eapply stack_witness_erases_to_actual_step; eauto|exact S].
Qed.

Theorem compound_and_split_heads_have_the_same_grade : forall first second tail left_tail right_tail,
  pop_stacks (stack_inputs [SAnd first second] [tail]) =
    Some ([payable_sig_atoms first ++ payable_sig_atoms second], [token_cells tail]) /\
  pop_stacks (stack_inputs [first; second] [left_tail; right_tail]) =
    Some ([payable_sig_atoms first; payable_sig_atoms second], [token_cells left_tail; token_cells right_tail]).
Proof. intros. split; reflexivity. Qed.

Theorem repeated_authority_still_requires_two_split_occurrences : forall authority left_tail right_tail,
  length (stack_inputs [authority; authority] [left_tail; right_tail]) = 2 /\
  concat (map payable_sig_atoms [authority; authority]) =
    payable_sig_atoms authority ++ payable_sig_atoms authority.
Proof. intros. simpl. rewrite app_nil_r. auto. Qed.

Definition physical_stack_binding (ids : list nat) (expected : list (@authority_stack sig))
  (inventory : @stack_inventory sig) : Prop :=
  NoDup ids /\ map inventory ids = map (@Some (@authority_stack sig)) expected.

Theorem physical_binding_has_one_identity_per_occurrence : forall ids expected inventory,
  physical_stack_binding ids expected inventory -> length ids = length expected.
Proof.
  intros ids expected inventory [_ E]. apply (f_equal (@length (option (@authority_stack sig)))) in E.
  now rewrite !length_map in E.
Qed.

Theorem duplicate_physical_identity_rejects_binding : forall id prefix middle suffix expected inventory,
  ~ physical_stack_binding (prefix ++ id :: middle ++ id :: suffix) expected inventory.
Proof.
  intros id prefix middle suffix expected inventory [Unique _].
  apply NoDup_remove_2 in Unique. apply Unique.
  apply in_or_app. right. apply in_or_app. right. now left.
Qed.

Theorem selected_step_maps_exact_tails : forall ids before after,
  selected_stack_step ids before after ->
  map after ids = map (fun entry => option_map (@tl (@authority_cell sig)) entry) (map before ids).
Proof.
  intros ids before after [_ Step]. rewrite map_map. apply map_ext_in.
  intros id Selected. specialize (Step id).
  destruct (in_dec Nat.eq_dec id ids); [|contradiction].
  destruct Step as [head [tail [B A]]]. rewrite B, A. reflexivity.
Qed.

Theorem logical_and_physical_pop_have_identical_residuals :
  forall term grade next heads tails ids before after,
  witnessed_graded_step term grade next heads tails ->
  physical_stack_binding ids (stack_inputs heads tails) before ->
  selected_stack_step ids before after ->
  physical_stack_binding ids (map token_cells tails) after.
Proof.
  intros term grade next heads tails ids before after W [Unique Binding] Step.
  split; [exact Unique|].
  rewrite (selected_step_maps_exact_tails _ _ _ Step), Binding, map_map.
  pose proof (pop_stacks_tails_are_ordered _ _ _
    (proj1 (actual_rule_projects_to_exact_ordered_pop _ _ _ _ _ W))) as Tails.
  rewrite Tails, map_map. reflexivity.
Qed.

Theorem physical_binding_preserves_occurrence_count :
  forall term grade next heads tails ids before,
  witnessed_graded_step term grade next heads tails ->
  physical_stack_binding ids (stack_inputs heads tails) before ->
  length ids = length heads.
Proof.
  intros term grade next heads tails ids before W B.
  pose proof (physical_binding_has_one_identity_per_occurrence _ _ _ B) as Count.
  pose proof (pop_stacks_preserves_stack_count _ _ _
    (proj1 (actual_rule_projects_to_exact_ordered_pop _ _ _ _ _ W))) as [Heads _].
  rewrite length_map in Heads. lia.
Qed.

Section FundedProgramEvents.

Context {location surface permission : Type}.
Context (near : surface -> location -> Prop).
Context (charge : allowance_capture -> @located_request sig surface -> nat).
Context (permits : permission -> allowance_capture -> @located_request sig surface -> Prop).

Definition funded_program_event term grade next identity index request
  (before after : @firing_state sig location permission) : Prop :=
  exists heads tails,
    witnessed_graded_step term grade next heads tails /\
    request_demand request = payable_sig_atoms grade /\
    physical_stack_binding (request_stacks request) (stack_inputs heads tails)
      (forget_locations (stacks before)) /\
    firing_step near charge permits (Fire identity index request) before after.

Theorem funded_program_event_preserves_accounting :
  forall term grade next identity index request before after,
  funded_program_event term grade next identity index request before after ->
  firing_valid before -> firing_valid after.
Proof.
  intros term grade next identity index request before after
    [heads [tails [W [Demand [Binding FireStep]]]]] Valid.
  eapply firing_step_preserves_accounting; eauto.
Qed.

Theorem construct_funded_program_event :
  forall term grade next heads tails index request before record old limit,
  witnessed_graded_step term grade next heads tails ->
  request_demand request = payable_sig_atoms grade ->
  physical_stack_binding (request_stacks request) (stack_inputs heads tails)
    (forget_locations (stacks before)) ->
  request_admitted near request (stacks before) ->
  nth_error (captures (funding before)) index = Some record ->
  nth_error (PersistentFundingAllowance.active_slots (ConsentedFundingAllowance.quantity_state (funding before))) index = Some (Some limit) ->
  nth_error (active_work before) index = Some old ->
  permits (service_permission before) record request ->
  charge_policy charge -> 0 < charge record request -> old + charge record request <= limit ->
  exists identity after, funded_program_event term grade next identity index request before after.
Proof.
  intros term grade next heads tails index request before record old limit
    W Demand Binding Admitted Capture Slot Work Permit Policy Positive Bound.
  set (identity := S (fold_right Nat.max 0 (firing_identities before))).
  set (after := {| service_permission := service_permission before; funding := funding before;
    stacks := consume_located (request_stacks request) (stacks before);
    active_work := set_work index (old + charge record request) (active_work before);
    retained_charge := retained_charge before + charge record request;
    firing_identities := identity :: firing_identities before |}).
  exists identity, after, heads, tails.
  split; [exact W|]. split; [exact Demand|]. split; [exact Binding|].
  eapply retain_firing with (record := record) (old := old) (limit := limit);
    try eassumption.
  - apply fresh_firing_identity.
  - split; [exact Admitted|reflexivity].
Qed.

Theorem funded_program_event_has_actual_step :
  forall term grade next identity index request before after,
  funded_program_event term grade next identity index request before after -> graded_step term grade next.
Proof.
  intros term grade next identity index request before after [heads [tails [W _]]].
  eapply stack_witness_erases_to_actual_step; eauto.
Qed.

Theorem funded_program_event_preserves_corresponding_tails :
  forall term grade next identity index request before after,
  funded_program_event term grade next identity index request before after ->
  exists heads tails,
    witnessed_graded_step term grade next heads tails /\
    physical_stack_binding (request_stacks request) (map token_cells tails)
      (forget_locations (stacks after)).
Proof.
  intros term grade next identity index request before after
    [heads [tails [W [Demand [Binding FireStep]]]]].
  exists heads, tails. split; [exact W|].
  eapply logical_and_physical_pop_have_identical_residuals; [exact W|exact Binding|].
  exact (firing_step_refines_ordered_consumption near charge permits _ _ _ FireStep).
Qed.

Theorem funded_program_event_proves_target_modality :
  forall term grade next identity index request before after formula,
  funded_program_event term grade next identity index request before after ->
  gsat next formula -> gsat term (GDia grade formula).
Proof.
  intros term grade next identity index request before after formula Event Target.
  eapply gdia_complete; [eapply funded_program_event_has_actual_step; eauto|exact Target].
Qed.

Theorem funded_program_event_has_nonempty_payable_grade :
  forall term grade next identity index request before after,
  funded_program_event term grade next identity index request before after ->
  payable_sig_atoms grade <> [].
Proof.
  intros term grade next identity index request before after
    [heads [tails [W [Demand [Binding Event]]]]] Empty.
  inversion Event; subst.
  match goal with
  | P : charge_policy _, B : 0 < charge _ request |- _ =>
      destruct P as [Zero _]; rewrite (Zero _ _ (eq_trans Demand Empty)) in B; lia
  end.
Qed.

Definition bookkeeping_action (action : @firing_action sig surface) : Prop :=
  match action with Fire _ _ _ => False | _ => True end.

Inductive funded_program_history : list (option sig * @firing_action sig surface) ->
  signed_term -> @firing_state sig location permission ->
  signed_term -> @firing_state sig location permission -> Prop :=
| funded_history_nil : forall term state, funded_program_history [] term state term state
| funded_history_fire : forall term grade next identity index request before middle rest final after,
    funded_program_event term grade next identity index request before middle ->
    funded_program_history rest next middle final after ->
    funded_program_history ((Some grade, Fire identity index request) :: rest) term before final after
| funded_history_bookkeeping : forall action term before middle rest final after,
    bookkeeping_action action -> firing_step near charge permits action before middle ->
    funded_program_history rest term middle final after ->
    funded_program_history ((None, action) :: rest) term before final after.

Theorem mixed_history_projects_to_funding_history : forall actions term before final after,
  funded_program_history actions term before final after ->
  firing_history near charge permits (map snd actions) before after.
Proof.
  intros actions term before final after H. induction H; simpl.
  - constructor.
  - destruct H as [heads [tails [W [Demand [Binding Step]]]]].
    econstructor; eauto.
  - econstructor; eauto.
Qed.

Inductive actual_graded_history : list sig -> signed_term -> signed_term -> Prop :=
| actual_history_nil : forall term, actual_graded_history [] term term
| actual_history_cons : forall term grade next rest final,
    graded_step term grade next -> actual_graded_history rest next final ->
    actual_graded_history (grade :: rest) term final.

Definition executed_grades (actions : list (option sig * @firing_action sig surface)) : list sig :=
  fold_right (fun action rest => match fst action with Some grade => grade :: rest | None => rest end) [] actions.

Theorem mixed_history_projects_to_actual_graded_history : forall actions term before final after,
  funded_program_history actions term before final after ->
  actual_graded_history (executed_grades actions) term final.
Proof.
  intros actions term before final after H. induction H; simpl.
  - constructor.
  - econstructor; [eapply funded_program_event_has_actual_step; eauto|exact IHfunded_program_history].
  - exact IHfunded_program_history.
Qed.

Theorem mixed_history_preserves_recorded_funding : forall actions term before final after,
  funded_program_history actions term before final after ->
  firing_valid before -> firing_valid after.
Proof.
  intros actions term before final after H V.
  eapply arbitrary_firing_histories_preserve_accounting; [|exact V].
  exact (mixed_history_projects_to_funding_history _ _ _ _ _ H).
Qed.

Theorem mixed_history_preserves_captured_consent : forall actions term before final after index record,
  funded_program_history actions term before final after ->
  nth_error (captures (funding before)) index = Some record ->
  nth_error (captures (funding after)) index = Some record.
Proof.
  intros actions term before final after index record H Capture.
  eapply firing_history_preserves_captures; [|exact Capture].
  exact (mixed_history_projects_to_funding_history _ _ _ _ _ H).
Qed.

Theorem mixed_history_preserves_exact_ordered_cells : forall actions term before final after id cells,
  funded_program_history actions term before final after ->
  forget_locations (stacks before) id = Some cells ->
  forget_locations (stacks after) id =
    Some (skipn (selected_uses id (map action_stack_selection (map snd actions))) cells) /\
  selected_uses id (map action_stack_selection (map snd actions)) <= length cells.
Proof.
  intros actions term before final after id cells H Cells.
  eapply reusable_permission_cannot_recreate_cells; [|exact Cells].
  exact (mixed_history_projects_to_funding_history _ _ _ _ _ H).
Qed.

Theorem mixed_history_preserves_purse_locations : forall actions term before final after id place cells,
  funded_program_history actions term before final after -> stacks before id = Some (place, cells) ->
  exists remaining, stacks after id = Some (place, remaining).
Proof.
  intros actions term before final after id place cells H Cells.
  eapply firing_history_preserves_locations; [|exact Cells].
  exact (mixed_history_projects_to_funding_history _ _ _ _ _ H).
Qed.

End FundedProgramEvents.

Theorem bound_physical_pop_preserves_unselected_entries : forall ids (before after : @stack_inventory sig) id,
  selected_stack_step ids before after -> ~ In id ids -> after id = before id.
Proof.
  intros ids before after id [_ Step] Absent. specialize (Step id).
  destruct (in_dec Nat.eq_dec id ids); [contradiction|exact Step].
Qed.
