From Stdlib Require Import Lists.List Bool.Bool Relations.Relation_Operators.
Import ListNotations.

Inductive deploy_format := BodyOnly | OfferedFunded.

Definition is_offered (deploy : deploy_format) : bool :=
  match deploy with
  | BodyOnly => false
  | OfferedFunded => true
  end.

Definition admit (active : bool) (deploy : deploy_format) : bool :=
  if active then is_offered deploy else true.

Definition select (height : nat) (active : bool)
    (pending : list deploy_format) : list deploy_format :=
  if active then filter is_offered pending else pending.

Definition validate (active : bool) (deploys : list deploy_format) : bool :=
  if active then forallb is_offered deploys else true.

Theorem active_admission_rejects_body_only : admit true BodyOnly = false.
Proof. reflexivity. Qed.

Theorem active_admission_accepts_offered : admit true OfferedFunded = true.
Proof. reflexivity. Qed.

Theorem active_selection_is_height_independent : forall first second pending,
  select first true pending = select second true pending.
Proof. reflexivity. Qed.

Theorem active_selection_contains_only_offered : forall height pending deploy,
  In deploy (select height true pending) -> deploy = OfferedFunded.
Proof.
  intros height pending deploy included.
  apply filter_In in included as [_ offered].
  destruct deploy; discriminate || reflexivity.
Qed.

Theorem active_validation_rejects_body_only : forall before after,
  validate true (before ++ BodyOnly :: after) = false.
Proof.
  intros before after.
  unfold validate.
  rewrite forallb_app.
  simpl.
  rewrite andb_false_r.
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

Theorem active_step_preserves_offered_only : forall before after,
  offered_only before -> step true before after -> offered_only after.
Proof.
  intros before after [pending_only blocks_only] transition.
  inversion transition; subst; split; simpl.
  - apply Forall_app. split; [exact pending_only |].
    destruct deploy; simpl in *; try congruence.
    constructor; [reflexivity | constructor].
  - exact blocks_only.
  - constructor.
  - constructor; [|exact blocks_only].
    apply Forall_forall. intros deploy included.
    symmetry. now apply active_selection_contains_only_offered in included.
Qed.

Theorem active_trace_preserves_offered_only : forall before after,
  offered_only before ->
  clos_refl_trans network_state (step true) before after ->
  offered_only after.
Proof.
  intros before after initial trace.
  induction trace.
  - eapply active_step_preserves_offered_only; eauto.
  - exact initial.
  - apply IHtrace2. apply IHtrace1. exact initial.
Qed.

Theorem active_reachable_states_are_offered_only : forall state,
  clos_refl_trans network_state (step true)
    {| pending_deploys := []; published_blocks := [] |} state ->
  offered_only state.
Proof.
  intros state trace.
  apply active_trace_preserves_offered_only with
    (before := {| pending_deploys := []; published_blocks := [] |});
    [split; constructor | exact trace].
Qed.

Print Assumptions active_admission_rejects_body_only.
Print Assumptions active_selection_is_height_independent.
Print Assumptions active_selection_contains_only_offered.
Print Assumptions active_validation_rejects_body_only.
Print Assumptions active_reachable_states_are_offered_only.
