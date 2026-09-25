From Stdlib Require Import List Arith Bool Lia.
Import ListNotations.

Module ProtobufParAccumulation.

Record observed := Observe {
  components : list nat;
  locally_free : list nat;
  connective_used : bool;
  position : nat
}.
Record planned := Plan {
  roots : list (list nat);
  root : nat;
  metadata : list nat;
  connective : bool;
  cursor : nat
}.
Definition lookup (a : list (list nat)) (i : nat) := nth i a [].
Definition observe p := Observe (lookup (roots p) (root p))
  (metadata p) (connective p) (cursor p).

Inductive action :=
| Insert (key : nat)
| SetLocallyFree (bytes : list nat)
| SetConnective (flag : bool)
| Refuse (error : nat).
Record event := Event { update : action; consumed : nat }.

Definition reference_step (maximum : nat) (s : observed) (e : event)
    : observed * option nat :=
  let offset := position s + consumed e in
  match update e with
  | Insert key =>
      if count_occ Nat.eq_dec (components s) key <? maximum then
        (Observe (key :: components s) (locally_free s) (connective_used s) offset, None)
      else (Observe (components s) (locally_free s) (connective_used s) offset, Some 0)
  | SetLocallyFree bytes =>
      (Observe (components s) bytes (connective_used s) offset, None)
  | SetConnective flag => (Observe (components s) (locally_free s) flag offset, None)
  | Refuse error =>
      (Observe (components s) (locally_free s) (connective_used s) offset, Some error)
  end.

Definition planned_step (maximum : nat) (s : planned) (e : event)
    : planned * option nat :=
  let offset := cursor s + consumed e in
  match update e with
  | Insert key =>
      let old := lookup (roots s) (root s) in
      if count_occ Nat.eq_dec old key <? maximum then
        (Plan (roots s ++ [key :: old]) (length (roots s))
          (metadata s) (connective s) offset, None)
      else (Plan (roots s) (root s) (metadata s) (connective s) offset, Some 0)
  | SetLocallyFree bytes =>
      (Plan (roots s) (root s) bytes (connective s) offset, None)
  | SetConnective flag => (Plan (roots s) (root s) (metadata s) flag offset, None)
  | Refuse error =>
      (Plan (roots s) (root s) (metadata s) (connective s) offset, Some error)
  end.

Lemma lookup_appended a value : lookup (a ++ [value]) (length a) = value.
Proof. unfold lookup. rewrite nth_middle. reflexivity. Qed.

Theorem each_field_refines maximum s e :
  let '(next, error) := planned_step maximum s e in
  (observe next, error) = reference_step maximum (observe s) e.
Proof.
  destruct s, e as [action bytes], action; cbn; try reflexivity.
  match goal with |- context [if ?test then _ else _] => destruct test end;
    cbn; try reflexivity. unfold observe, lookup; cbn. now rewrite nth_middle.
Qed.

Fixpoint execute {S : Type} (step : S -> event -> S * option nat)
    (events : list event) (s : S) : S * option nat * list event :=
  match events with
  | [] => (s, None, [])
  | e :: tail =>
      let '(next, error) := step s e in
      match error with
      | Some code => (next, Some code, tail)
      | None => execute step tail next
      end
  end.

Definition observation (result : planned * option nat * list event) :=
  let '(s, error, tail) := result in (observe s, error, tail).

Theorem ordered_fields_refine maximum events s :
  observation (execute (planned_step maximum) events s) =
  execute (reference_step maximum) events (observe s).
Proof.
  revert s. induction events as [|e tail IH]; intros s; cbn; [reflexivity|].
  pose proof (each_field_refines maximum s e) as correspondence.
  destruct (planned_step maximum s e) as [next error] eqn:step.
  rewrite <- correspondence. destruct error; cbn; [reflexivity|apply IH].
Qed.

Theorem earlier_arena_roots_survive a suffix i :
  i < length a -> lookup (a ++ suffix) i = lookup a i.
Proof. intros bounded. unfold lookup. now rewrite app_nth1. Qed.

Theorem insertion_preserves_metadata maximum s key consumed :
  let '(next, _) := planned_step maximum s (Event (Insert key) consumed) in
  metadata next = metadata s /\ connective next = connective s.
Proof.
  cbn. match goal with |- context [if ?test then _ else _] => destruct test end; auto.
Qed.

Theorem metadata_update_preserves_authority maximum s bytes consumed :
  let '(next, _) := planned_step maximum s (Event (SetLocallyFree bytes) consumed) in
  roots next = roots s /\ root next = root s.
Proof. cbn; auto. Qed.

Theorem connective_update_preserves_authority maximum s flag consumed :
  let '(next, _) := planned_step maximum s (Event (SetConnective flag) consumed) in
  roots next = roots s /\ root next = root s.
Proof. cbn; auto. Qed.

Theorem explicit_empty_locally_free_replaces maximum s consumed :
  metadata (fst (planned_step maximum s (Event (SetLocallyFree []) consumed))) = [].
Proof. reflexivity. Qed.

Theorem explicit_false_connective_replaces maximum s consumed :
  connective (fst (planned_step maximum s (Event (SetConnective false) consumed))) = false.
Proof. reflexivity. Qed.

Theorem insertion_failure_preserves_authority maximum s key consumed :
  maximum <= count_occ Nat.eq_dec (lookup (roots s) (root s)) key ->
  let '(next, error) := planned_step maximum s (Event (Insert key) consumed) in
  roots next = roots s /\ root next = root s /\ metadata next = metadata s /\
  connective next = connective s /\ error = Some 0.
Proof.
  intros limit.
  assert ((count_occ Nat.eq_dec (lookup (roots s) (root s)) key <? maximum) = false)
    as full by (apply Nat.ltb_ge; exact limit).
  unfold planned_step. cbn -[Nat.ltb lookup]. rewrite full.
  cbn; repeat split; reflexivity.
Qed.

Theorem refusal_preserves_committed_state maximum s error consumed :
  let '(next, failure) := planned_step maximum s (Event (Refuse error) consumed) in
  roots next = roots s /\ root next = root s /\ metadata next = metadata s /\
  connective next = connective s /\ cursor next = cursor s + consumed /\
  failure = Some error.
Proof. cbn; repeat split; reflexivity. Qed.

Theorem refusal_does_not_visit_suffix maximum s error consumed tail :
  execute (planned_step maximum) (Event (Refuse error) consumed :: tail) s =
  (Plan (roots s) (root s) (metadata s) (connective s) (cursor s + consumed), Some error, tail).
Proof. reflexivity. Qed.

Theorem derived_metadata_mutant_changes_observation :
  Observe [1] [] false 1 <> Observe [1] [7] true 1.
Proof. discriminate. Qed.

Theorem union_metadata_mutant_changes_explicit_reset :
  metadata (fst (planned_step 5 (Plan [[]] 0 [7] true 0)
    (Event (SetLocallyFree []) 1))) <> [7].
Proof. discriminate. Qed.

Print Assumptions each_field_refines.
Print Assumptions ordered_fields_refine.
Print Assumptions earlier_arena_roots_survive.
Print Assumptions insertion_preserves_metadata.
Print Assumptions metadata_update_preserves_authority.
Print Assumptions connective_update_preserves_authority.
Print Assumptions explicit_empty_locally_free_replaces.
Print Assumptions explicit_false_connective_replaces.
Print Assumptions insertion_failure_preserves_authority.
Print Assumptions refusal_preserves_committed_state.
Print Assumptions refusal_does_not_visit_suffix.
Print Assumptions derived_metadata_mutant_changes_observation.
Print Assumptions union_metadata_mutant_changes_explicit_reset.

End ProtobufParAccumulation.

Module ConstructionResources.

Section HeapTransaction.
Context {A : Type}.
Definition heap := nat -> A.
Definition put (h : heap) i value : heap :=
  fun j => if Nat.eq_dec i j then value else h j.

Lemma put_same h i value : put h i value i = value.
Proof. unfold put. destruct (Nat.eq_dec i i); congruence. Qed.

Lemma put_other h i j value : i <> j -> put h i value j = h j.
Proof. intros H. unfold put. destruct (Nat.eq_dec i j); congruence. Qed.

Definition agrees (protected : nat -> Prop) (left right : heap) :=
  forall i, protected i -> left i = right i.

Theorem private_write_preserves_protected h protected i value :
  ~ protected i -> agrees protected (put h i value) h.
Proof.
  intros private j observed. apply put_other. intro E; subst; contradiction.
Qed.

Fixpoint rollback (undo : list (nat * A)) (h : heap) : heap :=
  match undo with [] => h | (i, old) :: tail => rollback tail (put h i old) end.

Fixpoint apply_writes (writes : list (nat * A)) (h : heap)
    (undo : list (nat * A)) : heap * list (nat * A) :=
  match writes with
  | [] => (h, undo)
  | (i, value) :: tail => apply_writes tail (put h i value) ((i, h i) :: undo)
  end.

Lemma rollback_ext undo h k :
  (forall i, h i = k i) -> forall i, rollback undo h i = rollback undo k i.
Proof.
  revert h k. induction undo as [|[j old] tail IH]; intros h k H i; cbn; auto.
  apply IH. intros index. unfold put. destruct (Nat.eq_dec j index); auto.
Qed.

Lemma overwrite_then_restore h i value :
  forall j, put (put h i value) i (h i) j = h j.
Proof.
  intros j. unfold put. destruct (Nat.eq_dec i j); subst; reflexivity.
Qed.

Theorem reverse_journal_restores writes h undo :
  let '(updated, journal) := apply_writes writes h undo in
  forall i, rollback journal updated i = rollback undo h i.
Proof.
  revert h undo. induction writes as [|[i value] tail IH]; intros h undo; cbn.
  - reflexivity.
  - specialize (IH (put h i value) ((i, h i) :: undo)).
    destruct (apply_writes tail (put h i value) ((i, h i) :: undo)) as [next log].
    intros j. rewrite IH. cbn. apply rollback_ext. apply overwrite_then_restore.
Qed.

Theorem successful_private_writes_preserve_others writes h undo protected :
  Forall (fun write => ~ protected (fst write)) writes ->
  let '(updated, _) := apply_writes writes h undo in agrees protected updated h.
Proof.
  intros H. revert h undo. induction H as [|[i value] tail private rest IH]; intros h undo; cbn.
  - intros j Hj; reflexivity.
  - specialize (IH (put h i value) ((i, h i) :: undo)).
    destruct (apply_writes tail (put h i value) ((i, h i) :: undo)) as [next log].
    intros j Hj. rewrite (IH j Hj). apply put_other.
    intro E; subst. apply private. exact Hj.
Qed.

End HeapTransaction.

Definition seal_owner (_ : nat) := 0.

Record owned_node := OwnedNode { node_owner : nat; node_edges : list nat; node_value : nat }.
Definition owned_heap := nat -> owned_node.
Definition sealed_edges (h : owned_heap) :=
  forall parent child, node_owner (h parent) = 0 ->
    In child (node_edges (h parent)) -> node_owner (h child) = 0.

Inductive reachable (h : owned_heap) : nat -> nat -> Prop :=
| ReachSelf root : reachable h root root
| ReachChild root child target :
    In child (node_edges (h root)) -> reachable h child target -> reachable h root target.

Theorem sealed_root_has_only_sealed_descendants h root target :
  sealed_edges h -> node_owner (h root) = 0 -> reachable h root target ->
  node_owner (h target) = 0.
Proof.
  intros closed sealed reach. induction reach; auto.
  apply IHreach. eapply closed; eauto.
Qed.

Theorem private_owner_write_cannot_change_snapshot h root index replacement :
  sealed_edges h -> node_owner (h root) = 0 -> node_owner (h index) <> 0 ->
  forall target, reachable h root target -> put h index replacement target = h target.
Proof.
  intros closed sealed private target reach. apply put_other.
  intro E; subst. apply private. eapply sealed_root_has_only_sealed_descendants; eauto.
Qed.

Definition exclusive_owner h owner other_root :=
  forall target, reachable h other_root target -> node_owner (h target) <> owner.

Theorem exclusive_write_preserves_other_builder h owner other_root index replacement :
  exclusive_owner h owner other_root -> node_owner (h index) = owner ->
  forall target, reachable h other_root target -> put h index replacement target = h target.
Proof.
  intros exclusive owned target reach. apply put_other.
  intro E; subst index. apply (exclusive target reach). exact owned.
Qed.

Definition seal_node (n : owned_node) := OwnedNode 0 (node_edges n) (node_value n).

Theorem postorder_seal_preserves_closed_edges h index :
  sealed_edges h ->
  (forall child, In child (node_edges (h index)) -> node_owner (h child) = 0) ->
  sealed_edges (put h index (seal_node (h index))).
Proof.
  intros closed children parent child sealed edge.
  unfold put in *. destruct (Nat.eq_dec index parent) as [E|E].
  - subst parent. cbn in edge. destruct (Nat.eq_dec index child); cbn; auto.
  - destruct (Nat.eq_dec index child); cbn; eauto.
Qed.

Theorem seal_preserves_payload_and_edges h index target :
  node_edges (put h index (seal_node (h index)) target) = node_edges (h target) /\
  node_value (put h index (seal_node (h index)) target) = node_value (h target).
Proof. unfold put. destruct (Nat.eq_dec index target); subst; cbn; auto. Qed.
Theorem seal_idempotent owner : seal_owner (seal_owner owner) = seal_owner owner.
Proof. reflexivity. Qed.

Theorem sealed_owner_cannot_be_reused live : live <> 0 -> live <> seal_owner live.
Proof. auto. Qed.

Theorem fresh_owner_above_prior_ids count old :
  old <= count -> old <> S count.
Proof. lia. Qed.

Section CandidateSuffix.
Context {A : Type}.

Theorem candidate_rewind_restores_prefix (prefix suffix : list A) :
  firstn (length prefix) (prefix ++ suffix) = prefix.
Proof. rewrite firstn_app, firstn_all, Nat.sub_diag. cbn. now rewrite app_nil_r. Qed.

Theorem candidate_rewind_preserves_prior_handle (prefix suffix : list A) handle :
  handle < length prefix ->
  nth_error (firstn (length prefix) (prefix ++ suffix)) handle =
  nth_error (prefix ++ suffix) handle.
Proof.
  intros H. rewrite candidate_rewind_restores_prefix, nth_error_app1; auto.
Qed.

Theorem nested_candidate_rewinds_preserve_outer (prefix outer inner : list A) :
  firstn (length prefix)
    (firstn (length (prefix ++ outer)) ((prefix ++ outer) ++ inner)) = prefix.
Proof. rewrite candidate_rewind_restores_prefix. apply candidate_rewind_restores_prefix. Qed.

Theorem parent_append_must_follow_rewind (prefix candidate accepted : list A) :
  firstn (length prefix) (prefix ++ candidate) ++ accepted = prefix ++ accepted.
Proof. now rewrite candidate_rewind_restores_prefix. Qed.

Theorem retire_suffix_has_no_prior_handles (prefix suffix : list A) handle :
  length prefix <= handle -> nth_error prefix handle = None.
Proof. intros H. apply nth_error_None. exact H. Qed.

Theorem confined_live_handles_survive_rewind (prefix suffix : list A) handles :
  Forall (fun handle => handle < length prefix) handles ->
  map (nth_error (firstn (length prefix) (prefix ++ suffix))) handles =
  map (nth_error (prefix ++ suffix)) handles.
Proof.
  intros H. induction H; cbn; auto.
  rewrite candidate_rewind_preserves_prior_handle; auto. now rewrite IHForall.
Qed.

End CandidateSuffix.

Inductive tree := Empty | Branch (left : tree) (key count : nat) (right : tree).
Fixpoint entries t : list (nat * nat) :=
  match t with Empty => [] | Branch l key count r => entries l ++ (key, count) :: entries r end.

Theorem left_rotation_preserves_order a b c x nx y ny :
  entries (Branch a x nx (Branch b y ny c)) =
  entries (Branch (Branch a x nx b) y ny c).
Proof. cbn. repeat rewrite <- app_assoc. reflexivity. Qed.

Theorem right_rotation_preserves_order a b c x nx y ny :
  entries (Branch (Branch a x nx b) y ny c) =
  entries (Branch a x nx (Branch b y ny c)).
Proof. symmetry. apply left_rotation_preserves_order. Qed.

Theorem double_rotation_preserves_order a b c d x nx y ny z nz :
  entries (Branch a x nx (Branch (Branch b y ny c) z nz d)) =
  entries (Branch (Branch a x nx b) y ny (Branch c z nz d)).
Proof. cbn. repeat rewrite <- app_assoc. reflexivity. Qed.

Inductive drain_step : nat -> nat -> Prop :=
| DrainNode n : drain_step (S n) n
| DrainAttach n : drain_step (S n) (S n).

Theorem terminal_drain_does_not_grow before after :
  drain_step before after -> after <= before.
Proof. intros H; destruct H; lia. Qed.

Inductive pending := PendingNode | PendingAttach | PendingEpm.
Definition epm_control_valid depth frames :=
  In PendingEpm frames -> 0 < depth.

Theorem zero_epm_depth_excludes_pending_descent frames :
  epm_control_valid 0 frames -> ~ In PendingEpm frames.
Proof. unfold epm_control_valid. intros valid present. specialize (valid present). lia. Qed.

Definition terminal_control remaining saved_inputs depth :=
  remaining = 0 /\ saved_inputs = 0 /\ depth = 0.

Theorem terminal_control_excludes_nested_descent remaining saved_inputs depth frames :
  terminal_control remaining saved_inputs depth -> epm_control_valid depth frames ->
  remaining = 0 /\ saved_inputs = 0 /\ ~ In PendingEpm frames.
Proof.
  intros [R [S D]] valid. subst. split; [reflexivity|].
  split; [reflexivity|]. apply zero_epm_depth_excludes_pending_descent; exact valid.
Qed.

Inductive shrink_schedule : nat -> list nat -> Prop :=
| ShrinkDone bound : shrink_schedule bound []
| ShrinkNext bound target tail :
    0 < target -> 2 * target <= bound -> shrink_schedule target tail ->
    shrink_schedule bound (target :: tail).

Theorem geometric_retirement_has_linear_move_budget bound requests :
  shrink_schedule bound requests -> fold_right Nat.add 0 requests <= bound.
Proof. intros H; induction H; cbn; lia. Qed.

Theorem geometric_retirement_call_bound bound requests :
  0 < bound -> shrink_schedule bound requests -> 2 ^ length requests <= bound.
Proof.
  intros positive schedule. revert positive. induction schedule; intros positive; cbn; try lia.
Qed.

Theorem zero_retirement_preserves_frames {A} (frames : list A) :
  length frames = 0 -> frames = [].
Proof. apply length_zero_iff_nil. Qed.

Theorem wrong_journal_order_mutant_is_detected :
  rollback [(1, 0); (1, 7)] (put (fun _ => 0) 1 9) 1 <> 0.
Proof. discriminate. Qed.

Theorem early_rewind_mutant_loses_parent_update :
  firstn 1 ([10] ++ [20] ++ [30]) <> [10; 30].
Proof. discriminate. Qed.

Print Assumptions private_write_preserves_protected.
Print Assumptions sealed_root_has_only_sealed_descendants.
Print Assumptions private_owner_write_cannot_change_snapshot.
Print Assumptions exclusive_write_preserves_other_builder.
Print Assumptions postorder_seal_preserves_closed_edges.
Print Assumptions seal_preserves_payload_and_edges.
Print Assumptions reverse_journal_restores.
Print Assumptions successful_private_writes_preserve_others.
Print Assumptions seal_idempotent.
Print Assumptions sealed_owner_cannot_be_reused.
Print Assumptions fresh_owner_above_prior_ids.
Print Assumptions candidate_rewind_restores_prefix.
Print Assumptions candidate_rewind_preserves_prior_handle.
Print Assumptions nested_candidate_rewinds_preserve_outer.
Print Assumptions parent_append_must_follow_rewind.
Print Assumptions retire_suffix_has_no_prior_handles.
Print Assumptions confined_live_handles_survive_rewind.
Print Assumptions left_rotation_preserves_order.
Print Assumptions right_rotation_preserves_order.
Print Assumptions double_rotation_preserves_order.
Print Assumptions terminal_drain_does_not_grow.
Print Assumptions zero_epm_depth_excludes_pending_descent.
Print Assumptions terminal_control_excludes_nested_descent.
Print Assumptions geometric_retirement_has_linear_move_budget.
Print Assumptions geometric_retirement_call_bound.
Print Assumptions zero_retirement_preserves_frames.
Print Assumptions wrong_journal_order_mutant_is_detected.
Print Assumptions early_rewind_mutant_loses_parent_update.

End ConstructionResources.
