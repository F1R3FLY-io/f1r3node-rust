(** Provider ownership at the prepared-program boundary.

    This is a composition/refinement model, not a new evaluator. Immutable
    configuration and mutable provider state are deliberately different types.
    In particular authority tables are NOT modeled as inert caches. The node's
    existing soft-checkpoint wrapper restores only its RSpace projection; this
    file proves exactly that product law and a counterexample to whole-provider
    rollback. Runtime injection of this owner is a separate implementation step.

    Endpoint references denote ownership identity, not public URNs, hashes or
    capability bytes. The constant bundle models cloning the SAME Arc into the
    ten existing services and matcher. It does not prove Arc implementation,
    callback purity, immutable RegistrySnapshot implementations or service
    registration collision checks. Those require source correspondence.
*)
From Stdlib Require Import List.
From CostAccountedRho Require Import PreparedProgramAdmission.
Import ListNotations.
Set Implicit Arguments.

Inductive provider_endpoint :=
| Install | Parse | Construct | Pattern
| TheoremOpen | TheoremPrepare | TheoremCommit | TheoremRevoke
| Reduce | Observe | Matcher.

Definition service_endpoints : list provider_endpoint :=
  [Install; Parse; Construct; Pattern; TheoremOpen; TheoremPrepare;
   TheoremCommit; TheoremRevoke; Reduce; Observe].

Theorem service_roster_complete : forall endpoint,
  In endpoint service_endpoints \/ endpoint = Matcher.
Proof. destruct endpoint; cbn; intuition. Qed.

Theorem service_roster_unique : NoDup service_endpoints.
Proof. repeat constructor; cbn; intuition discriminate. Qed.

Section SharedOwner.
Context {Runtime Snapshot Policy Callbacks Resources : Type}.

(** Resources is the actual enclosing resource owner, not a borrowed path.
    Snapshot denotes the pinned immutable view required by RegistrySnapshot.
    Runtime is ownership identity of one service/capability directory. *)
Record provider_owner := {
  owner_runtime : Runtime;
  owner_snapshot : Snapshot;
  owner_policy : Policy;
  owner_callbacks : Callbacks;
  owner_resources : Resources
}.

Definition shared_endpoints (owner : provider_owner)
    (_ : provider_endpoint) : provider_owner := owner.

Theorem endpoint_and_matcher_share_owner : forall owner endpoint,
  shared_endpoints owner endpoint = shared_endpoints owner Matcher.
Proof. reflexivity. Qed.

Theorem endpoint_configuration_exact : forall owner endpoint,
  owner_snapshot (shared_endpoints owner endpoint) = owner_snapshot owner /\
  owner_policy (shared_endpoints owner endpoint) = owner_policy owner /\
  owner_callbacks (shared_endpoints owner endpoint) = owner_callbacks owner /\
  owner_resources (shared_endpoints owner endpoint) = owner_resources owner.
Proof. intros; repeat split; reflexivity. Qed.

(** A client reference retains the whole owner. Removing the separate driver
    reference cannot change what a surviving endpoint owns. No claim about
    release ordering of external resources is hidden in this value model. *)
Definition release_driver (state : option provider_owner * provider_owner) :=
  (None : option provider_owner, snd state).

Theorem surviving_endpoint_retains_owner : forall driver endpoint,
  snd (release_driver (driver, endpoint)) = endpoint.
Proof. reflexivity. Qed.
End SharedOwner.

Section MutableProvider.
Context {Authority Registrations Diagnostics : Type}.

(** Authority includes installation table, revocations, capability generations,
    exported handles and theorem-space state. Registrations includes retained
    FLT pattern/admission structures. Neither is presumed rollback-inert. *)
Record provider_state := {
  provider_authority : Authority;
  provider_registrations : Registrations;
  provider_diagnostics : Diagnostics
}.

Inductive registration_outcome :=
| RegistrationAccepted (retained : Registrations)
| RegistrationRefused.

Definition finish_registration (before : provider_state)
    (outcome : registration_outcome) : provider_state :=
  match outcome with
  | RegistrationAccepted retained =>
      {| provider_authority := provider_authority before;
         provider_registrations := retained;
         provider_diagnostics := provider_diagnostics before |}
  | RegistrationRefused => before
  end.

Theorem registration_does_not_grant_authority : forall before outcome,
  provider_authority (finish_registration before outcome) = provider_authority before.
Proof. intros before []; reflexivity. Qed.

Theorem refused_registration_preserves_provider : forall before,
  finish_registration before RegistrationRefused = before.
Proof. reflexivity. Qed.

(** Registration precedes RSpace checkpoint/injection. The current prepare
    function finishes its fallible traversal before mutation. Returned refusal
    preserves retained registrations; panics are outside this result model.
    Successful registration remains retained if later evaluation fails. *)
Definition register_before_execution {Host : Type} (state : Host * provider_state)
    (outcome : registration_outcome) :=
  (fst state, finish_registration (snd state) outcome).

Theorem registration_preserves_host : forall Host (state : Host * provider_state) outcome,
  fst (register_before_execution state outcome) = fst state.
Proof. reflexivity. Qed.
End MutableProvider.

Section WrapperProduct.
Context {Host Provider : Type}.

(** The provider argument is the state AFTER service execution. It is not the
    pre-evaluation snapshot. This distinction prevents an unsound rollback
    theorem for installation or prepared-pattern mutations. *)
Definition finish_product (finish_host : Host -> Host) (after : Host * Provider) :=
  (finish_host (fst after), snd after).

Theorem product_host_exact : forall finish_host after,
  fst (finish_product finish_host after) = finish_host (fst after).
Proof. reflexivity. Qed.

Theorem product_provider_exact : forall finish_host after,
  snd (finish_product finish_host after) = snd after.
Proof. reflexivity. Qed.

Theorem changed_provider_is_not_rolled_back : forall finish_host host before after,
  before <> after -> snd (finish_product finish_host (host, after)) <> before.
Proof. intros finish_host host before after Hchange Heq. apply Hchange. symmetry. exact Heq. Qed.
End WrapperProduct.

Section PreparedWrapperInstance.
Context {Diagnostic FinalCost Merge Budget Space Provider : Type}.

Definition finish_provider_wrapper (checkpoint : Space)
    (after : @runtime_state Merge Budget Space * Provider)
    (result : @runtime_result Diagnostic FinalCost Merge) :=
  finish_product
    (fun host => @finish_wrapper Diagnostic FinalCost Merge Budget Space checkpoint host result)
    after.

Theorem provider_wrapper_preserves_charged_budget_and_merge :
  forall checkpoint after result,
  state_budget (fst (finish_provider_wrapper checkpoint after result)) =
    state_budget (fst after) /\
  state_merge (fst (finish_provider_wrapper checkpoint after result)) =
    state_merge (fst after).
Proof. intros. apply wrapper_preserves_budget_and_merge. Qed.

Theorem provider_wrapper_restores_outer_error_space : forall checkpoint after error,
  state_space (fst (finish_provider_wrapper checkpoint after (Escaped error))) = checkpoint.
Proof. intros. apply wrapper_restores_outer_error. Qed.

Theorem provider_wrapper_restores_nonempty_error_space :
  forall checkpoint after cost error rest merge,
  state_space (fst (finish_provider_wrapper checkpoint after
    (Evaluated {| evaluation_cost := cost; evaluation_errors := error :: rest;
                  evaluation_merge := merge |}))) = checkpoint.
Proof. intros. apply wrapper_restores_nonempty_errors. Qed.

Theorem provider_wrapper_retains_postexecution_provider : forall checkpoint after result,
  snd (finish_provider_wrapper checkpoint after result) = snd after.
Proof. reflexivity. Qed.

Theorem successful_wrapper_preserves_all_state : forall checkpoint after cost merge,
  finish_provider_wrapper checkpoint after
    (Evaluated {| evaluation_cost := cost; evaluation_errors := [];
                  evaluation_merge := merge |}) = after.
Proof. intros checkpoint [host provider] cost merge. reflexivity. Qed.
End PreparedWrapperInstance.

Example space_restore_does_not_undo_installation :
  finish_product (fun _ : nat => 4) (9, [17; 23]) = (4, [17; 23]).
Proof. reflexivity. Qed.

Print Assumptions service_roster_complete.
Print Assumptions service_roster_unique.
Print Assumptions endpoint_and_matcher_share_owner.
Print Assumptions endpoint_configuration_exact.
Print Assumptions surviving_endpoint_retains_owner.
Print Assumptions registration_does_not_grant_authority.
Print Assumptions refused_registration_preserves_provider.
Print Assumptions registration_preserves_host.
Print Assumptions product_host_exact.
Print Assumptions product_provider_exact.
Print Assumptions changed_provider_is_not_rolled_back.
Print Assumptions provider_wrapper_preserves_charged_budget_and_merge.
Print Assumptions provider_wrapper_restores_outer_error_space.
Print Assumptions provider_wrapper_restores_nonempty_error_space.
Print Assumptions provider_wrapper_retains_postexecution_provider.
Print Assumptions successful_wrapper_preserves_all_state.
Print Assumptions space_restore_does_not_undo_installation.
