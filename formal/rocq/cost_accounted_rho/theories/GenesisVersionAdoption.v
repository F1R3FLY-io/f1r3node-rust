(* DR-73: approved-genesis version adoption applies only to a genesis that
   carries a resource policy.

   Rust correspondence:
   - [adopted_version]: casper.rs [adopted_casper_version], called by
     [hash_set_casper] before [AdoptedResourcePolicy::load].
   - [adopt_accepts]: genesis_resource_policy.rs [GenesisResourcePolicy::adopt],
     which requires the running casper_version to equal the protocol version of
     the policy schedule.
   - [context_valid]: [GenesisResourcePolicy::load], whose [validate_context]
     binds the policy schedule to the approved header version. *)

From Stdlib Require Import ZArith.ZArith Bool.Bool.
Open Scope Z_scope.

Record approved_genesis := {
  header_version : Z;
  policy_version : option Z
}.

Definition adopted_version (approved : approved_genesis) (local : Z) : Z :=
  match policy_version approved with
  | Some _ => header_version approved
  | None => local
  end.

Definition adopt_accepts (running schedule_version : Z) : bool :=
  Z.eqb running schedule_version.

Definition context_valid (approved : approved_genesis) : Prop :=
  forall schedule_version,
    policy_version approved = Some schedule_version ->
    schedule_version = header_version approved.

(* A legacy genesis carries no policy, so the general Casper path keeps the
   local version exactly as on dev. *)
Theorem legacy_genesis_keeps_local_version : forall approved local,
  policy_version approved = None ->
  adopted_version approved local = local.
Proof.
  intros approved local no_policy.
  unfold adopted_version.
  rewrite no_policy.
  reflexivity.
Qed.

(* A policy-carrying genesis adopts the approved header version. *)
Theorem policy_genesis_adopts_header_version :
  forall approved local schedule_version,
    policy_version approved = Some schedule_version ->
    adopted_version approved local = header_version approved.
Proof.
  intros approved local schedule_version policy.
  unfold adopted_version.
  rewrite policy.
  reflexivity.
Qed.

(* After adoption, the policy check in [adopt] always succeeds. *)
Theorem adopted_policy_passes_adopt_check :
  forall approved local schedule_version,
    context_valid approved ->
    policy_version approved = Some schedule_version ->
    adopt_accepts (adopted_version approved local) schedule_version = true.
Proof.
  intros approved local schedule_version valid policy.
  rewrite (policy_genesis_adopts_header_version approved local schedule_version policy).
  unfold adopt_accepts.
  apply Z.eqb_eq.
  symmetry.
  exact (valid schedule_version policy).
Qed.

(* Without adoption, a node whose local version differs from the approved
   header fails the policy check. This is why adoption exists. *)
Theorem unadopted_mismatch_fails_adopt_check :
  forall approved local schedule_version,
    context_valid approved ->
    policy_version approved = Some schedule_version ->
    local <> header_version approved ->
    adopt_accepts local schedule_version = false.
Proof.
  intros approved local schedule_version valid policy mismatch.
  unfold adopt_accepts.
  apply Z.eqb_neq.
  rewrite (valid schedule_version policy).
  exact mismatch.
Qed.

(* Nodes with different local configurations derive the same running version
   for a policy-carrying genesis, so they agree on the protocol rules. *)
Theorem policy_genesis_version_is_node_independent :
  forall approved first second schedule_version,
    policy_version approved = Some schedule_version ->
    adopted_version approved first = adopted_version approved second.
Proof.
  intros approved first second schedule_version policy.
  rewrite (policy_genesis_adopts_header_version approved first schedule_version policy).
  rewrite (policy_genesis_adopts_header_version approved second schedule_version policy).
  reflexivity.
Qed.

Print Assumptions legacy_genesis_keeps_local_version.
Print Assumptions policy_genesis_adopts_header_version.
Print Assumptions adopted_policy_passes_adopt_check.
Print Assumptions unadopted_mismatch_fails_adopt_check.
Print Assumptions policy_genesis_version_is_node_independent.
