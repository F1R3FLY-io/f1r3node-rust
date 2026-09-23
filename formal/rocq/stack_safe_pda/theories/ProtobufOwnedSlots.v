From Stdlib Require Import List Bool Arith Lia.
Import ListNotations.
Set Implicit Arguments.

Module ProtobufOwnedSlots.

Definition reopen {V} (fallback : V) (slot : option V) : V :=
  match slot with Some child => child | None => fallback end.

Definition take_slot {V} (slot : option V) : option V * option V :=
  (slot, None).

Definition take_reopen {V} (fallback : V) (slot : option V) : V * option V :=
  (reopen fallback slot, None).

Theorem absent_slot_commutes {V W} (denote : V -> W) :
  option_map denote None = None.
Proof. reflexivity. Qed.

Theorem present_slot_commutes {V W} (denote : V -> W) child :
  option_map denote (Some child) = Some (denote child).
Proof. reflexivity. Qed.

Theorem absent_and_present_default_remain_distinct {V W}
    (denote : V -> W) fallback :
  option_map denote None <> option_map denote (Some fallback).
Proof. discriminate. Qed.

Theorem take_slot_commutes {V W} (denote : V -> W) slot :
  (option_map denote (fst (take_slot slot)),
   option_map denote (snd (take_slot slot))) =
  take_slot (option_map denote slot).
Proof. reflexivity. Qed.

Theorem reopen_commutes {V W} (denote : V -> W) source_default target_default slot :
  denote source_default = target_default ->
  denote (reopen source_default slot) =
  reopen target_default (option_map denote slot).
Proof. intros defaults. destruct slot; cbn; [reflexivity|exact defaults]. Qed.

Theorem take_reopen_commutes {V W} (denote : V -> W)
    source_default target_default slot :
  denote source_default = target_default ->
  (denote (fst (take_reopen source_default slot)),
   option_map denote (snd (take_reopen source_default slot))) =
  take_reopen target_default (option_map denote slot).
Proof.
  intros defaults. destruct slot; cbn; [reflexivity|now rewrite defaults].
Qed.

Theorem reopen_present_preserves_child {V} (fallback child : V) :
  take_reopen fallback (Some child) = (child, None).
Proof. reflexivity. Qed.

Definition append_child {V} (children : list V) (child : V) := children ++ [child].

Theorem repeated_default_commutes {V W} (denote : V -> W) :
  map denote [] = [].
Proof. reflexivity. Qed.

Theorem repeated_append_commutes {V W} (denote : V -> W) children child :
  map denote (append_child children child) =
  append_child (map denote children) (denote child).
Proof. unfold append_child. rewrite map_app. reflexivity. Qed.

Theorem repeated_append_preserves_order {V} (children : list V) first second :
  append_child (append_child children first) second = children ++ [first; second].
Proof. unfold append_child. now rewrite <- app_assoc. Qed.

Section Keyed.
Context {K : Type}.
Variable same_key : forall left right : K, {left = right} + {left <> right}.

Definition bindings (V : Type) := list (K * V).

Fixpoint erase_key {V} (key : K) (items : bindings V) : bindings V :=
  match items with
  | [] => []
  | (found, value) :: tail =>
      if same_key key found then erase_key key tail
      else (found, value) :: erase_key key tail
  end.

Definition replace_key {V} (key : K) (value : V) (items : bindings V) :=
  (key, value) :: erase_key key items.

Fixpoint lookup_key {V} (key : K) (items : bindings V) : option V :=
  match items with
  | [] => None
  | (found, value) :: tail =>
      if same_key key found then Some value else lookup_key key tail
  end.

Definition denote_entry {V W} (denote : V -> W) (entry : K * V) :=
  (fst entry, denote (snd entry)).

Definition denote_bindings {V W} (denote : V -> W) (items : bindings V) :=
  map (denote_entry denote) items.

Definition default_entry {V} (default_key : K) (default_value : V) :=
  (default_key, default_value).

Definition set_entry_key {V} (key : K) (entry : K * V) := (key, snd entry).
Definition set_entry_value {V} (value : V) (entry : K * V) := (fst entry, value).

Theorem default_entry_commutes {V W} (denote : V -> W)
    key source_default target_default :
  denote source_default = target_default ->
  denote_entry denote (default_entry key source_default) =
  default_entry key target_default.
Proof. intros defaults. unfold denote_entry, default_entry; cbn. now rewrite defaults. Qed.

Theorem entry_key_update_commutes {V W} (denote : V -> W) key entry :
  denote_entry denote (set_entry_key key entry) =
  set_entry_key key (denote_entry denote entry).
Proof. reflexivity. Qed.

Theorem entry_value_update_commutes {V W} (denote : V -> W) value entry :
  denote_entry denote (set_entry_value value entry) =
  set_entry_value (denote value) (denote_entry denote entry).
Proof. reflexivity. Qed.

Lemma erase_key_commutes {V W} (denote : V -> W) key items :
  denote_bindings denote (erase_key key items) =
  erase_key key (denote_bindings denote items).
Proof.
  induction items as [|[found value] tail IH]; cbn; [reflexivity|].
  unfold denote_bindings, denote_entry in *; cbn in *.
  destruct (same_key key found); cbn; now rewrite IH.
Qed.

Theorem keyed_replacement_commutes {V W} (denote : V -> W) key value items :
  denote_bindings denote (replace_key key value items) =
  replace_key key (denote value) (denote_bindings denote items).
Proof.
  unfold replace_key, denote_bindings at 1; cbn.
  change ((key, denote value) :: denote_bindings denote (erase_key key items) =
    (key, denote value) :: erase_key key (denote_bindings denote items)).
  now rewrite erase_key_commutes.
Qed.

Theorem keyed_lookup_commutes {V W} (denote : V -> W) key items :
  option_map denote (lookup_key key items) =
  lookup_key key (denote_bindings denote items).
Proof.
  induction items as [|[found value] tail IH]; cbn; [reflexivity|].
  unfold denote_bindings, denote_entry in *; cbn in *.
  destruct (same_key key found); [reflexivity|exact IH].
Qed.

Theorem successful_keyed_replacement_is_last_wins {V} key (value : V) items :
  lookup_key key (replace_key key value items) = Some value.
Proof.
  unfold replace_key; cbn.
  destruct (same_key key key); [reflexivity|contradiction].
Qed.

Lemma erase_preserves_other_lookup {V} other key (items : bindings V) :
  other <> key -> lookup_key other (erase_key key items) = lookup_key other items.
Proof.
  intros different. induction items as [|[found value] tail IH]; cbn; [reflexivity|].
  destruct (same_key key found) as [equal|unequal].
  - subst found. destruct (same_key other key); [contradiction|exact IH].
  - cbn. destruct (same_key other found); [reflexivity|exact IH].
Qed.

Theorem successful_keyed_replacement_preserves_other_keys {V}
    other key (value : V) items :
  other <> key ->
  lookup_key other (replace_key key value items) = lookup_key other items.
Proof.
  intros different. unfold replace_key; cbn.
  destruct (same_key other key); [contradiction|].
  now apply erase_preserves_other_lookup.
Qed.

End Keyed.

Section Oneof.
Context {Tag : Type}.
Variable same_arm : forall left right : Tag, {left = right} + {left <> right}.

Definition denote_oneof {V W} (denote : V -> W) (slot : option (Tag * V)) :=
  option_map (fun item => (fst item, denote (snd item))) slot.

Definition select_oneof {V} (wanted : Tag) (fallback : V)
    (slot : option (Tag * V)) :=
  match slot with
  | Some (active, child) => if same_arm wanted active then child else fallback
  | None => fallback
  end.

Definition attach_oneof {V} (wanted : Tag) (child : V) : option (Tag * V) :=
  Some (wanted, child).

Theorem oneof_selection_commutes {V W} (denote : V -> W)
    wanted source_default target_default slot :
  denote source_default = target_default ->
  denote (select_oneof wanted source_default slot) =
  select_oneof wanted target_default (denote_oneof denote slot).
Proof.
  intros defaults. destruct slot as [[active child]|]; cbn; [|exact defaults].
  destruct (same_arm wanted active); [reflexivity|exact defaults].
Qed.

Theorem same_oneof_arm_reopens_existing_child {V} wanted (fallback child : V) :
  select_oneof wanted fallback (Some (wanted, child)) = child.
Proof. cbn. destruct (same_arm wanted wanted); [reflexivity|contradiction]. Qed.

Theorem different_oneof_arm_starts_default {V} wanted active (fallback child : V) :
  wanted <> active -> select_oneof wanted fallback (Some (active, child)) = fallback.
Proof. intros different. cbn. destruct (same_arm wanted active); [contradiction|reflexivity]. Qed.

Theorem successful_oneof_attach_commutes {V W} (denote : V -> W) wanted child :
  denote_oneof denote (attach_oneof wanted child) = attach_oneof wanted (denote child).
Proof. reflexivity. Qed.

Theorem successful_oneof_attach_selects_new_arm {V} wanted (child fallback : V) :
  select_oneof wanted fallback (attach_oneof wanted child) = child.
Proof. apply same_oneof_arm_reopens_existing_child. Qed.

End Oneof.

Inductive completion (V E : Type) :=
| Completed (value : V)
| Failed (error : E).
Arguments Completed {V E}.
Arguments Failed {V E}.

Definition denote_completion {V W E} (denote : V -> W) (result : completion V E) :=
  match result with Completed value => Completed (denote value) | Failed error => Failed error end.

Definition attach_result {P V E} (commit : P -> V -> P) (parent : P)
    (result : completion V E) : P * option E :=
  match result with
  | Completed value => (commit parent value, None)
  | Failed error => (parent, Some error)
  end.

Theorem child_attachment_commutes {P Q V W E}
    (parent_denotation : P -> Q) (child_denotation : V -> W)
    source_commit target_commit parent (result : completion V E) :
  (forall p v, parent_denotation (source_commit p v) =
    target_commit (parent_denotation p) (child_denotation v)) ->
  (parent_denotation (fst (attach_result source_commit parent result)),
   snd (attach_result source_commit parent result)) =
  attach_result target_commit (parent_denotation parent)
    (denote_completion child_denotation result).
Proof.
  intros correspondence. destruct result; cbn; [now rewrite correspondence|reflexivity].
Qed.

Theorem failed_attachment_preserves_committed_parent {P V E}
    (commit : P -> V -> P) parent (error : E) :
  attach_result commit parent (Failed error) = (parent, Some error).
Proof. reflexivity. Qed.

Theorem failed_attachment_allows_later_success {P V E}
    (commit : P -> V -> P) parent (error : E) child :
  attach_result commit (fst (attach_result commit parent (Failed error)))
    (Completed child : completion V E) = (commit parent child, None).
Proof. reflexivity. Qed.

Definition guarded_attach {P V E} (allowed : bool) (error : E)
    (commit : P -> V -> P) parent child : P * option E :=
  if allowed then attach_result commit parent (Completed child)
  else attach_result commit parent (Failed error).

Theorem rejected_api_guard_preserves_parent {P V E} (error : E)
    (commit : P -> V -> P) parent child :
  guarded_attach false error commit parent child = (parent, Some error).
Proof. reflexivity. Qed.

Theorem guarded_attachment_commutes {P Q V W E}
    (parent_denotation : P -> Q) (child_denotation : V -> W)
    source_commit target_commit allowed (error : E) parent child :
  (forall p v, parent_denotation (source_commit p v) =
    target_commit (parent_denotation p) (child_denotation v)) ->
  (parent_denotation (fst (guarded_attach allowed error source_commit parent child)),
   snd (guarded_attach allowed error source_commit parent child)) =
  guarded_attach allowed error target_commit (parent_denotation parent)
    (child_denotation child).
Proof.
  intros correspondence. destruct allowed; cbn; [now rewrite correspondence|reflexivity].
Qed.

Theorem optional_attachment_commutes {V W E} (denote : V -> W)
    parent (result : completion V E) :
  (option_map denote (fst (attach_result (fun _ child => Some child) parent result)),
   snd (attach_result (fun _ child => Some child) parent result)) =
  attach_result (fun _ child => Some child) (option_map denote parent)
    (denote_completion denote result).
Proof. destruct result; reflexivity. Qed.

Theorem repeated_attachment_commutes {V W E} (denote : V -> W)
    parent (result : completion V E) :
  (map denote (fst (attach_result append_child parent result)),
   snd (attach_result append_child parent result)) =
  attach_result append_child (map denote parent) (denote_completion denote result).
Proof. destruct result; cbn; [now rewrite repeated_append_commutes|reflexivity]. Qed.

Theorem repeated_prepend_mutant_changes_order :
  append_child [1] 2 <> 2 :: [1].
Proof. discriminate. Qed.

Theorem absent_collapsed_to_default_mutant_changes_presence :
  @None nat <> Some 0.
Proof. discriminate. Qed.

Section RankedOwnership.
Context {N : Type}.
Variable rank : N -> nat.
Variable child_edge : N -> N -> Prop.
Hypothesis edge_decreases : forall parent child,
  child_edge parent child -> rank child < rank parent.

Inductive ownership_path : N -> nat -> N -> Prop :=
| EmptyPath node : ownership_path node 0 node
| ChildPath parent child count last :
    child_edge parent child -> ownership_path child count last ->
    ownership_path parent (S count) last.

Theorem ownership_path_rank_budget parent count last :
  ownership_path parent count last -> count + rank last <= rank parent.
Proof.
  intros path. induction path; [cbn; lia|].
  pose proof (edge_decreases H). lia.
Qed.

Theorem ownership_path_length_is_schema_bounded parent count last :
  ownership_path parent count last -> count <= rank parent.
Proof. intros path. pose proof (ownership_path_rank_budget path). lia. Qed.

Theorem strict_schema_rank_excludes_positive_cycles parent count :
  ownership_path parent count parent -> count = 0.
Proof. intros path. pose proof (ownership_path_rank_budget path). lia. Qed.

End RankedOwnership.

Definition method_children {V} (target : V) (arguments : list V) :=
  arguments ++ [target].

Theorem method_child_order_commutes {V W} (denote : V -> W) target arguments :
  map denote (method_children target arguments) =
  method_children (denote target) (map denote arguments).
Proof. unfold method_children. now rewrite map_app. Qed.

Theorem method_child_order_preserves_cardinality {V} (target : V) arguments :
  length (method_children target arguments) = S (length arguments).
Proof. unfold method_children. rewrite length_app. cbn. lia. Qed.

Theorem method_child_order_retains_target_last {V} (target : V) arguments :
  last (method_children target arguments) target = target.
Proof. unfold method_children. apply last_last. Qed.

Definition semantic_optional {V} (fallback : V) (slot : option V) :=
  (match slot with Some _ => true | None => false end, reopen fallback slot).

Theorem semantic_optional_commutes {V W} (denote : V -> W)
    source_default target_default slot :
  denote source_default = target_default ->
  (fst (semantic_optional source_default slot),
   denote (snd (semantic_optional source_default slot))) =
  semantic_optional target_default (option_map denote slot).
Proof. intros defaults. destruct slot; cbn; [reflexivity|now rewrite defaults]. Qed.

Theorem semantic_optional_absent_retains_absence {V} (fallback : V) :
  semantic_optional fallback None = (false, fallback).
Proof. reflexivity. Qed.

Theorem semantic_optional_present_retains_presence {V} (fallback child : V) :
  semantic_optional fallback (Some child) = (true, child).
Proof. reflexivity. Qed.

Definition component_frontier {V} (complete : bool) (incomplete canonical : V) :=
  if complete then canonical else incomplete.

Theorem component_frontier_selects_incomplete {V} (incomplete canonical : V) :
  component_frontier false incomplete canonical = incomplete.
Proof. reflexivity. Qed.

Theorem component_frontier_selects_complete {V} (incomplete canonical : V) :
  component_frontier true incomplete canonical = canonical.
Proof. reflexivity. Qed.

Theorem component_frontier_commutes {V W} (denote : V -> W) complete
    source_incomplete source_canonical target_incomplete target_canonical :
  denote source_incomplete = target_incomplete ->
  denote source_canonical = target_canonical ->
  denote (component_frontier complete source_incomplete source_canonical) =
  component_frontier complete target_incomplete target_canonical.
Proof. intros incomplete canonical. destruct complete; assumption. Qed.

Theorem method_unrotated_mutant_changes_order :
  method_children 1 [2; 3] <> [1; 2; 3].
Proof. discriminate. Qed.

Print Assumptions absent_slot_commutes.
Print Assumptions present_slot_commutes.
Print Assumptions absent_and_present_default_remain_distinct.
Print Assumptions take_slot_commutes.
Print Assumptions reopen_commutes.
Print Assumptions take_reopen_commutes.
Print Assumptions reopen_present_preserves_child.
Print Assumptions repeated_default_commutes.
Print Assumptions repeated_append_commutes.
Print Assumptions repeated_append_preserves_order.
Print Assumptions default_entry_commutes.
Print Assumptions entry_key_update_commutes.
Print Assumptions entry_value_update_commutes.
Print Assumptions keyed_replacement_commutes.
Print Assumptions keyed_lookup_commutes.
Print Assumptions successful_keyed_replacement_is_last_wins.
Print Assumptions successful_keyed_replacement_preserves_other_keys.
Print Assumptions oneof_selection_commutes.
Print Assumptions same_oneof_arm_reopens_existing_child.
Print Assumptions different_oneof_arm_starts_default.
Print Assumptions successful_oneof_attach_commutes.
Print Assumptions successful_oneof_attach_selects_new_arm.
Print Assumptions child_attachment_commutes.
Print Assumptions failed_attachment_preserves_committed_parent.
Print Assumptions failed_attachment_allows_later_success.
Print Assumptions rejected_api_guard_preserves_parent.
Print Assumptions guarded_attachment_commutes.
Print Assumptions optional_attachment_commutes.
Print Assumptions repeated_attachment_commutes.
Print Assumptions repeated_prepend_mutant_changes_order.
Print Assumptions absent_collapsed_to_default_mutant_changes_presence.
Print Assumptions ownership_path_rank_budget.
Print Assumptions ownership_path_length_is_schema_bounded.
Print Assumptions strict_schema_rank_excludes_positive_cycles.
Print Assumptions method_child_order_commutes.
Print Assumptions method_child_order_preserves_cardinality.
Print Assumptions method_child_order_retains_target_last.
Print Assumptions semantic_optional_commutes.
Print Assumptions semantic_optional_absent_retains_absence.
Print Assumptions semantic_optional_present_retains_presence.
Print Assumptions component_frontier_selects_incomplete.
Print Assumptions component_frontier_selects_complete.
Print Assumptions component_frontier_commutes.
Print Assumptions method_unrotated_mutant_changes_order.

End ProtobufOwnedSlots.
