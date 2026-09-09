From Stdlib Require Import Lists.List Bool Arith Lia.
Import ListNotations.

Section Publication.
Context {Key Dependency : Type}.
Context (key_eq : forall x y : Key, {x = y} + {x <> y}).
Variable required : Key -> list Dependency.

Definition store := Key -> option (list Dependency).
Definition complete (s : store) :=
  forall k deps, s k = Some deps -> incl (required k) deps.
Definition existing (s : store) (k : Key) :=
  match s k with Some deps => deps | None => [] end.
Definition publish (s : store) (key : Key) (success : bool) : store :=
  if success then fun k => if key_eq k key
    then Some (existing s key ++ required key) else s k else s.

Theorem failed_write_preserves_all_rows : forall s k,
  publish s k false = s.
Proof. reflexivity. Qed.

Theorem successful_write_contains_all_dependencies : forall s k,
  publish s k true k = Some (existing s k ++ required k).
Proof. intros. unfold publish. destruct (key_eq k k); congruence. Qed.

Theorem independent_rows_unchanged : forall s k other success,
  other <> k -> publish s k success other = s other.
Proof. intros. unfold publish. destruct success; [destruct (key_eq other k)|]; congruence. Qed.

Theorem publication_preserves_completeness : forall s k success,
  complete s -> complete (publish s k success).
Proof.
  intros s k success HC other deps HR. unfold publish in HR.
  destruct success.
  - destruct (key_eq other k) as [->|HN].
    + inversion HR; subst. unfold incl. intros. apply in_or_app. auto.
    + eapply HC; eauto.
  - eapply HC; eauto.
Qed.

Theorem repairing_a_partial_row_preserves_old_and_required_dependencies : forall s k deps,
  publish s k true k = Some deps ->
  incl (existing s k) deps /\ incl (required k) deps.
Proof.
  intros s k deps H. rewrite successful_write_contains_all_dependencies in H.
  inversion H; subst. split; unfold incl; intros; apply in_or_app; auto.
Qed.

Definition has_row (s : store) (k : Key) : bool :=
  match s k with Some _ => true | None => false end.

Theorem explicit_empty_row_is_owner : forall s k,
  s k = Some [] -> has_row s k = true.
Proof. intros. unfold has_row. rewrite H. reflexivity. Qed.

Theorem implicit_parent_is_not_owner : forall s k,
  s k = None -> has_row s k = false.
Proof. intros. unfold has_row. rewrite H. reflexivity. Qed.

Definition repair_from_stored (s : store) (k : Key)
  (stored : option (list Dependency)) (_incoming : list Dependency) : store :=
  match stored with
  | None => s
  | Some checked => fun other => if key_eq other k
      then Some (existing s k ++ checked) else s other
  end.

Theorem quarantine_repair_is_independent_of_incoming_dependencies : forall s k stored first second,
  repair_from_stored s k stored first = repair_from_stored s k stored second.
Proof. reflexivity. Qed.

Theorem absent_stored_identity_cannot_publish_dependencies : forall s k incoming,
  repair_from_stored s k None incoming = s.
Proof. reflexivity. Qed.
End Publication.

Section Maintenance.
Context {Policy : Type}.
Record request := Request {
  received : bool;
  in_buffer : bool;
  timestamp : nat;
  initial_timestamp : nat;
  policy : Policy
}.

Definition stale (now lifetime : nat) (r : request) : bool :=
  received r && negb (in_buffer r) && (lifetime <? now - initial_timestamp r).
Definition maintain (now lifetime : nat) (r : request) : request :=
  if stale now lifetime r then
    Request false (in_buffer r) now (initial_timestamp r) (policy r)
  else r.

Theorem maintenance_preserves_policy : forall now lifetime r,
  policy (maintain now lifetime r) = policy r /\
  initial_timestamp (maintain now lifetime r) = initial_timestamp r.
Proof. intros. unfold maintain. destruct (stale now lifetime r); simpl; auto. Qed.

Theorem handoff_before_maintenance_is_unchanged : forall now lifetime r,
  received r = false -> maintain now lifetime r = r.
Proof. intros. unfold maintain, stale. rewrite H. reflexivity. Qed.

Theorem durable_receipt_is_not_reopened : forall now lifetime r,
  in_buffer r = true -> maintain now lifetime r = r.
Proof.
  intros. unfold maintain, stale. rewrite H.
  destruct (received r); reflexivity.
Qed.

Theorem expired_received_entry_becomes_retry_ready : forall now lifetime r,
  stale now lifetime r = true -> received (maintain now lifetime r) = false.
Proof. intros. unfold maintain. rewrite H. reflexivity. Qed.
End Maintenance.

Print Assumptions failed_write_preserves_all_rows.
Print Assumptions successful_write_contains_all_dependencies.
Print Assumptions independent_rows_unchanged.
Print Assumptions publication_preserves_completeness.
Print Assumptions repairing_a_partial_row_preserves_old_and_required_dependencies.
Print Assumptions explicit_empty_row_is_owner.
Print Assumptions implicit_parent_is_not_owner.
Print Assumptions quarantine_repair_is_independent_of_incoming_dependencies.
Print Assumptions absent_stored_identity_cannot_publish_dependencies.
Print Assumptions maintenance_preserves_policy.
Print Assumptions handoff_before_maintenance_is_unchanged.
Print Assumptions durable_receipt_is_not_reopened.
Print Assumptions expired_received_entry_becomes_retry_ready.
