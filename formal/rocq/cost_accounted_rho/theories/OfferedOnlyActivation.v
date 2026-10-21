From Stdlib Require Import Lists.List Bool.Bool Relations.Relation_Operators.
Import ListNotations.

(* DR-66 and DR-134: only offered-funded deploys are admitted, selected and
   validated, on every shard.

   The code keeps dev's activation flag (`offered_funded_v6_active` of the
   adopted genesis resource policy) and dev's inactive branches. A genesis
   requires the active protocol-6 policy (`create_genesis_block_with_policy`
   in casper/src/rust/genesis/genesis.rs), and a node adopts only such a
   genesis. The model therefore takes the flag from the adopted genesis policy.
   [genesis_accepts] is the genesis check, [accepted_genesis_is_active] shows
   that an accepted genesis sets the flag, and the offered-only theorems hold
   for every network that starts from an accepted genesis. The negative
   control [an_inactive_flag_reaches_a_body_only_state] shows that the genesis
   check is necessary. *)

Inductive deploy_format := BodyOnly | OfferedFunded.

Definition is_offered (deploy : deploy_format) : bool :=
  match deploy with
  | BodyOnly => false
  | OfferedFunded => true
  end.

Record genesis_policy := {
  policy_protocol : nat;
  policy_active : bool
}.

Definition genesis_accepts (policy : genesis_policy) : bool :=
  policy_active policy && Nat.eqb (policy_protocol policy) 6.

Theorem accepted_genesis_is_active : forall policy,
  genesis_accepts policy = true -> policy_active policy = true.
Proof.
  intros policy accepted.
  unfold genesis_accepts in accepted.
  apply andb_prop in accepted as [active _].
  exact active.
Qed.

Theorem an_inactive_genesis_is_refused : forall protocol,
  genesis_accepts {| policy_protocol := protocol; policy_active := false |} = false.
Proof. reflexivity. Qed.

(* Dev's admission, selection and validation, which keep the flag. *)

Definition admit (active : bool) (deploy : deploy_format) : bool :=
  if active then is_offered deploy else true.

Definition select (height : nat) (active : bool)
    (pending : list deploy_format) : list deploy_format :=
  if active then filter is_offered pending else pending.

Definition validate (active : bool) (deploys : list deploy_format) : bool :=
  if active then forallb is_offered deploys else true.

Theorem admission_rejects_body_only : forall policy,
  genesis_accepts policy = true -> admit (policy_active policy) BodyOnly = false.
Proof.
  intros policy accepted.
  rewrite (accepted_genesis_is_active policy accepted).
  reflexivity.
Qed.

Theorem admission_accepts_offered : forall active,
  admit active OfferedFunded = true.
Proof. intros active; destruct active; reflexivity. Qed.

Theorem selection_is_height_independent : forall first second active pending,
  select first active pending = select second active pending.
Proof. reflexivity. Qed.

Theorem selection_contains_only_offered : forall policy height pending deploy,
  genesis_accepts policy = true ->
  In deploy (select height (policy_active policy) pending) ->
  deploy = OfferedFunded.
Proof.
  intros policy height pending deploy accepted included.
  rewrite (accepted_genesis_is_active policy accepted) in included.
  apply filter_In in included as [_ offered].
  destruct deploy; discriminate || reflexivity.
Qed.

Theorem validation_rejects_body_only : forall policy before after,
  genesis_accepts policy = true ->
  validate (policy_active policy) (before ++ BodyOnly :: after) = false.
Proof.
  intros policy before after accepted.
  rewrite (accepted_genesis_is_active policy accepted).
  unfold validate.
  rewrite forallb_app.
  simpl.
  rewrite andb_false_r.
  reflexivity.
Qed.

Theorem validation_accepts_offered_only : forall active deploys,
  Forall (eq OfferedFunded) deploys -> validate active deploys = true.
Proof.
  intros active deploys offered.
  destruct active; [|reflexivity].
  unfold validate.
  apply forallb_forall.
  intros deploy included.
  rewrite Forall_forall in offered.
  rewrite <- (offered deploy included).
  reflexivity.
Qed.

Record network_state := {
  pending_deploys : list deploy_format;
  published_blocks : list (list deploy_format)
}.

Inductive step (active : bool) : network_state -> network_state -> Prop :=
| submit : forall state deploy,
    admit active deploy = true ->
    step active state
      {| pending_deploys := pending_deploys state ++ [deploy];
         published_blocks := published_blocks state |}
| publish : forall state height,
    step active state
      {| pending_deploys := [];
         published_blocks :=
           select height active (pending_deploys state) :: published_blocks state |}.

Definition offered_only (state : network_state) : Prop :=
  Forall (eq OfferedFunded) (pending_deploys state) /\
  Forall (Forall (eq OfferedFunded)) (published_blocks state).

Definition initial_state : network_state :=
  {| pending_deploys := []; published_blocks := [] |}.

Theorem step_preserves_offered_only : forall policy before after,
  genesis_accepts policy = true ->
  offered_only before ->
  step (policy_active policy) before after ->
  offered_only after.
Proof.
  intros policy before after accepted [pending_only blocks_only] transition.
  rewrite (accepted_genesis_is_active policy accepted) in transition.
  inversion transition; subst; split; simpl.
  - apply Forall_app. split; [exact pending_only |].
    destruct deploy; simpl in *; try congruence.
    constructor; [reflexivity | constructor].
  - exact blocks_only.
  - constructor.
  - constructor; [|exact blocks_only].
    apply Forall_forall. intros deploy included.
    apply filter_In in included as [_ offered].
    destruct deploy; discriminate || reflexivity.
Qed.

Theorem trace_preserves_offered_only : forall policy before after,
  genesis_accepts policy = true ->
  offered_only before ->
  clos_refl_trans network_state (step (policy_active policy)) before after ->
  offered_only after.
Proof.
  intros policy before after accepted initial trace.
  induction trace.
  - eapply step_preserves_offered_only; eauto.
  - exact initial.
  - apply IHtrace2. apply IHtrace1. exact initial.
Qed.

Theorem reachable_states_are_offered_only : forall policy state,
  genesis_accepts policy = true ->
  clos_refl_trans network_state (step (policy_active policy)) initial_state state ->
  offered_only state.
Proof.
  intros policy state accepted trace.
  apply trace_preserves_offered_only with (policy := policy) (before := initial_state);
    [exact accepted | split; constructor | exact trace].
Qed.

(* Negative control: with an inactive flag, dev's admission admits a body-only
   deploy, so a reachable state is not offered-only. This is why a genesis must
   require the active policy. *)

Theorem an_inactive_flag_reaches_a_body_only_state :
  exists state,
    clos_refl_trans network_state (step false) initial_state state /\
    ~ offered_only state.
Proof.
  exists {| pending_deploys := [BodyOnly]; published_blocks := [] |}.
  split.
  - apply rt_step. exact (submit false initial_state BodyOnly eq_refl).
  - intros [pending_only _]. simpl in pending_only.
    inversion pending_only as [| ? ? head _]. discriminate head.
Qed.

(* DR-66 item 4 and the scope map: each kind of evaluation maps to exactly one
   accounting scope. The scope type has no legacy case. *)

Inductive evaluation_kind :=
| UserDeploy
| GenesisPlay
| GenesisReplay
| SystemDeploy
| ExploratoryEstimate
| InternalRead
| RhoSpecHarness
| HistoricalBodyOnly.

Inductive evaluation_scope :=
| NativeOfferedMeter
| TrustedInstall
| SystemScope
| NativeDryRun
| UnmeteredInternalRead
| NamedInjPath
| CompatibilityError.

Definition scope_of (kind : evaluation_kind) : evaluation_scope :=
  match kind with
  | UserDeploy => NativeOfferedMeter
  | GenesisPlay => TrustedInstall
  | GenesisReplay => TrustedInstall
  | SystemDeploy => SystemScope
  | ExploratoryEstimate => NativeDryRun
  | InternalRead => UnmeteredInternalRead
  | RhoSpecHarness => NamedInjPath
  | HistoricalBodyOnly => CompatibilityError
  end.

(* The native meter measures COMMs and storage bytes. A funded offer is
   charged for its measure; a dry run only reports it. *)
Definition measured_by_native_meter (scope : evaluation_scope) : bool :=
  match scope with
  | NativeOfferedMeter | NativeDryRun => true
  | _ => false
  end.

Definition charges_a_payer (scope : evaluation_scope) : bool :=
  match scope with
  | NativeOfferedMeter => true
  | _ => false
  end.

Definition records_zero_cost (scope : evaluation_scope) : bool :=
  match scope with
  | TrustedInstall => true
  | _ => false
  end.

Definition executes_a_term (scope : evaluation_scope) : bool :=
  match scope with
  | CompatibilityError => false
  | _ => true
  end.

Definition all_scopes : list evaluation_scope :=
  [NativeOfferedMeter; TrustedInstall; SystemScope; NativeDryRun;
   UnmeteredInternalRead; NamedInjPath; CompatibilityError].

Theorem all_scopes_is_exhaustive : forall scope, In scope all_scopes.
Proof. intros scope; destruct scope; simpl; tauto. Qed.

Theorem genesis_replay_is_symmetric_with_genesis_play :
  scope_of GenesisReplay = scope_of GenesisPlay.
Proof. reflexivity. Qed.

Theorem genesis_runs_in_the_trusted_install_scope_at_cost_zero :
  scope_of GenesisPlay = TrustedInstall /\
  records_zero_cost (scope_of GenesisPlay) = true /\
  records_zero_cost (scope_of GenesisReplay) = true.
Proof. repeat split. Qed.

Theorem only_user_deploys_charge_a_payer : forall kind,
  charges_a_payer (scope_of kind) = true <-> kind = UserDeploy.
Proof.
  intros kind; split; [destruct kind; simpl; congruence | intros ->; reflexivity].
Qed.

Theorem the_estimate_uses_the_meter_of_user_deploys :
  measured_by_native_meter (scope_of ExploratoryEstimate) = true /\
  measured_by_native_meter (scope_of UserDeploy) = true /\
  charges_a_payer (scope_of ExploratoryEstimate) = false.
Proof. repeat split. Qed.

Theorem historical_body_only_execution_is_refused :
  scope_of HistoricalBodyOnly = CompatibilityError /\
  executes_a_term (scope_of HistoricalBodyOnly) = false.
Proof. split; reflexivity. Qed.

Theorem every_other_kind_executes_its_term : forall kind,
  kind <> HistoricalBodyOnly -> executes_a_term (scope_of kind) = true.
Proof. intros kind other; destruct kind; simpl; congruence. Qed.

Print Assumptions accepted_genesis_is_active.
Print Assumptions an_inactive_genesis_is_refused.
Print Assumptions admission_rejects_body_only.
Print Assumptions selection_is_height_independent.
Print Assumptions selection_contains_only_offered.
Print Assumptions validation_rejects_body_only.
Print Assumptions validation_accepts_offered_only.
Print Assumptions reachable_states_are_offered_only.
Print Assumptions an_inactive_flag_reaches_a_body_only_state.
Print Assumptions all_scopes_is_exhaustive.
Print Assumptions genesis_replay_is_symmetric_with_genesis_play.
Print Assumptions genesis_runs_in_the_trusted_install_scope_at_cost_zero.
Print Assumptions only_user_deploys_charge_a_payer.
Print Assumptions the_estimate_uses_the_meter_of_user_deploys.
Print Assumptions historical_body_only_execution_is_refused.
Print Assumptions every_other_kind_executes_its_term.
