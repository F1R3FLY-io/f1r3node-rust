(* DR-73: approved-genesis version adoption applies only to a genesis that
   carries a resource policy.

   Rust correspondence:
   - [adopted_version]: casper.rs [adopted_casper_version], called by
     [hash_set_casper] before [AdoptedResourcePolicy::load].
   - [adopt_accepts]: genesis_resource_policy.rs [GenesisResourcePolicy::adopt],
     which requires the running casper_version to equal the protocol version of
     the policy schedule.
   - [context_valid]: [GenesisResourcePolicy::load], whose [validate_context]
     binds the policy schedule to the approved header version.

   DR-99 extends the model to a node that joins after genesis (section
   "Joined nodes" below). *)

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

(* Joined nodes (DR-99).

   A node that joins after genesis restores from an anchor block, and its
   approved block is that anchor. The policy record is a genesis template
   constant, so the record read at the anchor state is the genesis record
   ([sealed_policy]). The policy context is checked against the genesis block
   that [resolve_policy_genesis] returns ([context_valid (genesis node)]).

   Rust correspondence:
   - [anchor]: the approved block of [hash_set_casper]. Its header version is
     the [approved_version] argument of [adopted_casper_version], and its
     [policy_version] is the record that [find_genesis_resource_policy] reads
     at the anchor state.
   - [genesis]: the block that [resolve_policy_genesis] returns.
     [GenesisResourcePolicy::load_at] checks [validate_context] on it. *)
Record joined_node := {
  anchor : approved_genesis;
  genesis : approved_genesis
}.

Definition sealed_policy (node : joined_node) : Prop :=
  policy_version (anchor node) = policy_version (genesis node).

(* A joined node that passes the adopt check runs the genesis version. *)
Theorem joined_node_runs_the_genesis_version :
  forall node local schedule_version,
    context_valid (genesis node) ->
    policy_version (genesis node) = Some schedule_version ->
    adopt_accepts (adopted_version (anchor node) local) schedule_version = true ->
    adopted_version (anchor node) local = header_version (genesis node).
Proof.
  intros node local schedule_version valid policy accepted.
  unfold adopt_accepts in accepted.
  apply Z.eqb_eq in accepted.
  rewrite accepted.
  exact (valid schedule_version policy).
Qed.

(* A joined node whose anchor carries the genesis version passes the adopt
   check, so the node starts. *)
Theorem joined_anchor_at_the_genesis_version_passes_adopt_check :
  forall node local schedule_version,
    sealed_policy node ->
    context_valid (genesis node) ->
    policy_version (genesis node) = Some schedule_version ->
    header_version (anchor node) = header_version (genesis node) ->
    adopt_accepts (adopted_version (anchor node) local) schedule_version = true.
Proof.
  intros node local schedule_version sealed valid policy same_version.
  assert (anchor_policy : policy_version (anchor node) = Some schedule_version).
  { rewrite sealed. exact policy. }
  rewrite (policy_genesis_adopts_header_version (anchor node) local schedule_version
    anchor_policy).
  unfold adopt_accepts.
  apply Z.eqb_eq.
  rewrite same_version.
  symmetry.
  exact (valid schedule_version policy).
Qed.

(* A joined node whose anchor carries another version fails the adopt check,
   so the node does not start with a version other than the genesis version. *)
Theorem joined_anchor_version_mismatch_fails_adopt_check :
  forall node local schedule_version,
    sealed_policy node ->
    context_valid (genesis node) ->
    policy_version (genesis node) = Some schedule_version ->
    header_version (anchor node) <> header_version (genesis node) ->
    adopt_accepts (adopted_version (anchor node) local) schedule_version = false.
Proof.
  intros node local schedule_version sealed valid policy mismatch.
  assert (anchor_policy : policy_version (anchor node) = Some schedule_version).
  { rewrite sealed. exact policy. }
  rewrite (policy_genesis_adopts_header_version (anchor node) local schedule_version
    anchor_policy).
  unfold adopt_accepts.
  apply Z.eqb_neq.
  rewrite (valid schedule_version policy).
  exact mismatch.
Qed.

(* Policy identity (DR-99). [resolve_policy_genesis] returns a parentless
   approved block itself, and otherwise the authenticated genesis copy that
   the node holds. The identity of the adopted policy is the post-state root
   of the returned block. *)
Record node_view := {
  approved_root : Z;
  approved_parentless : bool;
  resolved_genesis_root : Z
}.

Definition policy_identity (view : node_view) : Z :=
  if approved_parentless view then approved_root view else resolved_genesis_root view.

(* Before DR-99, the identity was the post-state root of the approved block. *)
Definition anchor_bound_identity (view : node_view) : Z := approved_root view.

(* A view of the chain whose genesis post-state root is [root]: a parentless
   approved block is that genesis, and an authenticated genesis copy has that
   post-state root. *)
Definition view_of_chain (root : Z) (view : node_view) : Prop :=
  (approved_parentless view = true -> approved_root view = root) /\
  (approved_parentless view = false -> resolved_genesis_root view = root).

Lemma policy_identity_is_the_genesis_root : forall root view,
  view_of_chain root view -> policy_identity view = root.
Proof.
  intros root view [parentless joined].
  unfold policy_identity.
  destruct (approved_parentless view).
  - exact (parentless eq_refl).
  - exact (joined eq_refl).
Qed.

(* A genesis participant and a joined node of one chain adopt one identity,
   whatever their anchors. *)
Theorem adopted_identity_is_anchor_independent : forall root first second,
  view_of_chain root first ->
  view_of_chain root second ->
  policy_identity first = policy_identity second.
Proof.
  intros root first second first_chain second_chain.
  rewrite (policy_identity_is_the_genesis_root root first first_chain).
  rewrite (policy_identity_is_the_genesis_root root second second_chain).
  reflexivity.
Qed.

(* Negative control: the identity before DR-99 separates a genesis participant
   from a joined node of the same chain. *)
Theorem anchor_bound_identity_splits_joined_nodes :
  exists root first second,
    view_of_chain root first /\
    view_of_chain root second /\
    anchor_bound_identity first <> anchor_bound_identity second.
Proof.
  exists 0.
  exists {| approved_root := 0; approved_parentless := true; resolved_genesis_root := 0 |}.
  exists {| approved_root := 1; approved_parentless := false; resolved_genesis_root := 0 |}.
  split; [| split].
  - split; intro shape; reflexivity.
  - split; intro shape.
    + cbn in shape. discriminate shape.
    + reflexivity.
  - cbn. discriminate.
Qed.

Print Assumptions legacy_genesis_keeps_local_version.
Print Assumptions policy_genesis_adopts_header_version.
Print Assumptions adopted_policy_passes_adopt_check.
Print Assumptions unadopted_mismatch_fails_adopt_check.
Print Assumptions policy_genesis_version_is_node_independent.
Print Assumptions joined_node_runs_the_genesis_version.
Print Assumptions joined_anchor_at_the_genesis_version_passes_adopt_check.
Print Assumptions joined_anchor_version_mismatch_fails_adopt_check.
Print Assumptions policy_identity_is_the_genesis_root.
Print Assumptions adopted_identity_is_anchor_independent.
Print Assumptions anchor_bound_identity_splits_joined_nodes.
